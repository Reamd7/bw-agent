# Git SSH Commit 签名

## 问题

bw-agent 是一个 Bitwarden-backed SSH agent，通过 SSH Agent 协议对外提供签名服务。但 Git SSH commit 签名不走 ssh-agent 协议 — 它调用 `ssh-keygen -Y sign` 作为独立程序，直接读磁盘私钥。用户的私钥只存在于 Bitwarden vault 中，无法用 ssh-keygen 签名。

## 目标

1. **完全替代 ssh-keygen** — bw-agent 作为 `gpg.ssh.program`，实现所有 `-Y` 子命令（sign、verify、find-principals、check-novalidate）
2. **私钥不落盘** — sign 操作通过 SSH Agent 协议连接运行中的 bw-agent，复用现有签名 + 审批机制
3. **UI 一键配置** — 在设置页添加 "Git Signing" 分区，一键设置 `git config --global`
4. **仅 global 级别** — 不做 `--local` 仓库级配置

## 背景：Git SSH 签名协议

Git 调用 `gpg.ssh.program` 的精确参数（源码 `gpg-interface.c`）：

```
# 签名
<program> -Y sign -n git -f <pubkey_file> [-U] <data_file>

# 验证
<program> -Y verify -n git -f <allowed_signers> -I <principal> -s <sigfile>

# 查找 principals
<program> -Y find-principals -f <allowed_signers> -s <sigfile>

# 不验证信任链的检查
<program> -Y check-novalidate -n git -s <sigfile>
```

- 数据通过临时文件传递（非 stdin）
- 签名输出写入 `<data_file>.sig`
- verify 时数据通过 stdin 传入

### ssh-keygen 精确输出格式（OpenSSH 源码验证）

| 子命令 | stdout (成功) | stderr (成功) | 退出码 |
|---|---|---|---|
| `-Y sign` | 空 | `Signing file <name>\n` + `Write signature to <name>.sig\n` | 0 |
| `-Y verify` | `Good "git" signature for <principal> with <TYPE> key SHA256:<fp>\n` | 空 | 0 |
| `-Y find-principals` | `<principal>\n`（每行一个） | 空 | 0 |
| `-Y check-novalidate` | `Good "git" signature with <TYPE> key SHA256:<fp>\n` | 空 | 0 |

失败：stdout `Could not verify signature.\n`，stderr `error: ...\n`，退出码 255。

### SSHSIG 格式

```
Signed Data（被签名的内容）:
  "SSHSIG" || string("git") || string("") || string("sha512") || H(message)

Output Blob:
  "SSHSIG" || uint32(1) || string(public_key) || string("git")
  || string("") || string("sha512") || string(signature)
```

Base64 编码后包裹在 `-----BEGIN SSH SIGNATURE-----` / `-----END SSH SIGNATURE-----` 之间。

## 方案

### 功能 A：`git-sign` CLI 子命令

#### 入口改造

`crates/bw-agent/src/main.rs` 引入 `clap`，添加子命令分发：

```
bw-agent              → 现有行为（启动 SSH agent daemon）
bw-agent git-sign ... → 新增（Git SSH 签名程序）
```

#### 新增模块 `crates/bw-agent/src/git_sign.rs`

四个 `-Y` 子命令全部实现：

**sign**：
1. 解析参数 `-Y sign -n git -f <pubkey_file> <data_file>`
2. 读取公钥文件 → 解析为 `ssh_key::PublicKey`
3. 读取数据文件
4. 用 `ssh_key::SshSig::signed_data("git", HashAlg::Sha512, data)` 构造待签名数据
5. 连接运行中的 bw-agent（通过 SSH Agent 协议）
   - Windows: 命名管道 `\\.\pipe\openssh-ssh-agent`（或 `BW_PIPE_NAME`）
   - Unix: `$SSH_AUTH_SOCK` 或 `$XDG_RUNTIME_DIR/bw-agent.sock`
6. 发送 `SSH2_AGENTC_SIGN_REQUEST`
   - RSA 密钥必须传 flag `SSH_AGENT_RSA_SHA2_512` (0x02)
   - Ed25519 传 flag 0
7. 用 `ssh_key::SshSig::new(pubkey, "git", HashAlg::Sha512, signature)` 构造 SSHSIG
8. `to_pem()` → 写入 `<data_file>.sig`
9. stderr 输出 `Signing file <name>\n` + `Write signature to <name>.sig\n`

