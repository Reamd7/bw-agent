# Approval Sessions（时间限制自动授权）Retrospective Spec

> **Status:** ✅ IMPLEMENTED (2026-05-05)
> **Type:** Retrospective specification — documents what was built and why, for future reference.

**Goal:** 减少重复授权摩擦。用户在使用 `git commit`、`git push`、`ssh` 等操作时，无需每次手动批准，可在设定时间窗口内对特定 Key + Scope 自动授权。

**Architecture:** 纯内存 SessionStore（不持久化）注入 SSH Agent 签名流程，匹配指纹 + 可执行文件哈希后自动批准，审计日志标记 `auto_approved`。前端 ApprovalDialog 增加 "Remember" 选项，DashboardPage 新增 Sessions 管理面板。

**Tech Stack:** Rust (session_store, ssh_agent, access_log, tauri commands), TypeScript/SolidJS (ApprovalDialog, LogTable, DashboardPage)

---

## 1. 需求背景

bw-agent 的 SSH Agent 每次 `git commit`/`ssh` 签名操作都弹出审批弹窗。高频使用场景（连续 commit、CI 本地推送、SSH 批量操作）下体验极差。

用户要求：
- 可设置时间窗口（5min / 10min / 15min / 30min / 1h / 2h / 4h）自动授权
- 可限定范围：某个 Key + 某个进程（如 git.exe / ssh.exe）
- 安全性：不能让恶意进程滥用

## 2. 安全模型

### 威胁分析

| 威胁 | 缓解措施 | 残余风险 |
|---|---|---|
| 恶意进程直接连接 named pipe | Named pipe DACL 限制为当前用户 (SY/BA/AU+currentUser) | 同用户权限下无法防御 |
| PID 欺骗（进程退出后 PID 被复用） | Executable scope 校验 exe_path + SHA-256 哈希 | 无法完全防御 PID 欺骗（与 1Password/GPG/OpenSSH 相同） |
| exe 文件被替换 | SHA-256 哈希在 session 创建时快照，每次 auto-approve 重算验证 | 替换后 hash 不匹配，session 自动失效 |
| 用户误设 AnyProcess scope | UI 显示 ⚠️ 警告，明确标注风险 | 用户主动选择时仍可设置 |
| Session 泄漏到磁盘 | 纯内存， NEVER persisted；app 重启清零 | 无 |
| 忘记收回 session | TTL 硬上限 4h；Vault Lock 清除所有 session | 无 |

### 安全边界定义

```
最强防御层: Named pipe DACL (系统级, 只允许当前用户)
         ↓
会话层:    Session TTL (时间窗口限制)
         ↓  
范围层:    Key Fingerprint 匹配 + Executable Hash 验证
         ↓
审计层:    全量 access_log + auto_approved 标记 + session_id 关联
```

**关键认知:** exe_path + hash 是 UX 优化而非安全边界。PID 欺骗攻击（Google Project Zero 研究）表明进程链不可信。此功能对标 GPG agent 的 `default-cache-ttl` 和 1Password 的自动批准 — 都存在相同的 PID 欺骗残余风险。

## 3. 数据模型

### 3.1 SessionScope（Rust tagged enum）

```rust
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionScope {
    AnyProcess,
    Executable { exe_path: String, exe_hash: Vec<u8> },
}
```

**设计决策:**
- `ProcessChain` scope 被明确排除 — PID 欺骗面更大，安全性收益不足以抵消复杂度
- `Executable` scope 使用 full path + SHA-256（比 1Password 仅用 path 更强）
- `AnyProcess` = GPG agent `default-cache-ttl` 等价物，需 UI 警告

### 3.2 ApprovalSession（内部，不序列化）

```rust
pub struct ApprovalSession {
    pub id: String,              // UUID v4
    pub key_fingerprint: String, // SSH key fingerprint
    pub scope: SessionScope,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub usage_count: AtomicU64,  // AtomicU64 避免全锁计数
}
```

### 3.3 SessionInfo（IPC 序列化）

```rust
pub struct SessionInfo {
    pub id: String,
    pub key_fingerprint: String,
    pub scope: SessionScope,
    pub created_at_unix: u64,
    pub expires_at_unix: u64,
    pub remaining_secs: u64,
    pub usage_count: u64,
}
```

### 3.4 AccessLog 扩展

```sql
-- 新增列（idempotent ALTER TABLE migration）
ALTER TABLE access_log ADD COLUMN auto_approved BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE access_log ADD COLUMN session_id TEXT;
```

## 4. 核心流程

### 4.1 Session 创建（用户在 ApprovalDialog 选择 "Remember"）

