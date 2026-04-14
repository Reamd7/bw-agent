# Bitwarden SSH Agent 实施计划

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**目标：** 构建一个跨平台的 SSH agent，从 Bitwarden 保险库提供 SSH 密钥，使用 egui 进行主密码提示，并具有 15 分钟的身份验证缓存。

**架构：** 从 rbw (`.reference/rbw/`) fork 核心加密/API 逻辑，将仅限 Unix 的组件（daemonize, Unix socket, pinentry）替换为跨平台替代方案（前台进程, ssh-agent-lib NamedPipe, egui 对话框）。该项目是一个 Cargo workspace，包含三个 crate：`bw-core`（加密 + API）、`bw-ui`（egui 对话框）、`bw-agent`（二进制文件）。

**技术栈：** Rust 2024 版 (MSRV 1.85.0), tokio, reqwest (带代理), ssh-agent-lib 0.5.x, eframe/egui 0.34, AES-256-CBC + HMAC-SHA256, PBKDF2/Argon2id, RSA-OAEP

**参考：** 所有 "Copy from rbw" 指令均指本工作区中的 `.reference/rbw/src/`。rbw 使用较旧的 crate 版本（Rust 2021, RustCrypto 0.8.x/0.10.x 时代）。我们使用最新版本 —— **复制的代码将需要针对新的 RustCrypto / rand / reqwest API 进行 API 调整**。请参阅下面的迁移说明。

**代理：** 所有 reqwest HTTP 客户端必须配置代理 `http://127.0.0.1:7890`。

### ⚠️ rbw 依赖版本的破坏性变更

以下 crate 与 rbw 使用的版本相比存在 **破坏性 API 变更**。从 rbw 复制的代码必须进行适配：

| Crate | rbw 版本 | 我们的版本 | 对复制内容的影响 |
|---|---|---|---|
| `aes` | 0.8.4 | 0.9.0 | `cipher` trait 导入可能不同 |
| `cbc` | 0.1.2 | 0.2.0 | `Encryptor`/`Decryptor` 构造函数 API 已更改 |
| `block-padding` | 0.3.3 | 0.4.2 | `UnpadError` 类型路径可能不同 |
| `hmac` | 0.12.1 | 0.13.0 | `Mac` trait → 检查新 API |
| `hkdf` | 0.12.4 | 0.13.0 | `Hkdf::from_prk` / `expand` API 可能不同 |
| `sha1` | 0.10.6 | 0.11.0 | `Digest` trait 可能不同 |
| `sha2` | 0.10.9 | 0.11.0 | `Digest` trait 可能不同 |
| `rand` | 0.9.2 | 0.10.1 | `RngCore`, `rng()` API 已更改；rbw 用于 RSA 签名的 `rand_8` (0.8.5) 也需要更新 |
| `reqwest` | 0.12 | 0.13.2 | Client builder API 可能不同 |
| `eframe` | 0.31 | 0.34.1 | `run_native` API, `ViewportCommand` 可能不同 |

**策略：** 先复制 rbw 代码，然后根据新 API 修复编译错误。RustCrypto crate（aes/cbc/hmac/hkdf/sha1/sha2/block-padding）作为一个家族共同更新 —— 请同时使用它们的新版本。查看 https://github.com/RustCrypto 的迁移指南。

---

## 文件结构

```
bw-agent/
├── Cargo.toml                        # 工作区根目录
├── crates/
│   ├── bw-core/                      # 库：加密, API, 类型
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                # 模块声明
│   │       ├── prelude.rs            # 重新导出 Error/Result
│   │       ├── error.rs              # 错误类型 (从 rbw 精简)
│   │       ├── base64.rs             # Base64 辅助工具 (从 rbw 复制)
│   │       ├── json.rs               # 带路径的 JSON 反序列化 (从 rbw 复制)
│   │       ├── locked.rs             # 内存锁定缓冲区 (从 rbw 复制)
│   │       ├── identity.rs           # KDF + 主密钥派生 (从 rbw 复制)
│   │       ├── cipherstring.rs       # AES/RSA 加密/解密 (从 rbw 复制)
│   │       ├── api.rs                # Bitwarden REST API (精简 + 代理)
│   │       └── db.rs                 # 保险库条目类型 (精简, 无文件 I/O)
│   ├── bw-ui/                        # 库：egui 密码对话框
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs                # 一次性密码提示窗口
│   └── bw-agent/                     # 二进制文件：SSH agent
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs               # 入口点, 启动 SSH agent 监听器
│           ├── ssh_agent.rs           # SSH agent Session 实现 (来自 rbw)
│           ├── auth.rs               # 验证流程：登录 → 同步 → 解锁 → 缓存
│           └── state.rs              # 共享状态：缓存的密钥 + TTL
```

**边界规则：**
- `bw-core` 零平台相关代码。没有 `std::os::unix`，没有 `std::os::windows`。
- `bw-ui` 仅依赖 `eframe`/`egui`。不依赖 `bw-core`。
- `bw-agent` 依赖 `bw-core` 和 `bw-ui`，以及 `ssh-agent-lib`。

---

## Chunk 1: 项目脚手架 + bw-core 加密链

此 Chunk 设置工作区并从 rbw 复制平台无关的加密模块。

### Task 1: 工作区脚手架

**文件：**
- 修改：`Cargo.toml` (工作区根目录)
- 创建：`crates/bw-core/Cargo.toml`
- 创建：`crates/bw-core/src/lib.rs`
- 创建：`crates/bw-core/src/prelude.rs`
- 创建：`crates/bw-ui/Cargo.toml`
- 创建：`crates/bw-ui/src/lib.rs`
- 创建：`crates/bw-agent/Cargo.toml`
- 创建：`crates/bw-agent/src/main.rs`
- 删除：`crates/example/` (不再需要)

- [ ] **Step 1: 更新工作区根目录 Cargo.toml**

```toml
[workspace]
resolver = "2"
members = [
    "crates/bw-core",
    "crates/bw-ui",
    "crates/bw-agent",
]

[workspace.package]
edition = "2024"
rust-version = "1.85.0"
license = "MIT"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"
module_name_repetitions = "allow"
```

- [ ] **Step 2: 创建 bw-core Cargo.toml**

```toml
[package]
name = "bw-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
aes = "0.9.0"
argon2 = "0.5.3"
arrayvec = "0.7.6"
base64 = "0.22.1"
block-padding = "0.4.2"
cbc = { version = "0.2.0", features = ["alloc", "std"] }
hkdf = "0.13.0"
hmac = { version = "0.13.0", features = ["std"] }
pbkdf2 = "0.12.2"
percent-encoding = "2.3.2"
pkcs8 = "0.10.2"
rand = "0.10.1"
region = "3.0.2"
reqwest = { version = "0.13.2", default-features = false, features = ["json", "rustls-tls-native-roots"] }
rsa = "0.9.10"
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
serde_path_to_error = "0.1.20"
serde_repr = "0.1.20"
sha1 = "0.11.0"
sha2 = "0.11.0"
thiserror = "2.0.18"
tokio = { version = "1.51", features = ["fs", "io-util"] }
url = "2.5.8"
uuid = { version = "1.23", features = ["v4"] }
zeroize = "1.8.2"
log = "0.4.29"

[lints]
workspace = true
```

- [ ] **Step 3: 创建 bw-core/src/lib.rs (存根 —— 模块将逐步添加)**

从仅包含 `prelude` 和 `error` 作为占位符开始。每个后续任务在创建文件时都会添加其 `pub mod` 声明。

```rust
pub mod error;
pub mod prelude;
```

**Task 2** 将添加：`pub mod base64; pub mod json; pub mod locked;`
**Task 3** 将添加：`pub mod identity; pub mod cipherstring;`
**Task 4+5** 将添加：`pub mod api; pub mod db;` (db 依赖于 api 类型，因此两者都在 Task 5 之后一起添加)

- [ ] **Step 4: 创建 bw-core/src/prelude.rs**

逐字复制自 `.reference/rbw/src/prelude.rs`：

