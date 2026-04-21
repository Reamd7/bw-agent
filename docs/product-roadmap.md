# bw-agent 产品调研与路线图

> 调研日期：2026-04-20 · 基于代码库 commit `86e9ac2` (main HEAD) · 版本 v0.1.0

---

## 一、产品定位

**一句话**：基于 Bitwarden vault 的开源 SSH agent，具备进程链审批 + 访问审计能力。

**目标用户**：
- 使用 Bitwarden 管理 SSH 密钥的开发者
- 需要可审计 SSH 密钥使用的安全团队
- 不愿为 SSH agent 功能支付 1Password 订阅费的个人/团队

**核心价值主张**：
1. **进程链可见性** — 唯一展示完整进程树的 SSH agent（`git.exe → ssh.exe → github.com`），可检测供应链攻击
2. **持久化审计日志** — 每次密钥使用可追溯，SQLite 存储，合规场景价值大
3. **开源 + 可自托管后端** — Bitwarden 可自托管，全栈可审计
4. **免费** — 无订阅，无功能限制

---

## 二、已交付功能清单

### 认证与 Vault 访问
- Bitwarden 登录（Cloud / Self-hosted）+ Master Password
- TOTP 两步验证（6 位自动提交）
- 暴力破解防护（3 次失败 → 10 秒冷却）
- Vault 自动同步（60 秒周期 + 手动触发）
- 登录失败自动重定向回登录页

### SSH Agent 核心
- 替代系统 ssh-agent（Windows Named Pipe / Unix Socket）
- ed25519 + RSA 密钥类型支持
- 按请求审批弹窗（显示完整进程链、密钥指纹、时间戳）
- 系统通知（OS 原生通知 + 自动聚焦窗口）
- 待审批队列（Dashboard tab + 红点 badge）

### Git SSH Signing
- `bw-agent-git-sign` sidecar 完整实现（620 行）
- 5 个 `-Y` 子命令：sign / verify / find-principals / match-principals / check-novalidate
- allowed_signers 文件解析（含 RSA SHA2 算法规范化）
- Windows Named Pipe + Unix Socket 双平台 agent 连接
- 4 个单元测试覆盖

### 智能密钥路由（gh-match）
- 基于 Git 远程 URL 匹配密钥
- Shell glob 模式（`github.com/mycompany/*`）
- 回退层级：匹配 + 通用 → 仅通用 → 全部
- 支持 worktree、bare repo、子目录

### 访问审计
- SQLite 持久化访问日志
- 智能操作解析：fetch / push / archive / ls-remote / git sign
- 详情弹窗（完整进程链、命令行、密钥指纹）
- Approved / Denied 状态标记

### 安全与锁定
- 6 种自动锁定模式：超时 / 系统空闲 / 睡眠 / 锁屏 / 重启 / 从不
- 手动锁定（Dashboard 按钮）
- 运行时热切换锁定模式
- 安全内存（zeroize-on-drop）

### 系统集成
- 系统托盘（Show Window / Lock / Quit）
- 关闭按钮最小化到托盘
- 6 平台构建（Win/Mac/Linux × x64/ARM64）
- CI + Release workflow（draft release）

---

## 三、技术架构概览

```
┌─────────────────────────────────────────────────┐
│             Frontend (SolidJS + Tailwind)        │
│  Login → Dashboard (3 tabs) → Settings          │
│  3 pages · 4 components · 1 modal               │
└────────────────────┬────────────────────────────┘
                     │ Tauri IPC (12 commands / 5 events)
┌────────────────────▼────────────────────────────┐
│             Tauri Shell (Rust)                   │
│  main.rs · commands.rs · tray.rs · events.rs    │
│  system_events.rs (Win/Mac platform-specific)   │
└────────────────────┬────────────────────────────┘
                     │ Rust function calls
┌────────────────────▼────────────────────────────┐
│             Core Libraries                       │
│  bw-core: Bitwarden API · Crypto · Identity     │
│  bw-agent: SSH Agent · Routing · Approval       │
│           AccessLog · ProcessChain · GitSign    │
└─────────────────────────────────────────────────┘
```

| 技术选型 | 版本 | 用途 |
|---|---|---|
| Tauri | 2.10 | 桌面框架（Rust + WebView） |
| SolidJS | 1.9 | 前端（细粒度响应式，无 VDOM） |
| Rsbuild | 1.7 | 构建工具（Rspack-based） |
| Tailwind CSS | 4.0 | 样式 |
| SQLite (rusqlite) | 0.35 | 访问日志持久化（bundled） |
| ssh-agent-lib | 0.5 | SSH agent 协议 |
| ssh-key | 0.6 | SSH 密钥解析 + SSHSIG |
| Edition 2024 / MSRV 1.85 | — | 最新 Rust edition |

