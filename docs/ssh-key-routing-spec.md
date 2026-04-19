# SSH Key Routing 功能规格书

> **状态**: Draft
> **创建日期**: 2026-04-18
> **范围**: bw-agent crate (`crates/bw-agent`)

---

## 1. 问题陈述

拥有多个 GitHub 账户的用户在使用 SSH 时面临密钥选择问题：

- `ssh` 连接 `github.com` 时，SSH agent 会 offer 所有公钥
- GitHub 按顺序匹配公钥，选中第一个匹配的账户
- 如果先匹配到了错误的账户，操作直接失败（`ERROR: Hi xxx! You've successfully authenticated, but...`）
- 传统解决方案（`~/.ssh/config` + `Host` alias + 改写 remote URL）维护成本高且容易出错

bw-agent 已经具备**进程链追踪**能力（PID → parent PID → git 进程），可以据此推断当前操作的 git 仓库上下文，实现智能密钥路由。

---

## 2. 设计目标

1. **零额外配置文件** — 路由规则存储在 Bitwarden vault 的 custom field 中，跟着密钥走
2. **对用户透明** — 不需要改 remote URL、不需要 `Host` alias
3. **向后兼容** — 没有路由规则的 key 保持现有行为

---

## 3. 定位声明

**本功能是密钥选择辅助（Key Selection Assistance），不是安全边界（Security Boundary）。**

- 路由过滤旨在减少 SSH client 向 server offer 错误密钥的概率
- 同用户的恶意进程可以绕过 agent 直接访问密钥（通过伪造 cwd/cmdline 或直接发送 sign 请求）
- 现有的 `sign()` 审批流程才是真正的用户确认机制
- 路由只是让 "默认行为" 变得更智能，不替代任何安全检查

---

## 4. 设计决策

### 4.1 路由规则：Bitwarden Custom Field

用户在 Bitwarden vault 的 SSH Key 条目上添加自定义字段：

| 属性 | 值 |
|------|------|
| 字段名 | `gh-match` |
| 字段类型 | Text（明文，无需解密） |
| 字段值 | Shell glob pattern |
| 多值支持 | 同名字段可添加多个 |

**示例**：

```
条目: "GitHub Work Key"
  Custom Field:
    Name:  gh-match
    Type:  Text
    Value: github.com/mycompany/*
  Custom Field:
    Name:  gh-match
    Type:  Text
    Value: github.com/mycompany-*/*
```

```
条目: "GitHub Personal Key"
  Custom Field:
    Name:  gh-match
    Type:  Text
    Value: github.com/myname/*
```

**选择 custom field 的原因**：
- `entry.fields: Vec<Field>` 已经从 Bitwarden API 完整解析（`CipherField` → `db::Field`），只是从未被使用
- Text 类型的 custom field 值是明文，不需要解密
- 配置跟着密钥走，不需要额外配置文件
- 同名字段天然支持多值

### 4.2 Pattern 格式：Shell Glob

使用 shell glob 通配符（`*` 匹配任意字符序列）：

| Pattern | 匹配 |
|---------|------|
| `github.com:mycompany/*` | `github.com:mycompany/api-server` ✓ |
| `github.com:mycompany-*/*` | `github.com:mycompany-frontend/app` ✓ |
| `github.com:myname/*` | `github.com:myname/dotfiles` ✓ |
| `gitlab.com:team/*` | `gitlab.com:team/project` ✓（不限于 GitHub） |

Pattern 匹配的是 **规范化后的 remote URL 路径**（见 §5.4 URL 规范化）。

**URL 规范化规则**：所有 Git remote URL 统一转换为 `{host}/{owner}/{repo}` 格式用于匹配：

| 原始 URL | 规范化结果 |
|----------|-----------|
| `git@github.com:mycompany/repo.git` | `github.com/mycompany/repo` |
| `ssh://git@github.com/mycompany/repo.git` | `github.com/mycompany/repo` |
| `https://github.com/mycompany/repo.git` | `github.com/mycompany/repo` |
| `git@gitlab.com:team/project.git` | `gitlab.com/team/project` |