```rust
pub use crate::error::{Error, Result};
```

- [ ] **Step 5: 创建存根 bw-ui/Cargo.toml 和 lib.rs**

`crates/bw-ui/Cargo.toml`:
```toml
[package]
name = "bw-ui"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
eframe = "0.34.1"

[lints]
workspace = true
```

`crates/bw-ui/src/lib.rs`:
```rust
// 占位符 - 在 Task 7 中实现
pub fn prompt_master_password() -> Option<String> {
    todo!("egui password prompt")
}
```

- [ ] **Step 6: 创建存根 bw-agent/Cargo.toml 和 main.rs**

`crates/bw-agent/Cargo.toml`:
```toml
[package]
name = "bw-agent"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
bw-core = { path = "../bw-core" }
bw-ui = { path = "../bw-ui" }
anyhow = "1.0.102"
log = "0.4.29"
env_logger = "0.11.10"
ssh-agent-lib = "0.5.2"
ssh-key = { version = "0.6.7", features = ["ed25519", "rsa"] }
rsa = "0.9.10"
sha1 = "0.11.0"
sha2 = "0.11.0"
signature = "2.2.0"
rand = "0.10.1"
tokio = { version = "1.51", features = ["full"] }

[lints]
workspace = true
```

`crates/bw-agent/src/main.rs`:
```rust
fn main() {
    println!("bw-agent placeholder");
}
```

- [ ] **Step 7: 删除 crates/example 目录**

```bash
rm -rf crates/example
```

- [ ] **Step 8: 验证工作区编译**

```bash
cargo check --workspace
```

预期：成功（仅存根）

- [ ] **Step 9: 提交**

```bash
git add -A && git commit -m "scaffold: workspace with bw-core, bw-ui, bw-agent crates"
```

---

### Task 2: 复制核心加密模块 (base64, json, locked, error)

这四个文件是小型实用程序，需要极少或不需要更改。

**文件：**
- 创建：`crates/bw-core/src/base64.rs`
- 创建：`crates/bw-core/src/json.rs`
- 创建：`crates/bw-core/src/locked.rs`
- 创建：`crates/bw-core/src/error.rs`

- [ ] **Step 0: 将新模块连接到 lib.rs**

在 `crates/bw-core/src/lib.rs` 中添加：

```rust
pub mod base64;
pub mod json;
pub mod locked;
```

- [ ] **Step 1: 逐字复制 base64.rs**

复制 `.reference/rbw/src/base64.rs` → `crates/bw-core/src/base64.rs`。无需更改。

- [ ] **Step 2: 复制 json.rs，移除阻塞实现**

复制 `.reference/rbw/src/json.rs` → `crates/bw-core/src/json.rs`。**移除** `impl DeserializeJsonWithPath for reqwest::blocking::Response` 块（rbw 中的第 15-23 行）—— 我们不启用 reqwest 的 `blocking` 特性。仅保留 `String` 实现和异步 `reqwest::Response` 实现。

- [ ] **Step 3: 逐字复制 locked.rs**

复制 `.reference/rbw/src/locked.rs` → `crates/bw-core/src/locked.rs`。无需更改。

- [ ] **Step 4: 创建 error.rs —— 精简版**

复制 `.reference/rbw/src/error.rs` → `crates/bw-core/src/error.rs`。移除这些我们不需要的变体：
- `ConfigMissingEmail` (我们以不同方式处理配置)
- `CreateDirectory` (无磁盘数据库)
- `CreateSSOCallbackServer` (无 SSO)
- `FailedToFindFreePort` (无 SSO)
- `FailedToOpenWebBrowser` (无 SSO)
- `FailedToProcessSSOCallback` (无 SSO)
- `FailedToReadFromStdin` (无 CLI)
- `FailedToFindEditor` (无 CLI)
- `FailedToRunEditor` (无 CLI)
- `FailedToParsePinentry` (无 pinentry)
- `IncorrectApiKey` (无 API 密钥验证)
- `InvalidEditor` (无 CLI)
保留 `InvalidTwoFactorProvider` — 它在 `api.rs` 的 `TwoFactorProviderType` 解析中使用。
- `LoadConfig*` (简化配置)
- `LoadDb*` (无磁盘数据库)
- `LoadDeviceId` (内联)
- `LoadClientCert` (无客户端证书)
- `ParseMatchType` (不需要)
- `PinentryCancelled` (无 pinentry)
- `PinentryErrorMessage` (无 pinentry)
- `PinentryReadOutput` (无 pinentry)
- `PinentryWait` (无 pinentry)
- `RegistrationRequired` (无注册流程)
- `RemoveDb` (无磁盘数据库)
- `SaveConfig*` (简化)
- `SaveDb*` (无磁盘数据库)
- `Spawn` (无 pinentry 子进程)
- `WriteStdin` (无 pinentry)

保留这些变体：
- `CreateBlockMode`
- `CreateHmac`
- `CreateReqwestClient`
- `Decrypt`
- `HkdfExpand`
- `IncorrectPassword`
- `InvalidBase64`
- `InvalidCipherString`
- `InvalidMac`
- `InvalidKdfType`
- `Json`
- `Padding`
- `Pbkdf2ZeroIterations`
- `Pbkdf2`
- `Argon2`
- `RequestFailed`
- `RequestUnauthorized`
- `Reqwest`
- `Rsa`
- `RsaPkcs8`
- `TooOldCipherStringType`
- `TwoFactorRequired`
- `UnimplementedCipherStringType`

生成的 `error.rs`：

```rust
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("failed to create block mode decryptor")]
    CreateBlockMode { source: aes::cipher::InvalidLength },

    #[error("failed to create HMAC")]
    CreateHmac { source: aes::cipher::InvalidLength },

    #[error("failed to create reqwest client")]
    CreateReqwestClient { source: reqwest::Error },

    #[error("failed to decrypt")]
    Decrypt { source: block_padding::UnpadError },

    #[error("failed to expand with hkdf")]
    HkdfExpand,

    #[error("{message}")]
    IncorrectPassword { message: String },

    #[error("invalid base64")]
    InvalidBase64 { source: base64::DecodeError },

    #[error("invalid cipherstring: {reason}")]
    InvalidCipherString { reason: String },

    #[error("invalid mac")]
    InvalidMac,

    #[error("invalid kdf type: {ty}")]
    InvalidKdfType { ty: String },

    #[error("failed to parse JSON")]
    Json {
        source: serde_path_to_error::Error<serde_json::Error>,
    },

    #[error("invalid padding")]
    Padding,

    #[error("pbkdf2 requires at least 1 iteration (got 0)")]
    Pbkdf2ZeroIterations,

    #[error("failed to run pbkdf2")]
    Pbkdf2,

    #[error("failed to run argon2")]
    Argon2,

    #[error("api request returned error: {status}")]
    RequestFailed { status: u16 },

    #[error("api request unauthorized")]
    RequestUnauthorized,

    #[error("error making api request")]
    Reqwest { source: reqwest::Error },

    #[error("failed to decrypt RSA")]
    Rsa { source: rsa::errors::Error },

    #[error("failed to parse RSA PKCS8")]
    RsaPkcs8 { source: rsa::pkcs8::Error },

    #[error("cipherstring type {ty} too old\n\nPlease rotate your account encryption key (https://bitwarden.com/help/article/account-encryption-key/) and try again.")]
    TooOldCipherStringType { ty: String },

    #[error("invalid two factor provider type: {ty}")]
    InvalidTwoFactorProvider { ty: String },

    #[error("two factor required")]
    TwoFactorRequired {
        providers: Vec<crate::api::TwoFactorProviderType>,
        sso_email_2fa_session_token: Option<String>,
    },

    #[error("unimplemented cipherstring type: {ty}")]
    UnimplementedCipherStringType { ty: String },
}

pub type Result<T> = std::result::Result<T, Error>;
```

- [ ] **Step 5: 验证 bw-core 编译**

```bash
cargo check -p bw-core
```

预期：成功。如果有导入错误，请修复 `crate::` 路径。