```
用户点击 "Allow 15min"
  → Frontend 调用 approve_request_with_session(requestId, 900, "executable")
    → Tauri command 查询 ApprovalQueue 获取 key_fingerprint
    → 构建 SessionScope::Executable { exe_path, exe_hash: SHA256(exe_path) }
    → SessionStore::create_session(fingerprint, scope, Duration(900s))
    → ApprovalQueue::respond(requestId, true)
    → SSH 签名完成
```

### 4.2 Session 匹配（SSH Agent sign() 调用）

```
ssh_agent::sign() 被调用
  → 获取 fingerprint + client_exe
  → SessionStore::check_session(fingerprint, client_exe)
    → 惰性清理过期 session
    → 遍历 active sessions:
      - fingerprint 不匹配 → skip
      - AnyProcess → match
      - Executable → 比较 exe_path + 重算 SHA-256 比较 exe_hash
    → 匹配: usage_count.fetch_add(1), return Some(session_id)
  → Some → auto-approve: approved=true, auto_approved=true, session_id_for_log=Some(id)
  → None → 正常审批流程（创建 request → UI 弹窗 → 等待响应）
```

### 4.3 Session 生命周期

- **创建:** 用户在 ApprovalDialog 中选择 "Remember" 并确认
- **匹配:** 每次 `sign()` 调用时检查，匹配则 auto-approve + usage_count++
- **过期:** TTL 到期后，下次 `check_session()` 或 `list_active()` 时惰性清理
- **撤销:** 用户在 Sessions 面板主动 Revoke，或 Vault Lock 时全部清除
- **不持久化:** App 重启 → 所有 session 消失

## 5. API 设计

### 5.1 Tauri IPC Commands

| Command | 参数 | 返回值 | 说明 |
|---|---|---|---|
| `approve_request_with_session` | `request_id: String, duration_secs: u64, scope_type: String, scope_exe_path?: String` | `()` | 批准请求并创建 session |
| `list_active_sessions` | — | `Vec<SessionInfo>` | 列出所有 active sessions |
| `revoke_session` | `session_id: String` | `bool` | 撤销指定 session |

### 5.2 Frontend TypeScript Types

```typescript
interface SessionScope {
  type: "any_process" | "executable";
  exe_path?: string;
  exe_hash?: number[];
}

interface ApprovalSessionInfo {
  id: string;
  key_fingerprint: string;
  scope: SessionScope;
  created_at_unix: number;
  expires_at_unix: number;
  remaining_secs: number;
  usage_count: number;
}
```

### 5.3 AccessLog 新字段

```typescript
interface AccessLogEntry {
  // ... existing fields
  auto_approved: boolean;
  session_id: string | null;
}
```

## 6. 文件结构

| 文件 | 变更类型 | 职责 |
|---|---|---|
| `crates/bw-agent/src/session_store.rs` | **新增** | SessionStore, SessionScope, hash_file(), 11 tests |
| `crates/bw-agent/src/access_log.rs` | 修改 | auto_approved + session_id 列, migration, record() 扩展 |
| `crates/bw-agent/src/ssh_agent.rs` | 修改 | sign() 注入 session check, auto-approve 路径 |
| `crates/bw-agent/src/lib.rs` | 修改 | 注册 session_store 模块, start_agent() 参数扩展 |
| `crates/bw-agent/src/approval.rs` | 修改 | 新增 get_request() 方法 |
| `src-tauri/src/commands.rs` | 修改 | 3 个新 Tauri commands + lock_vault 清除 sessions |
| `src-tauri/src/main.rs` | 修改 | AppState.session_store 字段 + command 注册 |
| `src/lib/tauri.ts` | 修改 | SessionScope, ApprovalSessionInfo types + 3 invoke wrappers |
| `src/lib/store.ts` | 修改 | activeSessions state (目前未使用, 预留) |
| `src/components/ApprovalDialog.tsx` | 修改 | "Remember" 折叠区: duration 选择器 + scope radio + Allow 按钮 |
| `src/components/LogTable.tsx` | 修改 | "Auto-Approved" badge + session info in detail modal |
| `src/pages/DashboardPage.tsx` | 修改 | Sessions tab: session 卡片 + 倒计时进度条 + revoke |

## 7. UI 设计

### 7.1 ApprovalDialog 变更

- 新增可折叠 "Remember this decision" 区域
- Duration 选择器: 5min / 10min / **15min (default)** / 30min / 1h / 2h / 4h
- Scope 单选: `executable` (default) / `any_process` (带 ⚠️ 警告)
- 按钮变更: "Approve" → "Approve Once"，展开后新增 "Allow {duration}" 按钮

### 7.2 LogTable 变更

- Access log badge: `Approved` (原有) / **`Auto-Approved`** (新增, brand 色调)
- Detail modal: 新增 "Session auto-approval" 行，条件显示