**用户写 pattern 时统一使用 `/` 分隔符**（`github.com/mycompany/*`），而非 SSH 的 `:` 分隔符。这样无论实际 remote URL 是什么格式，匹配行为一致。

### 4.3 匹配结果处理

| 匹配数 | 行为 |
|--------|------|
| 0（没有任何 key 匹配） | **Fallback**: 返回通用 key（无 `gh-match` 的 entry）；如果也没有通用 key，返回所有 |
| 1 | 只返回该 key + 通用 key |
| N（多个 key 匹配） | 弹审批对话框让用户选择 |

### 4.4 Fallback 行为

路由失败时按优先级降级：

```
1. matched entries (gh-match 匹配成功) + generic entries (无 gh-match)
2. generic entries only (有路由规则但无匹配，退回到通用 key)
3. all entries (连通用 key 都没有，完全兼容旧行为)
```

**关键**：fallback 时 **不返回** 已知不匹配的 routed entries（有 `gh-match` 字段但 pattern 不匹配的 entry）。这避免了 "fallback 模式下错误 key 被 offer" 的问题，同时降低触发 server 端 `MaxAuthTries` 的风险。

### 4.5 不在范围内

以下功能明确**不在**本 spec 范围内：

- Git `user.name` / `user.email` 自动配置
- Git commit signing key 管理
- 本地密钥文件支持（只支持 Bitwarden vault 中的 key）
- 非 Git 场景的 SSH 路由（纯 `ssh` 命令等，直接 fallback）

---

## 5. 技术规格

### 5.1 数据流

```
git push origin main
  │
  └─> ssh git@github.com
        │
        └─> bw-agent (named pipe / unix socket)
              │
              ├─ new_session(client_pid)                    // 已有
              │
              ├─ request_identities()
              │     │
              │     ├─ resolve_process_chain(client_pid)    // 已有，返回 Vec<ProcessInfo>
              │     │
              │     ├─ 找 git 进程 → 取 cwd                  // 🆕 需要新增 cwd 字段
              │     │
              │     ├─ 定位 git repo → 读 config → remote URL  // 🆕 处理 worktree 等
              │     │
              │     ├─ 规范化 remote URL → "host/owner/repo"   // 🆕
              │     │
              │     ├─ 遍历 state.entries:
              │     │    ├─ 提取 entry.fields 中 name=="gh-match" 的 value
              │     │    ├─ glob match 规范化后的 URL
              │     │    └─ 收集匹配的 entry
              │     │
              │     ├─ 路由决策 (见 §4.3/§4.4):
              │     │    ├─ matched + generic → 返回
              │     │    ├─ generic only → 返回
              │     │    └─ all → 兜底
              │     │
              │     ├─ 🆕 缓存 allowed entry IDs 到 per-session state
              │     │
              │     └─ 返回过滤后的 identities
              │
              └─ sign(request)
                    │
                    ├─ 🆕 校验: 请求的 key 是否在 session 的 allowed set 中
                    └─ 正常审批流程（已有，显示 key name + fingerprint）
```

### 5.2 进程工作目录获取

#### 5.2.1 `ProcessInfo` 结构体变更

**文件**: `crates/bw-agent/src/process.rs`

在现有 `ProcessInfo` 结构体中新增 `cwd` 字段：

```rust
pub struct ProcessInfo {
    pub exe: String,
    pub pid: u32,
    pub cmdline: String,
    pub cwd: String,          // 新增：进程工作目录
}
```

#### 5.2.2 `resolve_cwd()` 实现

**Windows** — 通过 PEB walk 读取 `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory`：