- [ ] **Step 6: 提交**

```bash
git add -A && git commit -m "feat(bw-core): add base64, json, locked, error modules from rbw"
```

---

### Task 3: 复制 identity.rs 和 cipherstring.rs

这些是加密核心 —— KDF 派生和 AES/RSA 加密/解密。

**文件：**
- 创建：`crates/bw-core/src/identity.rs`
- 创建：`crates/bw-core/src/cipherstring.rs`

- [ ] **Step 0: 将新模块连接到 lib.rs**

在 `crates/bw-core/src/lib.rs` 中添加：

```rust
pub mod identity;
pub mod cipherstring;
```

- [ ] **Step 1: 复制 identity.rs 并适配新的加密 API**

复制 `.reference/rbw/src/identity.rs` → `crates/bw-core/src/identity.rs`。然后针对新的 crate 版本进行修复：
- `sha2 0.11`：检查 `Digest` trait 导入路径是否已更改（`sha2::Digest` → 可能现在是 `digest::Digest`）。
- `hmac 0.13` / `pbkdf2 0.12`：验证 `pbkdf2::pbkdf2::<hmac::Hmac<sha2::Sha256>>()` 签名是否未更改。
- `hkdf 0.13`：验证 `Hkdf::from_prk()` 和 `.expand()` 签名。
- `rand 0.10`：`rand::rng()` 可能已更改 —— 检查它是 `rand::rng()` 还是 `rand::thread_rng()`。

- [ ] **Step 2: 复制 cipherstring.rs 并适配新的加密 API**

复制 `.reference/rbw/src/cipherstring.rs` → `crates/bw-core/src/cipherstring.rs`。然后针对新的 crate 版本进行修复：
- `aes 0.9` / `cbc 0.2`：来自 `aes::cipher` 的 `BlockDecryptMut`, `BlockEncryptMut`, `KeyIvInit` trait 可能已移动。检查 `cipher` crate 的重新导出。
- `cbc::Encryptor::<aes::Aes256>::new(key, iv)` 构造函数可能有不同的签名。
- `hmac 0.13`：`hmac::Hmac::new_from_slice()` 和 `Mac` trait —— 验证 API。
- `block-padding 0.4`：`UnpadError` 类型 —— 验证它是否匹配 `thiserror` 的 `#[error]` 源。
- `rand 0.10`：`rand::RngCore` → `fill_bytes` API —— 验证。
- `pkcs8 0.10` / `rsa 0.9.10`：`DecodePrivateKey`, `RsaPrivateKey::from_pkcs8_der()`, `Oaep::new::<sha1::Sha1>()` —— 验证签名。

- [ ] **Step 3: 验证加密链编译**

```bash
cargo check -p bw-core
```

预期：成功。

- [ ] **Step 4: 运行 cipherstring 测试**

```bash
cargo test -p bw-core -- cipherstring
```

预期：`test_pkcs7_unpad` 通过（rbw 的 cipherstring.rs 中唯一的测试）。

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "feat(bw-core): add identity and cipherstring crypto modules from rbw"
```

---

## Chunk 2: bw-core API 客户端 + 类型

### Task 4: 创建 api.rs 类型存根 + db.rs —— 保险库条目类型

db.rs 依赖于来自 api.rs 的类型（`CipherRepromptType`, `FieldType`, `LinkedIdType`, `UriMatchType`）。我们在此任务中创建这两个文件：首先是仅包含类型定义的 api.rs，然后是 db.rs。Client 实现和 action 函数在 Task 5 中添加。

**文件：**
- 创建：`crates/bw-core/src/api.rs` (仅限类型 —— 枚举, serde 结构体)
- 创建：`crates/bw-core/src/db.rs`

- [ ] **Step 0: 将两个模块连接到 lib.rs**

在 `crates/bw-core/src/lib.rs` 中添加：

```rust
pub mod api;
pub mod db;
```

- [ ] **Step 1: 创建仅包含类型定义的 api.rs**

仅从 `.reference/rbw/src/api.rs` 复制公共类型定义（没有 `Client`，没有函数）：
- `UriMatchType` 枚举 + Display 实现
- `TwoFactorProviderType` 枚举 + 所有实现
- `KdfType` 枚举 + 所有实现
- `CipherRepromptType` 枚举
- `FieldType` 枚举
- `LinkedIdType` 枚举

此文件将在 Task 5 中扩展，添加 Client 结构体和所有私有请求/响应类型。

- [ ] **Step 2: 创建仅包含类型的 db.rs**

从 `.reference/rbw/src/db.rs` 复制以下内容：
- `Entry` 结构体
- `Uri` 结构体 + 其自定义 Deserialize 实现
- `EntryData` 枚举 (Login, Card, Identity, SecureNote, SshKey)
- `Field` 结构体
- `HistoryEntry` 结构体

不要复制：
- `Db` 结构体 (我们不持久化到磁盘)
- `Db::load()`, `Db::save()` 等。
- 任何 `std::fs` 或 `tokio::fs` 的使用

生成的文件应定义 `api.rs` 同步响应映射到的数据类型。保持所有 `crate::api::*` 引用完整（它们将解析到我们精简后的 api.rs）。

- [ ] **Step 2: 为 EntryData::SshKey 添加往返 serde 测试**

创建 `crates/bw-core/src/db.rs` 内联测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssh_key_entry_roundtrip() {
        let entry = Entry {
            id: "test-id".to_string(),
            org_id: None,
            folder: None,
            folder_id: None,
            name: "My SSH Key".to_string(),
            data: EntryData::SshKey {
                private_key: Some("2.encrypted_privkey".to_string()),
                public_key: Some("2.encrypted_pubkey".to_string()),
                fingerprint: Some("2.encrypted_fp".to_string()),
            },
            fields: vec![],
            notes: None,
            history: vec![],
            key: None,
            master_password_reprompt: crate::api::CipherRepromptType::None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let deserialized: Entry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, deserialized);
        assert!(matches!(deserialized.data, EntryData::SshKey { .. }));
    }
}
```

运行：`cargo test -p bw-core -- db::tests`
预期：通过

- [ ] **Step 3: 验证编译**

```bash
cargo check -p bw-core
```

- [ ] **Step 4: 提交**

```bash
git add -A && git commit -m "feat(bw-core): add vault entry types from rbw (no file I/O)"
```

---

### Task 5: 扩展 api.rs —— Bitwarden REST API 客户端 (精简 + 代理)

Task 4 创建了包含公共类型定义的 api.rs。现在我们添加 Client、私有请求/响应结构体和 action 函数。

我们保留：prelogin, login (仅限密码), sync, refresh token。
我们移除：register, SSO, 添加/编辑/移除密码项, 文件夹, send_email_login, 阻塞变体。

**文件：**
- 修改：`crates/bw-core/src/api.rs` (使用 Client + 私有类型进行扩展)

- [ ] **Step 1: 向 api.rs 添加私有请求/响应类型**

将这些类型追加到现有的 `api.rs`（其中已包含 Task 4 中的公共枚举）：
- `PreloginReq` / `PreloginRes`
- `ConnectTokenReq` / `ConnectTokenAuth` / `ConnectTokenPassword` (移除 `ConnectTokenAuthCode` 和 `ConnectTokenClientCredentials`)
- `ConnectTokenRes`
- `ConnectErrorRes` / `ConnectErrorResErrorModel`
- `ConnectRefreshTokenReq` / `ConnectRefreshTokenRes`
- `SyncRes` / `SyncResCipher` + `to_entry()` 方法
- `SyncResProfile` / `SyncResProfileOrganization`
- `SyncResFolder`
- `CipherLogin` / `CipherLoginUri` / `CipherCard` / `CipherIdentity` / `CipherSshKey` / `CipherSecureNote`
- `CipherField`
- `SyncResPasswordHistory`
- `classify_login_error` 函数

- [ ] **Step 2: 创建支持代理的 Client 结构体**

用此版本替换 rbw 的 `Client`：