### 7.3 DashboardPage Sessions Tab

- 侧边栏新增 "Sessions" nav 按钮 + 活跃 session 计数 badge
- Session 卡片: fingerprint, scope badge (⚠️/🔗), usage count, 创建时间
- 实时倒计时进度条 (1s interval, <2min 变红)
- Revoke 按钮
- 空 state: "No active sessions — Approve a request with Remember to create a session."

## 8. 测试覆盖

### 8.1 Rust Tests（93/93 passing）

| 模块 | 测试数 | 覆盖内容 |
|---|---|---|
| `session_store` | 11 | create+check (AnyProcess/Executable), hash mismatch, scope mismatch (fingerprint/exe), expiry, revoke, revoke_all, usage_count, list_active_cleans_expired, hash_file_missing |
| `access_log` | 3 (2 new) | record_and_query_auto_approved, record_auto_approved_default, 原有 log_and_query |
| `ssh_agent` | 1 | request_identities_returns_empty_when_no_entries (原有) |

### 8.2 Frontend Verification

- TypeScript 编译: 0 新增 errors（6 个 pre-existing 无关 errors）
- LSP diagnostics: 所有修改文件 clean

## 9. 关键设计决策记录

| # | 决策 | 理由 | 替代方案 |
|---|---|---|---|
| D1 | 纯内存，不持久化 | 最小攻击面，app 重启即清零 | SQLite 持久化（攻击面增大，收益低） |
| D2 | `std::sync::Mutex` 而非 tokio Mutex | 匹配 AccessLog 现有模式；session 操作极快（无 I/O） | tokio::sync::Mutex（不必要） |
| D3 | `AtomicU64` for usage_count | 避免全量锁仅为了递增计数器 | MutexGuard 内递增（锁持有时间更长） |
| D4 | 排除 `ProcessChain` scope | PID 欺骗面更大（链上每个 PID 都可被欺骗）；复杂度高，安全收益低 | 实现 ProcessChain 匹配（过度工程） |
| D5 | SHA-256 而非仅 path 比对 | 比 1Password 仅用 exe path 更强；成本极低（一次文件 hash） | 仅 path 比对（替换后无法检测） |
| D6 | 硬上限 TTL = 4h | 平衡便利性与安全；足够覆盖一个工作 session | 无上限或 24h（风险过高） |
| D7 | 惰性过期清理 | 避免后台定时器；在 check_session() 和 list_active() 时顺便清理 | dedicated cleanup thread（过度工程） |
| D8 | Vault Lock → revoke_all() | 与用户心智模型一致：锁 = 一切重置 | 保留 session（锁后仍可 auto-approve → 惊讶） |
| D9 | Session 创建时快照 hash | 防止 exe 被替换后 session 继续有效 | 不快照（替换后仍匹配 → 不安全） |
| D10 | Frontend 1s interval 倒计时 | 实时 UX，过期 session 自动从列表移除 | 静态显示（用户无法直观感知剩余时间） |

## 10. 已知限制

1. **PID 欺骗无法防御** — 与所有生产 SSH agent 相同（1Password, GPG, OpenSSH）
2. **Hash 计算开销** — 每次 auto-approve 需要读取 exe 文件计算 SHA-256。对于 <100MB 的二进制文件可忽略不计。
3. **`store.ts` 中 `activeSessions` state 预留但未使用** — DashboardPage 使用独立 signal 而非 global store。后续可统一。
4. **无 session 续期** — 过期后需要重新创建。不支持 "延长" 操作。
5. **`AnyProcess` scope 无进程级过滤** — 同一用户下任何进程都可利用。这是设计选择，非 bug。

## 11. 未来改进方向

- [ ] Session 续期（Extend）— 在 Sessions 面板添加 "Extend" 按钮
- [ ] Session 模板 — 保存常用 duration + scope 组合，一键选择
- [ ] 全局 store 统一 — 将 DashboardPage 的 session signal 迁移到 `store.ts` 的 `activeSessions`
- [ ] Session 过期 Tauri event — 后端 emit 事件，前端实时更新而非轮询
- [ ] 可配置 TTL 上限 — 目前硬编码 4h，可通过 Settings 调整

## 12. Verification Summary

| 检查项 | 结果 |
|---|---|
| `cargo test -p bw-agent` | ✅ 93/93 passed |
| `cargo clippy -p bw-agent -p bw-agent-desktop` | ✅ Clean |
| `cargo build -p bw-agent-desktop` | ✅ Success |
| `pnpm tsc --noEmit` | ✅ 0 new errors (6 pre-existing unrelated) |
| LSP diagnostics (all changed files) | ✅ Clean |