现有代码中已有 `resolve_cmdline()` 的 PEB walk 实现（`process.rs` 中通过 `NtQueryInformationProcess` 获取 PEB 地址，再 `ReadProcessMemory` 读 `ProcessParameters`）。复用相同的 PEB walk 逻辑，改为读取 `CurrentDirectory`（`RTL_USER_PROCESS_PARAMETERS` 偏移 `0x60`，`UNICODE_STRING` 结构 `{ Length, MaxLength, Buffer }`）。

**Unix** — 一行代码：

```rust
std::fs::read_link(format!("/proc/{pid}/cwd"))
```

#### 5.2.3 调用时机

在 `query_process_info()` 函数中，与 `resolve_exe()` 和 `resolve_cmdline()` 并列调用 `resolve_cwd(pid)`。失败时 `cwd` 设为空字符串（不影响现有行为）。

### 5.3 Git 仓库上下文提取

**新文件**: `crates/bw-agent/src/git_context.rs`

```rust
/// 从进程链中提取规范化后的 git remote URL。
///
/// 返回 None 表示无法确定 git context（fallback）。
pub fn extract_remote_url(process_chain: &[ProcessInfo]) -> Option<String>
```

#### 5.3.1 仓库定位算法

从进程链中找到 git 可执行文件后，需要正确定位 `.git/config`。以下是完整的边界处理：

```
1. 找到 git 进程的 cwd

2. 定位 git repo:
   a. 检查 {cwd}/.git:
      - 如果是目录 → {cwd}/.git/config
      - 如果是文件 → 读取文件内容（worktree 指针）
        格式: "gitdir: /path/to/main/repo/.git/worktrees/name"
        → 使用指向路径中的 config
   b. 向上遍历父目录重复步骤 a（最多 10 层）
      支持在 repo 子目录中执行 git 命令的场景
   c. 检查 {cwd}/config（bare repo，没有 .git 目录）
      判断条件: {cwd}/HEAD 存在

3. 读取 config，提取 remote URL:
   a. 查找当前分支的 upstream remote:
      [branch "main"] → remote = origin → url
   b. 如果无法确定分支，fallback 到 [remote "origin"]
   c. 如果有多个 GitHub remote 且无法确定 → 返回 None（ambiguous → fallback）
```

**已知的限制（不处理，遇到时 fallback）**：
- `git -C /other/path` — git cwd 可能不是实际 repo 路径（但大多数场景下 cwd 仍是 repo）
- `GIT_DIR` / `GIT_WORK_TREE` 环境变量 — 不从进程环境变量读取
- `.gitconfig` 中的 `insteadOf` / `pushInsteadOf` URL 重写 — 只处理 config 中的原始 URL
- `include` / `includeIf` — 不解析额外的 config 文件

#### 5.3.2 `.git/config` 解析

不引入 git2 或类似重量级依赖。`.git/config` 是标准 INI 格式，用简单的字符串解析：

```
[remote "origin"]
    url = git@github.com:mycompany/repo.git
    fetch = +refs/heads/*:refs/remotes/origin/*
```

解析规则：
1. 按行读取，识别 `[section "subsection"]` 段头
2. 在 `[remote "..."]` 段中查找 `url = ` 行
3. 不处理 `include`、`insteadOf` 等（见上述限制）

### 5.4 URL 规范化

**核心函数**: `normalize_remote_url(raw_url: &str) -> Option<String>`

所有 remote URL 统一转换为 `{host}/{owner}/{repo}` 格式：

```
输入: git@github.com:mycompany/repo.git     → github.com/mycompany/repo
输入: ssh://git@github.com/mycompany/repo.git → github.com/mycompany/repo
输入: https://github.com/mycompany/repo.git   → github.com/mycompany/repo
输入: git@gitlab.com:team/project.git         → gitlab.com/team/project
输入: /local/path                              → None (非网络 URL)
```