```rust
const BITWARDEN_CLIENT: &str = "cli";
const DEVICE_TYPE: u8 = 8;

#[derive(Debug, Clone)]
pub struct Client {
    base_url: String,
    identity_url: String,
    proxy: Option<String>,
}

impl Client {
    pub fn new(base_url: &str, identity_url: &str, proxy: Option<&str>) -> Self {
        Self {
            base_url: base_url.to_string(),
            identity_url: identity_url.to_string(),
            proxy: proxy.map(String::from),
        }
    }

    /// 为官方 Bitwarden 云配置一个新的 Client。
    pub fn bitwarden_cloud(proxy: Option<&str>) -> Self {
        Self::new(
            "https://api.bitwarden.com",
            "https://identity.bitwarden.com",
            proxy,
        )
    }

    fn reqwest_client(&self) -> crate::error::Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .user_agent(format!("bw-agent/{}", env!("CARGO_PKG_VERSION")));

        if let Some(proxy_url) = &self.proxy {
            let proxy = reqwest::Proxy::all(proxy_url)
                .map_err(|source| crate::error::Error::CreateReqwestClient { source })?;
            builder = builder.proxy(proxy);
        }

        builder
            .build()
            .map_err(|source| crate::error::Error::CreateReqwestClient { source })
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn identity_url(&self, path: &str) -> String {
        format!("{}{}", self.identity_url, path)
    }
}
```

- [ ] **Step 3: 实现 prelogin, login, sync, exchange_refresh_token (仅限异步)**

从 rbw 的 `api.rs` 移植异步版本。关键更改：
- 使用 `self.reqwest_client()`（我们新的支持代理的版本）而不是 rbw 的。
- 从 `login()` 中移除 `sso_id` 参数。仅限密码验证。
- 移除所有 `blocking` 变体（add, edit, remove, folders, create_folder）。
- 移除 `register`, `send_email_login`, SSO 回调服务器代码。
- 完全移除 `axum` 依赖（仅用于 SSO）。
- `login()` 方法简化为仅支持 `ConnectTokenAuth::Password`。

`login()` 签名变为：
```rust
pub async fn login(
    &self,
    email: &str,
    device_id: &str,
    password_hash: &crate::locked::PasswordHash,
    two_factor_token: Option<&str>,
    two_factor_provider: Option<TwoFactorProviderType>,
) -> crate::error::Result<(String, String, String)>
```

`sync()` 与 rbw 保持一致，但使用我们的 reqwest_client()。

`exchange_refresh_token()` 变为仅限异步 (移除阻塞版本)：
```rust
pub async fn exchange_refresh_token(&self, refresh_token: &str) -> crate::error::Result<String>
```

- [ ] **Step 4: 添加 `device_id()` 辅助函数**

生成内存中的 device_id，而不是 rbw 基于文件的 device_id，并可选地持久化：

```rust
/// 生成或加载一个稳定的设备 ID。
/// 目前，每次运行都会生成一个新的 UUID。未来：持久化到配置文件。
pub fn generate_device_id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}
```

- [ ] **Step 5: 添加高级 action 函数**

在 `api.rs` 底部（或在单独的 `actions` 部分）创建便捷函数，这些函数结合了低级 API 调用，镜像 `rbw/src/actions.rs`：

```rust
/// 执行完整的登录流程：prelogin → 派生密钥 → 登录 → 返回会话数据。
pub async fn full_login(
    client: &Client,
    email: &str,
    password: &crate::locked::Password,
) -> crate::error::Result<LoginSession> {
    let device_id = generate_device_id();
    let (kdf, iterations, memory, parallelism) = client.prelogin(email).await?;

    let identity = crate::identity::Identity::new(
        email, password, kdf, iterations, memory, parallelism,
    )?;

    let (access_token, refresh_token, protected_key) = client
        .login(email, &device_id, &identity.master_password_hash, None, None)
        .await?;

    Ok(LoginSession {
        access_token,
        refresh_token,
        kdf,
        iterations,
        memory,
        parallelism,
        protected_key,
        email: email.to_string(),
        identity,
    })
}

pub struct LoginSession {
    pub access_token: String,
    pub refresh_token: String,
    pub kdf: KdfType,
    pub iterations: u32,
    pub memory: Option<u32>,
    pub parallelism: Option<u32>,
    pub protected_key: String,
    pub email: String,
    pub identity: crate::identity::Identity,
}

/// 同步保险库并返回准备好解密的数据。
pub async fn sync_vault(
    client: &Client,
    access_token: &str,
) -> crate::error::Result<SyncData> {
    let (protected_key, protected_private_key, org_keys, entries) =
        client.sync(access_token).await?;
    Ok(SyncData {
        protected_key,
        protected_private_key,
        org_keys,
        entries,
    })
}

pub struct SyncData {
    pub protected_key: String,
    pub protected_private_key: String,
    pub org_keys: std::collections::HashMap<String, String>,
    pub entries: Vec<crate::db::Entry>,
}

/// 解锁保险库：从主密钥派生用户密钥，解密组织密钥。
pub fn unlock_vault(
    email: &str,
    password: &crate::locked::Password,
    kdf: KdfType,
    iterations: u32,
    memory: Option<u32>,
    parallelism: Option<u32>,
    protected_key: &str,
    protected_private_key: &str,
    protected_org_keys: &std::collections::HashMap<String, String>,
) -> crate::error::Result<(
    crate::locked::Keys,
    std::collections::HashMap<String, crate::locked::Keys>,
)> {
    // 这与 rbw::actions::unlock 相同
    let identity = crate::identity::Identity::new(
        email, password, kdf, iterations, memory, parallelism,
    )?;

    let protected_key = crate::cipherstring::CipherString::new(protected_key)?;
    let key = match protected_key.decrypt_locked_symmetric(&identity.keys) {
        Ok(master_keys) => crate::locked::Keys::new(master_keys),
        Err(crate::error::Error::InvalidMac) => {
            return Err(crate::error::Error::IncorrectPassword {
                message: "密码错误。请重试。".to_string(),
            })
        }
        Err(e) => return Err(e),
    };

    let protected_private_key =
        crate::cipherstring::CipherString::new(protected_private_key)?;
    let private_key = crate::locked::PrivateKey::new(
        protected_private_key.decrypt_locked_symmetric(&key)?,
    );

    let mut org_keys_map = std::collections::HashMap::new();
    for (org_id, protected_org_key) in protected_org_keys {
        let protected_org_key =
            crate::cipherstring::CipherString::new(protected_org_key)?;
        let org_key = crate::locked::Keys::new(
            protected_org_key.decrypt_locked_asymmetric(&private_key)?,
        );
        org_keys_map.insert(org_id.clone(), org_key);
    }

    Ok((key, org_keys_map))
}
```

- [ ] **Step 6: 为 Client 构造和 URL 辅助函数添加单元测试**

在 `api.rs` 底部添加内联测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_bitwarden_cloud_urls() {
        let client = Client::bitwarden_cloud(None);
        assert_eq!(client.api_url("/sync"), "https://api.bitwarden.com/sync");
        assert_eq!(
            client.identity_url("/connect/token"),
            "https://identity.bitwarden.com/connect/token"
        );
    }

    #[test]
    fn test_client_with_proxy_builds() {
        let client = Client::new(
            "https://api.bitwarden.com",
            "https://identity.bitwarden.com",
            Some("http://127.0.0.1:7890"),
        );
        // 不应 panic — 代理 URL 是有效的
        let _ = client.reqwest_client().unwrap();
    }

    #[test]
    fn test_classify_login_error_incorrect_password() {
        let error_res = ConnectErrorRes {
            error: "invalid_grant".to_string(),
            error_description: Some("invalid_username_or_password".to_string()),
            error_model: Some(ConnectErrorResErrorModel {
                message: "Username or password is incorrect.".to_string(),
            }),
            two_factor_providers: None,
            sso_email_2fa_session_token: None,
        };
        let err = classify_login_error(&error_res, 400);
        assert!(matches!(err, crate::error::Error::IncorrectPassword { .. }));
    }

    #[test]
    fn test_device_id_is_uuid_format() {
        let id = generate_device_id();
        assert_eq!(id.len(), 36); // UUID 带连字符格式
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }
}
```

运行：`cargo test -p bw-core -- api::tests`
预期：所有 4 个测试通过。

- [ ] **Step 7: 验证 bw-core 编译**

```bash
cargo check -p bw-core
```

- [ ] **Step 8: 提交**

```bash
git add -A && git commit -m "feat(bw-core): add Bitwarden API client with proxy support and vault unlock logic"
```

---

## Chunk 3: egui 密码对话框

### Task 6: 实现 bw-ui 密码提示

**文件：**
- 修改：`crates/bw-ui/src/lib.rs`

- [ ] **Step 1: 实现 egui 密码对话框**

```rust
use eframe::egui;
use std::sync::{Arc, Mutex};