**verify**：
1. 解析 SSHSIG 签名文件
2. 从 stdin 读取待验证数据
3. 解析 allowed_signers 文件
4. 用 `ssh_key::PublicKey::verify()` 验证签名
5. stdout 输出 `Good "git" signature for <principal> with <TYPE> key SHA256:<fp>\n`
6. 失败输出 `Could not verify signature.\n`

**find-principals**：
1. 解析 SSHSIG 签名文件，提取公钥
2. 在 allowed_signers 中查找匹配的 principal
3. 每个匹配的 principal 输出一行
4. 无匹配时 stderr 输出 `No principal matched.\n`，退出码 255

**check-novalidate**：
1. 解析 SSHSIG 签名文件
2. 验证签名自洽（不检查信任链）
3. stdout 输出 `Good "git" signature with <TYPE> key SHA256:<fp>\n`

#### 关键依赖

- **`ssh-agent-lib` blocking client** — CLI 工具用同步客户端，无需 tokio runtime
- **`ssh-key` crate（已有 v0.6.7）** — 原生支持 SSHSIG（`SshSig::signed_data()`、`SshSig::new()`、`to_pem()`）
- **`clap`** — CLI 参数解析（新增依赖）

#### 连接 agent 的方式

```
sign:     ssh-agent-lib blocking client → SSH Agent 协议 → 运行中的 bw-agent
verify:   纯本地操作，不需要连接 agent
find-principals: 纯本地操作
check-novalidate: 纯本地操作
```

只有 sign 需要连接 agent，其余三个是纯公钥操作。

### 功能 B：设置页 "Git Signing" 分区

#### 新增 Tauri commands

**`get_git_signing_status()`**：
- 读取 `git config --global` 的三个值
- 返回 `GitSigningStatus { ssh_program: Option<String>, gpg_format: Option<String>, commit_gpgsign: bool }`

**`configure_git_signing()`**：
- 获取 bw-agent 二进制路径（`std::env::current_exe()`）
- 执行三条 `git config --global` 命令：
  - `gpg.ssh.program` = `<bw-agent-path> git-sign`
  - `gpg.format` = `ssh`
  - `commit.gpgsign` = `true`
- 返回成功/失败

#### 前端 UI

在 `SettingsPage.tsx` 的 Network 分区后、Save 按钮前新增 "Git Signing" 分区：

```
── Git Signing ──
  配置 git 使用 bw-agent 进行 SSH commit 签名。

  [状态指示器]
  ✓ 已配置 — gpg.ssh.program: <path>    （已配置时）
  ⚠ 未配置                                （未配置时）

  [配置 Git SSH 签名] 按钮                 （未配置时显示）
```

样式完全复用现有模式：
- Section header: `text-lg font-medium leading-6 text-gray-900`
- Description: `mt-1 text-sm text-gray-500`
- 按钮: `bg-blue-600` primary button 样式
- Toast: 复用现有 toast 机制

#### `user.signingkey` 不由 UI 设置

用户自行管理 `user.signingkey`（每个项目有自己的配置）。bw-agent `git-sign` 的 sign 子命令从 `-f` 参数获取公钥，自动在 vault 中匹配。

## 修改文件清单

| 文件 | 改动 |
|---|---|
| `crates/bw-agent/Cargo.toml` | 添加 `clap` 依赖 |
| `crates/bw-agent/src/main.rs` | 引入 clap，添加子命令分发 |
| `crates/bw-agent/src/lib.rs` | 注册新模块 `git_sign` |
| `crates/bw-agent/src/git_sign.rs` | **新增**：SSHSIG + 四个 `-Y` 子命令实现 |
| `src-tauri/src/commands.rs` | 添加 `get_git_signing_status`、`configure_git_signing` |
| `src-tauri/src/main.rs` | 注册新 command 到 invoke_handler |
| `src/lib/tauri.ts` | 添加 TypeScript 接口和 invoke wrapper |
| `src/pages/SettingsPage.tsx` | 添加 "Git Signing" 分区 |

## 不做的事情

- ❌ `--local` 仓库级配置（最多给示例命令）
- ❌ 设置 `user.signingkey`（用户自行管理）
- ❌ ECDSA/SK 密钥支持（当前 agent 只支持 Ed25519/RSA）
- ❌ `-q` quiet 模式（Git 不传这个参数）
- ❌ 透传给 ssh-keygen（完全替代）