---

## 四、竞品分析

### 4.1 竞品矩阵

|  | 1Password SSH Agent | Bitwarden 原生 SSH Agent | Gitsign (Sigstore) | **bw-agent** |
|---|---|---|---|---|
| **类型** | 密码管理器内置 | 密码管理器内置 | 独立 CLI | 独立 Agent |
| **SSH Agent** | ✅ | ✅ | ❌ | ✅ |
| **Git Signing** | ✅ 一键配置 | ✅ | ✅ 无密 | ✅ |
| **审批 UI** | Touch ID / Win Hello | 基础提示 | N/A | **进程链审批** |
| **审计日志** | ❌ | ❌ | ✅ Rekor 透明日志 | **SQLite 审计** |
| **Agent Forwarding** | ✅ | ❌ | N/A | ❌ |
| **WSL 集成** | ✅ | ❌ | N/A | ❌ |
| **浏览器公钥填充** | ✅ | ❌ | N/A | ❌ |
| **自托管后端** | ❌ | ✅ | N/A | ✅ |
| **开源** | ❌ | ✅ | ✅ | ✅ |
| **价格** | $2.99-7.99/月 | 免费/$1月 | 免费 | **免费** |

### 4.2 独特优势（竞品没有的）

1. **进程链可见性** — 展示完整父→子进程树，唯一能检测 "哪个进程真正发起请求" 的 SSH agent
2. **持久化审计日志** — 每次签名操作可追溯，合规审计的敲门砖
3. **独立轻量** — 不捆绑密码管理器，一个功能做到位

### 4.3 关键差距（竞品有，我们没有）

| 差距 | 竞品 | 用户影响 | 优先级 |
|---|---|---|---|
| 一键 Git Signing 配置 | 1Password | 降低入门门槛，当前需手动编辑 gitconfig | **P1-High** |
| 生物识别审批（Touch ID / Win Hello） | 1Password | 体验提升，快速审批 | P2 |
| Agent Forwarding | 1Password | 远程开发 / CDE 场景刚需 | **P2-High** |
| WSL 集成 | 1Password | Windows 开发者群体大 | P2 |
| 浏览器公钥自动填充 | 1Password | 上传密钥到 GitHub 更方便 | P3 |

### 4.4 市场趋势

- **SSH signing 正在取代 GPG** — Git ≥2.34 默认推荐，ed25519 成为 2026 标准
- **Gitsign/Sigstore** 无密签名在 CI/CD 场景增长，但桌面端仍需密钥管理
- **ssh-git** (Electron GUI) 已停止维护 — 证明有用户需求，bw-agent 可承接
- **1Password 是标杆** — 产品成熟度最高，但闭源 + 订阅制留出了开源免费的市场空间

---

## 五、当前问题与局限

### 🔴 Critical — 安全/稳定性风险

#### 1. ~~Tray Lock 是占位符~~ ✅ 已修复 (v0.2.0)
- **位置**：`src-tauri/src/tray.rs`
- **修复**：Lock 菜单接线到 vault lock 逻辑，点击时清除 vault keys + pending 2FA + emit lock-state-changed

#### 2. ~~ApprovalQueue 无超时~~ ✅ 已修复 (v0.2.0)
- **位置**：`crates/bw-agent/src/ssh_agent.rs`
- **修复**：添加 30s 可配置超时（`config.approval_timeout_secs`），超时自动拒绝 + 清理 stale request

#### 3. ~~无 Token 刷新逻辑~~ ✅ 已修复 (v0.2.0)
- **位置**：`crates/bw-agent/src/auth.rs`, `src-tauri/src/main.rs`, `src-tauri/src/commands.rs`
- **修复**：Reunlock / 周期同步 / 手动同步 3 个路径均先尝试 `exchange_refresh_token` 再 fallback

### 🟠 High — 功能缺陷

#### 4. macOS CWD 解析是 Stub
- **位置**：`crates/bw-agent/src/process.rs:640-654`
- **现状**：仅支持获取自身进程 CWD，其他进程返回空
- **影响**：macOS 上 gh-match 路由基本失效（无法确定 git 工作目录）
- **工作量**：1 天

#### 5. 前端零测试覆盖
- **位置**：`package.json:11`
- **现状**：`test:unit` 是 `echo "placeholder"` 脚本
- **影响**：3 个页面 + 4 个组件 + 1 个 modal 完全无测试保护
- **工作量**：3-5 天（搭建 + 核心路径覆盖）

#### 6. 无 E2E 测试
- **现状**：仅有一个测试未实现的 git-sign CLI 的 PowerShell 脚本
- **影响**：完整用户流程（登录→审批→签名→锁定）无验证
- **工作量**：3-5 天