/// 显示模态密码提示窗口。阻塞直到用户提交或取消。
/// 提交时返回 `Some(password)`，取消/关闭时返回 `None`。
pub fn prompt_master_password() -> Option<String> {
    let result: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 180.0])
            .with_resizable(false)
            .with_always_on_top(),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Bitwarden SSH Agent - Unlock",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(PasswordApp {
                password: String::new(),
                result: result_clone,
                error_msg: None,
            }))
        }),
    );

    let lock = result.lock().unwrap();
    lock.clone()
}

struct PasswordApp {
    password: String,
    result: Arc<Mutex<Option<String>>>,
    error_msg: Option<String>,
}

impl eframe::App for PasswordApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.heading("Bitwarden 主密码");
                ui.add_space(10.0);

                ui.label("输入主密码以解锁 SSH 密钥：");
                ui.add_space(5.0);

                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                        .desired_width(300.0)
                        .hint_text("主密码"),
                );

                // 自动聚焦密码字段
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.submit(ctx);
                    return;
                }
                response.request_focus();

                if let Some(msg) = &self.error_msg {
                    ui.colored_label(egui::Color32::RED, msg);
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("解锁").clicked() {
                        self.submit(ctx);
                    }
                    if ui.button("取消").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
        });
    }
}

impl PasswordApp {
    fn submit(&mut self, ctx: &egui::Context) {
        if self.password.is_empty() {
            self.error_msg = Some("密码不能为空".to_string());
            return;
        }
        *self.result.lock().unwrap() = Some(self.password.clone());
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
```

- [ ] **Step 2: 验证 bw-ui 编译**

```bash
cargo check -p bw-ui
```

- [ ] **Step 3: 创建并运行测试示例**

创建 `crates/bw-ui/examples/test_dialog.rs`:

```rust
fn main() {
    match bw_ui::prompt_master_password() {
        Some(pw) => println!("Got password ({} chars)", pw.len()),
        None => println!("Cancelled"),
    }
}
```

运行：

```bash
cargo run -p bw-ui --example test_dialog
```

预期：出现一个标题为 "Bitwarden SSH Agent - Unlock" 的 400x180 窗口。输入任何文本，按 Enter 或点击 "解锁" → 控制台打印 "Got password (N chars)"。点击 "取消" → 控制台打印 "Cancelled"。提交空内容 → 红色错误文本 "密码不能为空"。

- [ ] **Step 4: 提交**

```bash
git add -A && git commit -m "feat(bw-ui): egui master password prompt dialog"
```

---

## Chunk 4: SSH Agent 二进制文件

### Task 7: 实现 state.rs —— 共享身份验证缓存

**文件：**
- 创建：`crates/bw-agent/src/state.rs`
- 修改：`crates/bw-agent/src/main.rs` (添加模块声明)

- [ ] **Step 0: 预先将所有 agent 模块连接到 main.rs**

用模块存根替换 `crates/bw-agent/src/main.rs`，以便 Task 7-9 可以独立编译和测试：

```rust
mod auth;
mod ssh_agent;
mod state;

fn main() {
    println!("bw-agent placeholder");
}
```

同时为 `auth.rs` 和 `ssh_agent.rs` 创建空存根：

`crates/bw-agent/src/auth.rs`:
```rust
// 占位符 — 在 Task 8 中实现
```

`crates/bw-agent/src/ssh_agent.rs`:
```rust
// 占位符 — 在 Task 9 中实现
```

- [ ] **Step 1: 实现缓存状态**

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 带有 TTL 的缓存身份验证状态。
pub struct State {
    /// 解密的保险库密钥 (用户密钥)。
    pub keys: Option<bw_core::locked::Keys>,
    /// 解密的组织密钥。
    pub org_keys: Option<HashMap<String, bw_core::locked::Keys>>,
    /// 密钥最后一次缓存的时间。
    pub cached_at: Option<Instant>,
    /// 缓存密钥保持有效的时间。
    pub cache_ttl: Duration,
    /// 缓存的保险库条目 (加密的 cipherstring)。
    pub entries: Vec<bw_core::db::Entry>,
    /// 用于令牌刷新的登录会话数据。
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    /// KDF 参数 (重新解锁所需)。
    pub email: Option<String>,
    pub kdf: Option<bw_core::api::KdfType>,
    pub iterations: Option<u32>,
    pub memory: Option<u32>,
    pub parallelism: Option<u32>,
    pub protected_key: Option<String>,
    pub protected_private_key: Option<String>,
    pub protected_org_keys: HashMap<String, String>,
}

impl State {
    pub fn new(cache_ttl: Duration) -> Self {
        Self {
            keys: None,
            org_keys: None,
            cached_at: None,
            cache_ttl,
            entries: Vec::new(),
            access_token: None,
            refresh_token: None,
            email: None,
            kdf: None,
            iterations: None,
            memory: None,
            parallelism: None,
            protected_key: None,
            protected_private_key: None,
            protected_org_keys: HashMap::new(),
        }
    }

    /// 检查密钥是否已缓存且仍然有效。
    pub fn is_unlocked(&self) -> bool {
        if let (Some(_keys), Some(cached_at)) = (&self.keys, &self.cached_at) {
            cached_at.elapsed() < self.cache_ttl
        } else {
            false
        }
    }

    /// 获取给定 org_id 的解密密钥 (如果为 None 则获取用户密钥)。
    pub fn key(&self, org_id: Option<&str>) -> Option<&bw_core::locked::Keys> {
        org_id.map_or(self.keys.as_ref(), |id| {
            self.org_keys.as_ref().and_then(|h| h.get(id))
        })
    }

    /// 清除所有缓存密钥 (锁定)。
    pub fn clear(&mut self) {
        self.keys = None;
        self.org_keys = None;
        self.cached_at = None;
    }

    /// 存储解锁的密钥并刷新 TTL。
    pub fn set_unlocked(
        &mut self,
        keys: bw_core::locked::Keys,
        org_keys: HashMap<String, bw_core::locked::Keys>,
    ) {
        self.keys = Some(keys);
        self.org_keys = Some(org_keys);
        self.cached_at = Some(Instant::now());
    }

    /// 刷新 TTL (在每次成功的 SSH 操作时调用)。
    pub fn touch(&mut self) {
        if self.keys.is_some() {
            self.cached_at = Some(Instant::now());
        }
    }
}
```

- [ ] **Step 2: 为 State TTL 逻辑添加单元测试**

在 `state.rs` 底部添加测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_keys() -> bw_core::locked::Keys {
        let mut v = bw_core::locked::Vec::new();
        v.extend(std::iter::repeat(0u8).take(64));
        bw_core::locked::Keys::new(v)
    }

    #[test]
    fn test_new_state_is_locked() {
        let state = State::new(Duration::from_secs(900));
        assert!(!state.is_unlocked());
        assert!(state.key(None).is_none());
    }

    #[test]
    fn test_unlock_and_check() {
        let mut state = State::new(Duration::from_secs(900));
        state.set_unlocked(dummy_keys(), HashMap::new());
        assert!(state.is_unlocked());
        assert!(state.key(None).is_some());
    }

    #[test]
    fn test_cache_expires() {
        let mut state = State::new(Duration::from_millis(1));
        state.set_unlocked(dummy_keys(), HashMap::new());
        std::thread::sleep(Duration::from_millis(10));
        assert!(!state.is_unlocked());
    }

    #[test]
    fn test_clear_locks() {
        let mut state = State::new(Duration::from_secs(900));
        state.set_unlocked(dummy_keys(), HashMap::new());
        state.clear();
        assert!(!state.is_unlocked());
        assert!(state.key(None).is_none());
    }

    #[test]
    fn test_touch_refreshes_ttl() {
        let mut state = State::new(Duration::from_secs(900));
        state.set_unlocked(dummy_keys(), HashMap::new());
        std::thread::sleep(Duration::from_millis(10));
        state.touch();
        assert!(state.is_unlocked());
    }
}
```

运行：`cargo test -p bw-agent -- state::tests`
预期：所有 5 个测试通过。

- [ ] **Step 3: 验证编译**

```bash
cargo check -p bw-agent
```

- [ ] **Step 4: 提交**

```bash
git add -A && git commit -m "feat(bw-agent): add auth cache state with 15-min TTL"
```

---

### Task 8: 实现 auth.rs —— 身份验证流程

**文件：**
- 创建：`crates/bw-agent/src/auth.rs`

- [ ] **Step 1: 实现身份验证编排器**

该模块协调：检查缓存 → 提示密码 (egui) → 登录 → 同步 → 解锁 → 缓存。

```rust
use crate::state::State;
use std::sync::Arc;
use tokio::sync::Mutex;

/// 确保 agent 已解锁。如果缓存过期，提示输入密码并重新验证。
pub async fn ensure_unlocked(
    state: Arc<Mutex<State>>,
    client: &bw_core::api::Client,
) -> anyhow::Result<()> {
    let needs_auth = {
        let s = state.lock().await;
        !s.is_unlocked()
    };

    if needs_auth {
        // 通过 egui 提示输入主密码 (在 GUI 线程上阻塞)
        let password_str = tokio::task::spawn_blocking(|| {
            bw_ui::prompt_master_password()
        })
        .await?
        .ok_or_else(|| anyhow::anyhow!("用户取消了密码提示"))?;

        let mut password_vec = bw_core::locked::Vec::new();
        password_vec.extend(password_str.as_bytes().iter().copied());
        let password = bw_core::locked::Password::new(password_vec);

        let mut s = state.lock().await;

        // 确定我们需要重新登录还是仅解锁
        let needs_login = s.access_token.is_none();

        if needs_login {
            let email = s.email.as_ref()
                .ok_or_else(|| anyhow::anyhow!("未配置电子邮件"))?
                .clone();

            // 完整登录流程
            let session = bw_core::api::full_login(client, &email, &password).await?;

            s.access_token = Some(session.access_token.clone());
            s.refresh_token = Some(session.refresh_token.clone());
            s.kdf = Some(session.kdf);
            s.iterations = Some(session.iterations);
            s.memory = session.memory;
            s.parallelism = session.parallelism;
            s.protected_key = Some(session.protected_key.clone());

            // 同步保险库
            let sync_data = bw_core::api::sync_vault(client, &session.access_token).await?;
            s.protected_private_key = Some(sync_data.protected_private_key.clone());
            s.protected_org_keys = sync_data.org_keys.clone();
            s.entries = sync_data.entries;

            // 解锁
            let (keys, org_keys) = bw_core::api::unlock_vault(
                &email,
                &password,
                session.kdf,
                session.iterations,
                session.memory,
                session.parallelism,
                &session.protected_key,
                &sync_data.protected_private_key,
                &sync_data.org_keys,
            )?;
            s.set_unlocked(keys, org_keys);
        } else {
            // 使用现有会话数据重新解锁
            let email = s.email.clone().unwrap();
            let kdf = s.kdf.unwrap();
            let iterations = s.iterations.unwrap();
            let memory = s.memory;
            let parallelism = s.parallelism;
            let protected_key = s.protected_key.clone().unwrap();
            let protected_private_key = s.protected_private_key.clone().unwrap();
            let protected_org_keys = s.protected_org_keys.clone();

            let (keys, org_keys) = bw_core::api::unlock_vault(
                &email,
                &password,
                kdf,
                iterations,
                memory,
                parallelism,
                &protected_key,
                &protected_private_key,
                &protected_org_keys,
            )?;
            s.set_unlocked(keys, org_keys);

            // 如果 access_token 有效，也在后台重新同步
            let access_token = s.access_token.clone().unwrap();
            drop(s); // 在网络调用前释放锁

            if let Ok(sync_data) = bw_core::api::sync_vault(client, &access_token).await {
                let mut s = state.lock().await;
                s.entries = sync_data.entries;
                s.protected_org_keys = sync_data.org_keys;
            }
        }
    } else {
        // 每次访问时刷新 TTL
        state.lock().await.touch();
    }

    Ok(())
}

/// 使用缓存的密钥解密 cipher string。
pub fn decrypt_cipher(
    state: &State,
    cipherstring: &str,
    entry_key: Option<&str>,
    org_id: Option<&str>,
) -> anyhow::Result<String> {
    let keys = state.key(org_id)
        .ok_or_else(|| anyhow::anyhow!("没有可用的解密密钥"))?;

    let entry_key = if let Some(ek) = entry_key {
        let ek_cs = bw_core::cipherstring::CipherString::new(ek)?;
        Some(bw_core::locked::Keys::new(ek_cs.decrypt_locked_symmetric(keys)?))
    } else {
        None
    };

    let cs = bw_core::cipherstring::CipherString::new(cipherstring)?;
    let plaintext = String::from_utf8(cs.decrypt_symmetric(keys, entry_key.as_ref())?)?;
    Ok(plaintext)
}
```

- [ ] **Step 2: 为带有已知测试向量的 decrypt_cipher 添加单元测试**

在 `auth.rs` 底部添加测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decrypt_cipher_returns_error_when_locked() {
        let state = State::new(std::time::Duration::from_secs(900));
        let result = decrypt_cipher(&state, "2.fake|data|mac", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("没有可用的解密密钥"));
    }
}
```

运行：`cargo test -p bw-agent -- auth::tests`
预期：通过 —— 确认 `decrypt_cipher` 在状态锁定时正确报错。

- [ ] **Step 3: 验证编译**

```bash
cargo check -p bw-agent
```

- [ ] **Step 4: 提交**

```bash
git add -A && git commit -m "feat(bw-agent): auth flow with egui password prompt and vault unlock"
```

---

### Task 9: 实现 ssh_agent.rs —— SSH Agent 协议

**文件：**
- 创建：`crates/bw-agent/src/ssh_agent.rs`

- [ ] **Step 1: 从 rbw 移植 ssh_agent.rs**

适配 `.reference/rbw/src/bin/rbw-agent/ssh_agent.rs`。关键更改：
- 将 `crate::state::State` 引用替换为我们的 `crate::state::State`
- 将 `crate::actions::get_ssh_public_keys` / `find_ssh_private_key` 替换为使用 `crate::auth` 的内联逻辑
- 向结构体添加 `bw_core::api::Client` 用于身份验证流程
- `run()` 方法使用 `#[cfg]` 进行平台相关的监听器

```rust
use crate::auth;
use crate::state::State;
use std::sync::Arc;
use tokio::sync::Mutex;

const SSH_AGENT_RSA_SHA2_256: u32 = 2;
const SSH_AGENT_RSA_SHA2_512: u32 = 4;

#[derive(Clone)]
pub struct SshAgentHandler {
    state: Arc<Mutex<State>>,
    client: bw_core::api::Client,
}

impl SshAgentHandler {
    pub fn new(state: Arc<Mutex<State>>, client: bw_core::api::Client) -> Self {
        Self { state, client }
    }
}

#[ssh_agent_lib::async_trait]
impl ssh_agent_lib::agent::Session for SshAgentHandler {
    async fn request_identities(
        &mut self,
    ) -> Result<Vec<ssh_agent_lib::proto::Identity>, ssh_agent_lib::error::AgentError> {
        // 确保已解锁 (可能触发 egui 提示)
        auth::ensure_unlocked(self.state.clone(), &self.client)
            .await
            .map_err(|e| ssh_agent_lib::error::AgentError::Other(e.into()))?;

        let state = self.state.lock().await;
        let mut identities = Vec::new();

        for entry in &state.entries {
            if let bw_core::db::EntryData::SshKey {
                public_key: Some(encrypted_pubkey),
                ..
            } = &entry.data
            {
                let plaintext = auth::decrypt_cipher(
                    &state,
                    encrypted_pubkey,
                    entry.key.as_deref(),
                    entry.org_id.as_deref(),
                )
                .map_err(|e| ssh_agent_lib::error::AgentError::Other(e.into()))?;

                let pubkey = plaintext
                    .parse::<ssh_agent_lib::ssh_key::PublicKey>()
                    .map_err(ssh_agent_lib::error::AgentError::other)?;

                identities.push(ssh_agent_lib::proto::Identity {
                    pubkey: pubkey.key_data().clone(),
                    comment: entry.name.clone(),
                });
            }
        }

        Ok(identities)
    }

    async fn sign(
        &mut self,
        request: ssh_agent_lib::proto::SignRequest,
    ) -> Result<ssh_agent_lib::ssh_key::Signature, ssh_agent_lib::error::AgentError> {
        use signature::{RandomizedSigner as _, Signer as _, SignatureEncoding as _};

        // 确保已解锁
        auth::ensure_unlocked(self.state.clone(), &self.client)
            .await
            .map_err(|e| ssh_agent_lib::error::AgentError::Other(e.into()))?;

        let request_pubkey = ssh_agent_lib::ssh_key::PublicKey::new(request.pubkey.clone(), "");
        let request_bytes = request_pubkey.to_bytes();

        // 查找匹配的私钥
        let state = self.state.lock().await;
        for entry in &state.entries {
            if let bw_core::db::EntryData::SshKey {
                private_key: Some(encrypted_privkey),
                public_key: Some(encrypted_pubkey),
                ..
            } = &entry.data
            {
                let pubkey_plain = auth::decrypt_cipher(
                    &state,
                    encrypted_pubkey,
                    entry.key.as_deref(),
                    entry.org_id.as_deref(),
                )
                .map_err(|e| ssh_agent_lib::error::AgentError::Other(e.into()))?;

                let pubkey = ssh_agent_lib::ssh_key::PublicKey::from_openssh(&pubkey_plain)
                    .map_err(ssh_agent_lib::error::AgentError::other)?;

                if pubkey.to_bytes() != request_bytes {
                    continue;
                }

                // 找到匹配的密钥 — 解密私钥
                let privkey_plain = auth::decrypt_cipher(
                    &state,
                    encrypted_privkey,
                    entry.key.as_deref(),
                    entry.org_id.as_deref(),
                )
                .map_err(|e| ssh_agent_lib::error::AgentError::Other(e.into()))?;

                let private_key =
                    ssh_agent_lib::ssh_key::PrivateKey::from_openssh(&privkey_plain)
                        .map_err(ssh_agent_lib::error::AgentError::other)?;

                // 根据密钥类型进行签名 — 与 rbw 的逻辑相同
                return match private_key.key_data() {
                    ssh_agent_lib::ssh_key::private::KeypairData::Ed25519(key) => {
                        key.try_sign(&request.data)
                            .map_err(ssh_agent_lib::error::AgentError::other)
                    }
                    ssh_agent_lib::ssh_key::private::KeypairData::Rsa(key) => {
                        let p = rsa::BigUint::from_bytes_be(key.private.p.as_bytes());
                        let q = rsa::BigUint::from_bytes_be(key.private.q.as_bytes());
                        let e = rsa::BigUint::from_bytes_be(key.public.e.as_bytes());
                        let rsa_key = rsa::RsaPrivateKey::from_p_q(p, q, e)
                            .map_err(ssh_agent_lib::error::AgentError::other)?;

                        let mut rng = rand::rngs::OsRng;

                        let (algorithm, sig_bytes) =
                            if request.flags & SSH_AGENT_RSA_SHA2_512 != 0 {
                                let signing_key =
                                    rsa::pkcs1v15::SigningKey::<sha2::Sha512>::new(rsa_key);
                                let sig = signing_key
                                    .try_sign_with_rng(&mut rng, &request.data)
                                    .map_err(ssh_agent_lib::error::AgentError::other)?;
                                ("rsa-sha2-512", sig.to_bytes())
                            } else if request.flags & SSH_AGENT_RSA_SHA2_256 != 0 {
                                let signing_key =
                                    rsa::pkcs1v15::SigningKey::<sha2::Sha256>::new(rsa_key);
                                let sig = signing_key
                                    .try_sign_with_rng(&mut rng, &request.data)
                                    .map_err(ssh_agent_lib::error::AgentError::other)?;
                                ("rsa-sha2-256", sig.to_bytes())
                            } else {
                                let signing_key =
                                    rsa::pkcs1v15::SigningKey::<sha1::Sha1>::new_unprefixed(
                                        rsa_key,
                                    );
                                let sig = signing_key
                                    .try_sign_with_rng(&mut rng, &request.data)
                                    .map_err(ssh_agent_lib::error::AgentError::other)?;
                                ("ssh-rsa", sig.to_bytes())
                            };

                        Ok(ssh_agent_lib::ssh_key::Signature::new(
                            ssh_agent_lib::ssh_key::Algorithm::new(algorithm)
                                .map_err(ssh_agent_lib::error::AgentError::other)?,
                            sig_bytes,
                        )
                        .map_err(ssh_agent_lib::error::AgentError::other)?)
                    }
                    other => Err(ssh_agent_lib::error::AgentError::Other(
                        format!("不支持的密钥类型：{other:?}").into(),
                    )),
                };
            }
        }

        Err(ssh_agent_lib::error::AgentError::Other(
            "未找到匹配的私钥".into(),
        ))
    }
}
```

- [ ] **Step 2: 为 SshAgentHandler 构造添加单元测试**

在 `ssh_agent.rs` 底部添加测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_request_identities_returns_empty_when_no_entries() {
        let state = Arc::new(Mutex::new(
            crate::state::State::new(std::time::Duration::from_secs(900)),
        ));
        // 使用虚拟密钥预解锁，这样它就不会触发 egui 提示
        {
            let mut s = state.lock().await;
            let mut v = bw_core::locked::Vec::new();
            v.extend(std::iter::repeat(0u8).take(64));
            s.set_unlocked(bw_core::locked::Keys::new(v), std::collections::HashMap::new());
            s.email = Some("test@example.com".to_string());
        }
        let client = bw_core::api::Client::bitwarden_cloud(None);
        let mut handler = SshAgentHandler::new(state, client);

        use ssh_agent_lib::agent::Session;
        let identities = handler.request_identities().await.unwrap();
        assert!(identities.is_empty()); // 无条目 = 无身份
    }
}
```

运行：`cargo test -p bw-agent -- ssh_agent::tests`
预期：通过 —— 确认具有零个保险库条目的已解锁 agent 返回空的身份列表。

- [ ] **Step 3: 验证完整项目编译**

```bash
cargo check -p bw-agent
```

- [ ] **Step 4: 提交**

```bash
git add -A && git commit -m "feat(bw-agent): SSH agent protocol handler with Ed25519 and RSA signing"
```

---

### Task 10: 实现 main.rs —— 入口点

**文件：**
- 修改：`crates/bw-agent/src/main.rs`

- [ ] **Step 1: 实现带有 CLI 参数和 agent 启动的 main**

```rust
mod auth;
mod ssh_agent;
mod state;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Windows 上 OpenSSH 的默认命名管道路径。
#[cfg(windows)]
const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\openssh-ssh-agent";