规范化步骤：
1. 去掉 scheme 前缀（`ssh://`、`https://`、`git://`）
2. 去掉用户名部分（`git@`）
3. 将 `:` 分隔符转换为 `/`（SSH 风格 `host:owner/repo` → `host/owner/repo`）
4. 去掉 `.git` 后缀
5. 去掉尾部的 `/`
6. 验证结果至少包含 `host/owner/repo` 三段，否则返回 None

### 5.5 路由匹配

**新文件**: `crates/bw-agent/src/routing.rs`

```rust
/// 从 entry 的 custom fields 中提取所有 gh-match pattern。
pub fn extract_match_patterns(entry: &bw_core::db::Entry) -> Vec<String>

/// 检查 remote URL 是否匹配任意一个 pattern（shell glob）。
pub fn matches_any_pattern(remote_url: &str, patterns: &[String]) -> bool

/// 路由决策：根据 remote URL 过滤 entries。
/// 返回 (matched_entries, fallback: bool)。
pub fn route_entries(
    entries: &[bw_core::db::Entry],
    remote_url: Option<&str>,
) -> Vec<bw_core::db::Entry>
```

**Glob 匹配实现**：

使用 `glob` crate（轻量，无 transitive 依赖）或手写简化版（只支持 `*` 通配符）。

#### 5.5.1 `extract_match_patterns` 逻辑

```rust
fn extract_match_patterns(entry: &Entry) -> Vec<String> {
    entry.fields.iter()
        .filter(|f| f.name.as_deref() == Some("gh-match"))
        .filter_map(|f| f.value.clone())
        .collect()
}
```

#### 5.5.2 `route_entries` 逻辑

```
输入: entries (所有 vault 中的 SSH key 条目), remote_url (规范化后，或 None)

if remote_url is None:
    return entries (无法获取 git context → 完全 fallback)

分类每个 entry:
    patterns = extract_match_patterns(entry)
    if patterns.is_empty():
        → generic (无路由规则)
    else if patterns 中有任一匹配 remote_url:
        → matched (路由命中)
    else:
        → excluded (有规则但不匹配，排除)

决策:
    if matched 非空:
        return matched + generic (路由命中 + 通用 key)
    else if generic 非空:
        return generic (无路由命中，用通用 key 兜底)
    else:
        return all entries (没有任何可用 key，完全兼容旧行为)
```

**关键设计**：
- `excluded` entries（有 `gh-match` 但不匹配的）**不参与 fallback**，避免错误 key 被 offer
- 通用 entries 在所有非完全-fallback 场景下都返回
- 完全 fallback（最后一档）只在没有任何通用 key 时触发

### 5.6 Per-Session 授权状态

**关键修正**：SSH agent 协议不保证 `sign()` 只接收 `request_identities()` 返回的 key。任何有 agent 访问权限的客户端可以直接发送 `sign` 请求。

因此需要 **per-session** 缓存允许的 entry 集合：

```rust
pub struct SshAgentHandler<U: UiCallback> {
    // ... 现有字段 ...
    
    /// 当前 session 允许的 entry IDs。
    /// 由 request_identities() 设置，sign() 校验。
    /// None 表示未执行过路由（首次或 fallback 场景）。
    allowed_entry_ids: Option<Vec<String>>,
}
```

**生命周期**：

| 事件 | 行为 |
|------|------|
| `new_session(pid)` | 清空 `allowed_entry_ids`（新连接） |
| `request_identities()` | 执行路由，设置 `allowed_entry_ids = Some(ids)` |
| `request_identities()` 再次调用 | 重新执行路由，更新 `allowed_entry_ids` |
| `sign(request)` | 如果 `allowed_entry_ids` 为 `Some`，校验请求的 key 在允许列表中；否则（未路由/fallback）直接放行 |

**校验失败行为**：返回 `AgentError`（与现有 "sign request denied" 行为一致），同时记录日志。

### 5.7 `request_identities()` 变更

**文件**: `crates/bw-agent/src/ssh_agent.rs`