#### 7. ~~约 68 个 unwrap/expect 调用~~ ✅ 关键路径已清理 (v0.2.0)
- **修复**：`access_log.rs` 3 处 `expect("lock poisoned")` 替换为 mutex recovery + `locked.rs` 添加 Keys 长度断言 + `region::lock` 优雅 fallback

### 🟡 Medium — 设计与健壮性

| # | 问题 | 位置 | 工作量 |
|---|------|------|--------|
| ~~8~~ | ~~Windows PEB 偏移量硬编码（仅 64-bit）~~ ✅ 已修复 (v0.2.1) | `process.rs:333-335` | ~~1 天~~ |
| ~~9~~ | ~~LogTable SSH 解析仅覆盖 3 种操作~~ ✅ 已覆盖 | `LogTable.tsx:62-66` | ~~0.5 天~~ |
| ~~10~~ | ~~无数据库迁移框架~~ ✅ 已缓解 (v0.2.1) | `access_log.rs:60` | ~~1 天~~ |
| 11 | git config 手写解析 | `git_context.rs:144-175` | 已评估-可接受：只读 repo-local `remote.xxx.url`，手写解析覆盖实际场景 |
| ~~12~~ | ~~无 Crash Dump / Panic Handler~~ ✅ 已修复 (v0.2.1) | `main.rs` | ~~1 天~~ |
| ~~13~~ | ~~无登录限速~~ ✅ 已缓解 (v0.2.1) | `auth.rs:24` | ~~0.5 天~~ |

> **注**：#10 迁移失败现在会记录 debug 日志；#13 登录重试次数已提取到 `Config.max_login_attempts` 可配置。

### 🟢 Low — 打磨

| # | 问题 | 位置 |
|---|------|------|
| 14 | KeyTable 中有中文硬编码 `密钥路由规则` | `KeyTable.tsx:91` |
| ~~15~~ | ~~硬编码进程黑白名单不完整~~ ✅ 已修复 (v0.2.1) | `process.rs:17-47` |
| ~~16~~ | ~~glob 匹配不支持 `**` 和字符类~~ ✅ 已修复 (v0.2.1) | `routing.rs:29-62` |
| 17 | 根目录有死文件 `gpg-interface.c` | 项目根 — 已不存在 |
| 18 | 无暗色主题（Login 暗色 / Dashboard 亮色不统一） | UI 全局 |
| 19 | 无 i18n 框架 | UI 全局 |

---

## 六、下一步方向（按优先级）

### P0 — 必修课（堵住明显漏洞） ✅ 已完成

| # | 方向 | 工作量 | 状态 |
|---|------|--------|------|
| P0-1 | 修复 Tray Lock | 0.5 天 | ✅ `8115b91` |
| P0-2 | ApprovalQueue 超时机制 | 0.5 天 | ✅ 可配置 `config.approval_timeout_secs` |
| P0-3 | Token 刷新逻辑 | 1 天 | ✅ 3 个 sync 路径均先 refresh |

### v0.2.1 — 健壮性改进 ✅ 已完成

| # | 改进 | 状态 |
|---|------|------|
| AccessLog spawn_blocking | ✅ SQLite I/O 不再阻塞 tokio 线程 |
| Keys 长度断言 | ✅ malformed 数据不再 panic |
| Mutex lock 合并 | ✅ auth.rs 减少竞争窗口 |
| 事件发射日志 | ✅ 8 处 `let _ = emit_*` 改为 `if let Err` + warn |
| 解密失败日志 | ✅ 密钥消失时可追踪原因 |
| 审批超时/重试可配置 | ✅ `Config.approval_timeout_secs` + `Config.max_login_attempts` |
| State lock 缩减 | ✅ ssh_agent.rs 快照 state 后释放锁 |
| region::lock fallback | ✅ 不再 unwrap panic |
| Socket 清理日志 | ✅ Unix 启动失败可追踪 |

### P1 — 核心体验提升（从"能用"到"好用"）

| # | 方向 | 工作量 | 理由 |
|---|------|--------|------|
| P1-1 | **一键 Git Signing 配置向导** | 2-3 天 | 最大 UX 痛点：用户需手动编辑 gitconfig。1Password 已有此功能。UI 检测并一键设置 `gpg.format=ssh`、`gpg.ssh.program`、`user.signingkey` |
| P1-2 | **Code Signing（Windows/Mac）** | 1-2 天 | 安装包无签名 → SmartScreen/Gatekeeper 警告，严重影响信任度 |
| P1-3 | **macOS CWD 解析** | 1 天 | gh-match 路由在 macOS 上基本失效 |
| P1-4 | ~~unwrap() 清理（关键路径）~~ | ~~2-3 天~~ | ✅ 已完成 — access_log + locked.rs + region::lock |

### P2 — 差异化扩展（产品竞争力）