/// 默认 Unix socket 路径。
#[cfg(unix)]
fn default_socket_path() -> String {
    std::env::var("SSH_AUTH_SOCK").unwrap_or_else(|_| {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/tmp".to_string());
        format!("{runtime_dir}/bw-agent.sock")
    })
}

const DEFAULT_CACHE_TTL_SECS: u64 = 900; // 15 分钟
const DEFAULT_PROXY: &str = "http://127.0.0.1:7890";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // 从环境变量读取配置
    let email = std::env::var("BW_EMAIL")
        .expect("必须设置 BW_EMAIL 环境变量");
    let base_url = std::env::var("BW_BASE_URL")
        .unwrap_or_else(|_| "https://api.bitwarden.com".to_string());
    let identity_url = std::env::var("BW_IDENTITY_URL")
        .unwrap_or_else(|_| "https://identity.bitwarden.com".to_string());
    let proxy = std::env::var("BW_PROXY")
        .unwrap_or_else(|_| DEFAULT_PROXY.to_string());
    let proxy = if proxy.is_empty() { None } else { Some(proxy.as_str()) };
    let cache_ttl = std::env::var("BW_CACHE_TTL")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_CACHE_TTL_SECS);

    let client = bw_core::api::Client::new(&base_url, &identity_url, proxy);

    let mut initial_state = state::State::new(Duration::from_secs(cache_ttl));
    initial_state.email = Some(email);

    let state = Arc::new(Mutex::new(initial_state));
    let handler = ssh_agent::SshAgentHandler::new(state, client);

    log::info!("正在启动 bw-agent SSH agent...");

    #[cfg(windows)]
    {
        let pipe_name = std::env::var("BW_PIPE_NAME")
            .unwrap_or_else(|_| DEFAULT_PIPE_NAME.to_string());
        log::info!("正在监听命名管道：{pipe_name}");
        let listener = ssh_agent_lib::agent::NamedPipeListener::bind(pipe_name)?;
        ssh_agent_lib::agent::listen(listener, handler).await?;
    }

    #[cfg(unix)]
    {
        let socket_path = std::env::var("BW_SOCKET_PATH")
            .unwrap_or_else(|_| default_socket_path());
        let _ = std::fs::remove_file(&socket_path);
        log::info!("正在监听 Unix socket：{socket_path}");
        let listener = tokio::net::UnixListener::bind(&socket_path)?;
        ssh_agent_lib::agent::listen(listener, handler).await?;
    }

    Ok(())
}
```

- [ ] **Step 2: 验证完整项目编译**

```bash
cargo check --workspace
```

- [ ] **Step 3: 验证二进制文件构建**

```bash
cargo build -p bw-agent
```

预期：成功。二进制文件位于 `target/debug/bw-agent.exe` (Windows)。

- [ ] **Step 4: 提交**

```bash
git add -A && git commit -m "feat(bw-agent): main entry point with env-based config and platform listener"
```

---

## Chunk 5: 集成与完善

### Task 11: 修复编译问题和依赖对齐

此任务保留用于修复在先前任务中出现的任何编译错误。常见问题：

- [ ] **Step 1: 修复版本不匹配**

确保 `rsa`, `sha1`, `sha2`, `signature` crate 版本在 `bw-core` 和 `bw-agent` 之间兼容。`ssh-agent-lib` 0.5.x 重新导出 `ssh-key` 0.6.x，后者使用特定版本的加密 crate。使用以下命令检查：

```bash
cargo tree -p bw-agent -d
```

通过对齐 `[workspace.dependencies]` 解决任何重复版本。

- [ ] **Step 2: 修复 `rand` 版本兼容性**

我们在所有地方都使用 `rand 0.10.1`。验证 `ssh-agent-lib 0.5.2` 和 `rsa 0.9.10` 是否与 `rand 0.10` 兼容。如果 `ssh-agent-lib` 或 `rsa` 重新导出或依赖于较旧的 `rand`，你可能需要仅为 RSA 签名代码添加一个兼容别名，如 `rand_08 = { package = "rand", version = "0.8" }`。使用 `cargo tree -p bw-agent -i rand` 检查。

同时验证：rand 0.10 中的 `rand::rngs::OsRng` —— 它可能已移动。在 rand 0.10 中，`OsRng` 位于 `rand::rngs::OsRng` 或仅 `rand::rngs::OsRng`。查看文档。

- [ ] **Step 3: 完整构建**

```bash
cargo build --workspace
```

- [ ] **Step 4: 运行完整测试套件**

```bash
cargo test --workspace
```

预期：所有测试均通过 —— 这包括：
- `bw-core::cipherstring::test_pkcs7_unpad`
- `bw-core::db::tests::test_ssh_key_entry_roundtrip`
- `bw-core::api::tests::*` (4 个测试)
- `bw-agent::state::tests::*` (5 个测试)
- `bw-agent::auth::tests::*` (1 个测试)
- `bw-agent::ssh_agent::tests::*` (1 个测试)

如果任何测试失败，请在提交前修复问题。

- [ ] **Step 5: 提交**

```bash
git add -A && git commit -m "fix: resolve dependency version conflicts across workspace"
```

---

### Task 12: 端到端冒烟测试

- [ ] **Step 1: 设置环境并运行**

```powershell
$env:BW_EMAIL = "your-email@example.com"
$env:BW_PROXY = "http://127.0.0.1:7890"
$env:RUST_LOG = "debug"
cargo run -p bw-agent
```

预期：Agent 启动，打印 "正在监听命名管道：\\.\pipe\openssh-ssh-agent"

- [ ] **Step 2: 使用 ssh-add 进行测试**

在另一个终端中（无需设置环境变量 —— Windows 的 `ssh-add.exe` 会自动连接 `\\.\pipe\openssh-ssh-agent`）：

> **前提条件：** 先停掉 Windows 内置的 OpenSSH Agent 服务，否则 bw-agent 无法绑定默认管道：
> ```powershell
> Stop-Service ssh-agent
> Set-Service ssh-agent -StartupType Disabled
> ```

```powershell
ssh-add -l
```

预期：弹出 egui 密码对话框。输入正确密码后，列出 Bitwarden 保险库中的 SSH 密钥。

- [ ] **Step 3: 使用 ssh -T 测试签名**

无需额外配置 —— Windows 上的 `ssh.exe` 会自动使用 `\\.\pipe\openssh-ssh-agent`：

```powershell
ssh -T git@github.com
```

预期：如果 Bitwarden 保险库中有在 GitHub 注册的 SSH 密钥，你应该看到 `Hi <username>! You've authenticated...`。如果未注册，你将看到 `Permission denied (publickey).` —— 但重要的是 **没有出现 egui 提示**（在 15 分钟缓存窗口内），确认缓存有效。检查 agent 调试日志以验证签名请求已处理。