在现有的 `request_identities()` 实现中加入路由逻辑：

```rust
async fn request_identities(&mut self) -> Result<Vec<Identity>, AgentError> {
    // ... 现有的 ensure_unlocked 逻辑 ...

    let state = self.state.lock().await;

    // 🆕 解析 git context
    let process_chain = crate::process::resolve_process_chain(self.client_pid);
    let remote_url = crate::git_context::extract_remote_url(&process_chain);

    // 🆕 路由过滤
    let entries = crate::routing::route_entries(&state.entries, remote_url.as_deref());

    // 🆕 检查是否有多个匹配需要用户选择
    let ssh_entries: Vec<_> = entries.iter()
        .filter(|e| matches!(&e.data, EntryData::SshKey { public_key: Some(_), .. }))
        .collect();

    if ssh_entries.len() > 1 && remote_url.is_some() {
        // 多个匹配 → 弹审批让用户选择
        // 复用 approval_queue 机制，但需要一个 "选择" 变体
        // TODO: 审批 UI 变体设计见 §4.6
    }

    // 现有的 identity 构建逻辑（基于过滤后的 entries）
    let mut identities = Vec::new();
    for entry in &entries {
        // ... 现有的解密和构建 identity 代码 ...
    }

    Ok(identities)
}
```

### 5.8 多匹配审批 UI 变体

当多个 SSH key 匹配到同一个 remote URL 时，需要让用户选择。

**复用现有审批机制**，但扩展 `ApprovalRequest`：

```rust
/// 新增 approval 类型
pub enum ApprovalType {
    Sign,                                        // 现有：签名审批
    ProfileSelect { candidates: Vec<String> },   // 新增：profile 选择
}

pub struct ApprovalRequest {
    pub id: String,
    pub approval_type: ApprovalType,  // 新增字段
    pub key_name: String,
    pub fingerprint: String,
    pub client_exe: String,
    pub client_pid: u32,
    pub process_chain: Vec<ProcessInfo>,
}
```

前端展示一个选择对话框而非同意/拒绝对话框。用户选择的 profile 名称传回后端，后端只返回该 profile 对应的 key。

**Cancel/Timeout 行为**：
- 用户取消选择 → `request_identities()` 返回通用 key（无 `gh-match` 的 entry）
- 审批超时（复用现有超时机制）→ 同 cancel 行为
- 这确保取消不会退回到返回所有 key（包括错误匹配的），只返回安全的通用 key

**注意**：此 UI 变体涉及前端改动（Tauri + SolidJS），本 spec 只定义接口，前端实现另行规划。

### 5.9 `sign()` 变更

`sign()` 方法需要增加 per-session 授权校验：

```rust
async fn sign(&mut self, request: SignRequest) -> Result<Signature, AgentError> {
    // ... 现有的 ensure_unlocked 逻辑 ...

    // 🆕 Per-session 授权校验
    if let Some(allowed_ids) = &self.allowed_entry_ids {
        // 查找请求的 key 对应的 entry
        let requested_entry_id = find_entry_id_by_pubkey(&state, &request.pubkey);
        if let Some(id) = requested_entry_id {
            if !allowed_ids.contains(&id) {
                log::warn!(
                    "sign() rejected: key {} not in session allowed set (routing enforcement)",
                    id
                );
                return Err(AgentError::Other(
                    "Sign request rejected: key not authorized for this session".into(),
                ));
            }
        }
        // 如果找不到对应 entry，放行（fallback 兼容）
    }
    // allowed_entry_ids 为 None → 未执行路由 → 放行

    // ... 现有的审批和解密逻辑 ...
}
```

---

## 6. 错误处理与降级

所有失败场景统一降级为 fallback 行为，**永远不阻止 SSH 认证**：