| # | 方向 | 工作量 | 理由 |
|---|------|--------|------|
| P2-1 | **审批策略引擎** | 3-5 天 | 允许定义规则："`github.com/myorg/*` 自动批准"。扩展现有 routing 为策略层，竞品都没有 |
| P2-2 | **Agent Forwarding** | 3-5 天 | 远程开发/CDE 场景刚需，1Password 已支持 |
| P2-3 | **日志导出 + 仪表板** | 2-3 天 | JSON/CSV 导出，简单聚合统计（按天/密钥/操作类型），企业合规敲门砖 |
| P2-4 | **WSL 集成** | 1-2 天 | Windows 上 WSL 用户是核心开发者群体 |

### P3 — 未来方向（产品愿景）

| # | 方向 | 描述 |
|---|------|------|
| P3-1 | 团队审计聚合 | 多人 access_log 聚合到中央看板，企业 SaaS 化可能 |
| P3-2 | Sigstore 集成 | SSH 签名之外增加无密签名选项 |
| P3-3 | 暗色主题 | Login 暗色 / Dashboard 亮色体验不统一 |
| P3-4 | i18n | 全英文 + 一个中文字符串，无国际化框架 |
| P3-5 | 生物识别审批 | Touch ID / Windows Hello 快速审批 |

---

## 七、推荐路线图

### v0.2.0 — 稳定性修复
```
预计工期：1-2 周
目标：堵住所有安全和稳定性漏洞

├── P0-1 修复 Tray Lock（接线到现有 state.clear() 逻辑）
├── P0-2 ApprovalQueue 添加超时（默认 30s，可配置）
├── P0-3 Token 刷新（用 refresh_token 换新 access_token）
├── P1-2 Code Signing（Windows PFX / macOS codesign + notarize）
└── P1-4 关键路径 unwrap 清理（ssh_agent.rs / access_log.rs / auth.rs）
```

### v0.3.0 — Git Signing 体验闭环
```
预计工期：2-3 周
目标：让 Git Signing 从"可用"变"好用"

├── P1-1 一键 Git Signing 配置向导
│   ├── 检测当前 git config 状态
│   ├── 自动设置 gpg.format=ssh / gpg.ssh.program / user.signingkey
│   └── 检测 vault 中的 signing key 并推荐
├── P1-3 macOS CWD 解析（proc_pidinfo + PROC_PIDVNODEPATHINFO）
├── P2-1 审批策略引擎
│   ├── 基于 remote URL / process name / working dir 的规则
│   ├── auto-approve / auto-deny / prompt 三种动作
│   └── 规则存储在 Bitwarden vault 自定义字段
└── 测试覆盖率提升（核心 Rust 模块 + 前端关键路径）
```

### v0.4.0 — 企业就绪
```
预计工期：3-4 周
目标：满足团队/企业使用场景

├── P2-3 日志导出 + 统计仪表板
│   ├── JSON / CSV 导出
│   ├── 按天 / 密钥 / 操作类型聚合
│   └── 可视化图表（日活跃签名量等）
├── P2-2 Agent Forwarding
├── P2-4 WSL 集成
└── E2E 测试框架搭建
```

### v0.5.0+ — 未来探索
```
├── 团队审计聚合（中央看板）
├── Sigstore 无密签名集成
├── 暗色主题 + i18n
├── 生物识别审批
└── Linux 系统事件检测（logind / systemd-inhibit）
```

---

## 八、核心度量建议

| 指标 | 定义 | 目标 |
|---|---|---|
| 安装成功率 | 下载 → 首次解锁 vault | > 90%（Code Signing 后） |
| Git Signing 配置完成率 | 安装 → 首次 commit sign | > 70%（一键向导后） |
| 审批响应时间 | SSH 请求 → 用户审批/拒绝 | < 5s 中位数 |
| Agent 可用性 | agent 正常运行时间占比 | > 99.9%（修复超时/panic 后） |
| 审计完整性 | 签名操作被记录的比例 | 100% |

---

## 九、风险与假设

### 风险
1. **Bitwarden 官方 SSH agent 持续迭代** — 可能逐步覆盖 bw-agent 的差异化功能
2. **Sigstore/keyless signing 增长** — 如果 GitHub 原生支持 keyless "Verified" badge，密钥管理类工具价值下降
3. **Tauri 框架成熟度** — 部分平台特定功能（如 Linux 系统事件）依赖 Tauri 能力
4. **Code Signing 成本** — Windows EV 证书 ~$400/年，Apple Developer $99/年

### 假设
1. SSH 密钥签名在 2026-2027 仍是 Git 签名的主流方案
2. Bitwarden 不会在短期内内置进程链审批和审计日志
3. 自托管 Bitwarden 的用户群体足够大，值得专门支持
4. 开源 + 免费 是有效的市场差异化策略