- [ ] **Step 4: 测试缓存过期**

等待 15 分钟（或临时设置 `BW_CACHE_TTL=10` 为 10 秒）。然后重试 `ssh-add -l`。预期：再次弹出 egui 对话框。

- [ ] **Step 5: 最终提交**

```bash
git add -A && git commit -m "chore: integration smoke test passed"
```

---

## 环境变量参考

| 变量 | 默认值 | 描述 |
|---|---|---|
| `BW_EMAIL` | (必填) | Bitwarden 账户电子邮件 |
| `BW_BASE_URL` | `https://api.bitwarden.com` | Bitwarden API 基础 URL |
| `BW_IDENTITY_URL` | `https://identity.bitwarden.com` | Bitwarden 身份验证 URL |
| `BW_PROXY` | `http://127.0.0.1:7890` | 所有 API 请求的 HTTP 代理 |
| `BW_CACHE_TTL` | `900` (15 分钟) | 身份验证缓存 TTL (秒) |
| `BW_PIPE_NAME` (Windows) | `\\.\pipe\openssh-ssh-agent` | 命名管道路径 |
| `BW_SOCKET_PATH` (Unix) | `$SSH_AUTH_SOCK` 或 `$XDG_RUNTIME_DIR/bw-agent.sock` | Unix socket 路径 |
| `RUST_LOG` | (未设置) | 日志级别 (`debug`, `info`, `warn`) |