| 失败场景 | 行为 |
|----------|------|
| 进程链中无 git 进程 | `extract_remote_url` 返回 `None` → 完全 fallback |
| git 进程已退出（race condition） | `resolve_cwd` 失败 → `cwd` 为空 → `None` → fallback |
| `.git` 路径解析失败（权限等） | `extract_remote_url` 返回 `None` → fallback |
| `.git/config` 无 remote URL | `extract_remote_url` 返回 `None` → fallback |
| remote URL 无法规范化（本地路径等） | `normalize_remote_url` 返回 `None` → fallback |
| remote 有多个 GitHub 来源且无法确定 | `extract_remote_url` 返回 `None` → fallback |
| glob pattern 格式无效 | 记录日志，该 pattern 视为不匹配 |
| URL 规范化后少于 3 段 | 返回 `None` → fallback |

**日志策略**：
- 路由决策记录在 `debug` 级别（匹配结果、fallback 原因）
- `gh-match` pattern 值 **不在正常日志级别输出**（避免泄露 org/repo 信息）
- 路由失败/降级记录在 `info` 级别

---

## 7. 文件变更清单

| 文件 | 变更类型 | 说明 |
|------|----------|------|
| `crates/bw-agent/src/process.rs` | 修改 | `ProcessInfo` 加 `cwd` 字段，新增 `resolve_cwd()` |
| `crates/bw-agent/src/git_context.rs` | **新建** | Git 仓库定位 + remote URL 提取 + URL 规范化 |
| `crates/bw-agent/src/routing.rs` | **新建** | 路由匹配逻辑（pattern 提取、glob 匹配、路由决策） |
| `crates/bw-agent/src/ssh_agent.rs` | 修改 | `request_identities()` 加路由；`sign()` 加 per-session 校验；新增 `allowed_entry_ids` 字段 |
| `crates/bw-agent/src/approval.rs` | 修改 | `ApprovalRequest` 加 `approval_type` 字段 |
| `crates/bw-agent/src/lib.rs` | 修改 | 注册新模块 `git_context`, `routing` |
| `crates/bw-agent/Cargo.toml` | 修改 | 可选：添加 `glob` crate 依赖 |

**不需要变更的**：
- `crates/bw-core/` — 完全不需要动，custom fields 已在解析
- `crates/bw-agent/src/auth.rs` — 解密逻辑不变
- `crates/bw-agent/src/pipe.rs` — PID 提取逻辑不变

---

## 8. 测试计划

### 8.1 单元测试

| 测试 | 文件 | 验证点 |
|------|------|--------|
| `test_extract_match_patterns` | `routing.rs` | 正确提取 `gh-match` 字段值，忽略其他字段 |
| `test_extract_match_patterns_empty` | `routing.rs` | 无 `gh-match` 字段时返回空 Vec |
| `test_extract_match_patterns_multiple` | `routing.rs` | 多个 `gh-match` 字段全部提取 |
| `test_glob_match_star` | `routing.rs` | `github.com/mycompany/*` 匹配 `github.com/mycompany/repo` |
| `test_glob_match_exact` | `routing.rs` | 无通配符的 pattern 精确匹配 |
| `test_glob_no_match` | `routing.rs` | 不匹配的 pattern 返回 false |
| `test_glob_invalid_pattern` | `routing.rs` | 无效 pattern 不 crash，返回 false |
| `test_route_entries_no_rules` | `routing.rs` | 无 `gh-match` 的 entries 在所有场景下都返回 |
| `test_route_entries_single_match` | `routing.rs` | 单个匹配时返回 matched + generic |
| `test_route_entries_fallback_generic` | `routing.rs` | 0 matched 但有 generic → 返回 generic only |
| `test_route_entries_fallback_all` | `routing.rs` | 0 matched 且 0 generic → 返回 all |
| `test_route_entries_no_remote_url` | `routing.rs` | 无 git context 时返回所有 entries |
| `test_route_entries_excluded_not_in_fallback` | `routing.rs` | excluded entries 不出现在 generic fallback 中 |
| `test_normalize_url_ssh` | `git_context.rs` | `git@github.com:org/repo.git` → `github.com/org/repo` |
| `test_normalize_url_ssh_scheme` | `git_context.rs` | `ssh://git@github.com/org/repo.git` → `github.com/org/repo` |
| `test_normalize_url_https` | `git_context.rs` | `https://github.com/org/repo.git` → `github.com/org/repo` |
| `test_normalize_url_local_path` | `git_context.rs` | `/local/path` → `None` |
| `test_normalize_url_trailing_slash` | `git_context.rs` | 去掉尾部的 `/` |
| `test_extract_remote_url` | `git_context.rs` | 从 `.git/config` 内容正确提取 remote URL |
| `test_extract_remote_url_no_origin` | `git_context.rs` | 无 origin remote 时返回 None |
| `test_extract_remote_url_worktree` | `git_context.rs` | `.git` 是文件（worktree 指针）时正确解析 |
| `test_extract_remote_url_bare_repo` | `git_context.rs` | bare repo（无 `.git` 目录）时从 `config` 读取 |
| `test_extract_remote_url_submodule` | `git_context.rs` | cwd 在 submodule 内时正确解析 |
| `test_resolve_cwd` | `process.rs` | 平台相关的 cwd 解析（mock 或真实进程） |
| `test_session_allowed_ids_enforcement` | `ssh_agent.rs` | `sign()` 拒绝不在 allowed set 中的 key |
| `test_session_allowed_ids_none_passthrough` | `ssh_agent.rs` | `allowed_entry_ids = None` 时 `sign()` 放行所有 |

### 8.2 集成测试

| 测试 | 验证点 |
|------|--------|
| 无路由规则的 key 全部返回 | 确认向后兼容 |
| 有路由规则时只返回 matched + generic | 路由过滤生效 |
| 非 git 场景完全 fallback | `ssh -T git@github.com` 时返回所有 key |
| 多 key 匹配弹审批 | 两个 key 都匹配时触发选择对话框 |
| 审批取消返回 generic only | 取消选择不返回所有 key |
| `sign()` 被路由约束 | 非 allowed key 的 sign 请求被拒绝 |

### 8.3 手动 QA

1. 在 Bitwarden vault 中创建两个 SSH key 条目，各加 `gh-match` 字段
2. 在两个不同 git repo 中分别 `git push`，确认使用正确的 key
3. 纯 `ssh -T git@github.com` 时确认 fallback 行为
4. 两个 key 匹配同一 remote 时确认弹窗选择行为
5. 在 git worktree 中 `git push`，确认路由正常工作
6. 在 repo 子目录中 `git push`，确认路由正常工作

---

## 9. 实现顺序建议

```
Phase 1: 基础设施（无 UI 改动，纯后端，三个 step 可并行）
  ├─ Step 1: process.rs — 加 cwd 字段 + resolve_cwd()
  ├─ Step 2: git_context.rs — 新建，仓库定位 + remote URL 提取 + URL 规范化
  └─ Step 3: routing.rs — 新建，pattern 匹配 + 路由决策

Phase 2: 集成
  ├─ Step 4: ssh_agent.rs — request_identities() 加路由 + sign() 加 per-session 校验
  └─ Step 5: lib.rs — 注册新模块 + Cargo.toml 加依赖

Phase 3: 多匹配审批（可后续迭代）
  ├─ Step 6: approval.rs — 扩展 ApprovalRequest
  └─ Step 7: 前端选择对话框

Phase 4: 测试
  └─ Step 8: 单元测试 + 集成测试 + 手动 QA
```

**Phase 1 的三个 step 之间无依赖，可并行开发。**

Phase 3 的多匹配审批可以在 Phase 2 完成后独立迭代。Phase 2 阶段，多匹配场景可以暂时 fallback 返回所有匹配的 key（让 SSH client 按顺序 offer），不影响核心功能。
