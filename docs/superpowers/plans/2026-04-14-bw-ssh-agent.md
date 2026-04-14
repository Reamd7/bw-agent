# Bitwarden SSH Agent Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Build a cross-platform SSH agent that serves SSH keys from a Bitwarden vault, using egui for master password prompts and a 15-minute auth cache.

**Architecture:** Fork core crypto/API logic from rbw (`.reference/rbw/`), replace Unix-only components (daemonize, Unix socket, pinentry) with cross-platform alternatives (foreground process, ssh-agent-lib NamedPipe, egui dialog). The project is a Cargo workspace with three crates: `bw-core` (crypto + API), `bw-ui` (egui dialog), `bw-agent` (binary).

**Tech Stack:** Rust 2024 edition (MSRV 1.85.0), tokio, reqwest (with proxy), ssh-agent-lib 0.5.x, eframe/egui 0.34, AES-256-CBC + HMAC-SHA256, PBKDF2/Argon2id, RSA-OAEP

**Reference:** All "Copy from rbw" instructions refer to `.reference/rbw/src/` in this workspace. rbw uses older crate versions (Rust 2021, RustCrypto 0.8.x/0.10.x era). We use the latest versions — **copied code WILL need API adjustments** for the new RustCrypto / rand / reqwest APIs. See the migration notes below.

**Proxy:** All reqwest HTTP clients MUST be configured with proxy `http://127.0.0.1:7890`.

### ⚠️ Breaking Changes from rbw's Dependency Versions

The following crates have **breaking API changes** compared to what rbw uses. Code copied from rbw MUST be adapted:

| Crate | rbw version | Our version | Impact on copied code |
|---|---|---|---|
| `aes` | 0.8.4 | 0.9.0 | `cipher` trait imports may differ |
| `cbc` | 0.1.2 | 0.2.0 | `Encryptor`/`Decryptor` constructor APIs changed |
| `block-padding` | 0.3.3 | 0.4.2 | `UnpadError` type path may differ |
| `hmac` | 0.12.1 | 0.13.0 | `Mac` trait → check new API |
| `hkdf` | 0.12.4 | 0.13.0 | `Hkdf::from_prk` / `expand` API may differ |
| `sha1` | 0.10.6 | 0.11.0 | `Digest` trait may differ |
| `sha2` | 0.10.9 | 0.11.0 | `Digest` trait may differ |
| `rand` | 0.9.2 | 0.10.1 | `RngCore`, `rng()` API changed; rbw's `rand_8` (0.8.5) for RSA signing also needs updating |
| `reqwest` | 0.12 | 0.13.2 | Client builder API may differ |
| `eframe` | 0.31 | 0.34.1 | `run_native` API, `ViewportCommand` may differ |

**Strategy:** Copy rbw code first, then fix compilation errors against the new APIs. The RustCrypto crates (aes/cbc/hmac/hkdf/sha1/sha2/block-padding) update as a family — use them ALL at the new versions together. Check migration guides at https://github.com/RustCrypto.

---

## File Structure

```
bw-agent/
├── Cargo.toml                        # Workspace root
├── crates/
│   ├── bw-core/                      # Library: crypto, API, types
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                # Module declarations
│   │       ├── prelude.rs            # Re-export Error/Result
│   │       ├── error.rs              # Error types (trimmed from rbw)
│   │       ├── base64.rs             # Base64 helpers (copy from rbw)
│   │       ├── json.rs               # JSON deser with path (copy from rbw)
│   │       ├── locked.rs             # Memory-locked buffers (copy from rbw)
│   │       ├── identity.rs           # KDF + master key derivation (copy from rbw)
│   │       ├── cipherstring.rs       # AES/RSA encrypt/decrypt (copy from rbw)
│   │       ├── api.rs                # Bitwarden REST API (trimmed + proxy)
│   │       └── db.rs                 # Vault entry types (trimmed, no file I/O)
│   ├── bw-ui/                        # Library: egui password dialog
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs                # One-shot password prompt window
│   └── bw-agent/                     # Binary: SSH agent
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs               # Entry point, starts SSH agent listener
│           ├── ssh_agent.rs           # SSH agent Session impl (from rbw)
│           ├── auth.rs               # Auth flow: login → sync → decrypt → cache
│           └── state.rs              # Shared state: cached keys + TTL
```

**Boundary rules:**
- `bw-core` has ZERO platform-specific code. No `std::os::unix`, no `std::os::windows`.
- `bw-ui` depends only on `eframe`/`egui`. No dependency on `bw-core`.
- `bw-agent` depends on both `bw-core` and `bw-ui`, plus `ssh-agent-lib`.

---

## Chunk 1: Project Scaffolding + bw-core Crypto Chain

This chunk sets up the workspace and copies the platform-independent crypto modules from rbw.

### Task 1: Workspace Scaffolding

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/bw-core/Cargo.toml`
- Create: `crates/bw-core/src/lib.rs`
- Create: `crates/bw-core/src/prelude.rs`
- Create: `crates/bw-ui/Cargo.toml`
- Create: `crates/bw-ui/src/lib.rs`
- Create: `crates/bw-agent/Cargo.toml`
- Create: `crates/bw-agent/src/main.rs`
- Delete: `crates/example/` (no longer needed)

- [x] **Step 1: Update workspace root Cargo.toml**

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

- [x] **Step 2: Create bw-core Cargo.toml**

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

- [x] **Step 3: Create bw-core/src/lib.rs (stub — modules added incrementally)**

Start with only `prelude` and `error` as placeholders. Each subsequent Task will add its `pub mod` declaration when the file is created.

```rust
pub mod error;
pub mod prelude;
```

**Task 2** will add: `pub mod base64; pub mod json; pub mod locked;`
**Task 3** will add: `pub mod identity; pub mod cipherstring;`
**Task 4+5** will add: `pub mod api; pub mod db;` (db depends on api types, so both are added together after Task 5)

- [x] **Step 4: Create bw-core/src/prelude.rs**

Copy verbatim from `.reference/rbw/src/prelude.rs`:

```rust
pub use crate::error::{Error, Result};
```

- [x] **Step 5: Create stub bw-ui/Cargo.toml and lib.rs**

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
// Placeholder - implemented in Task 7
pub fn prompt_master_password() -> Option<String> {
    todo!("egui password prompt")
}
```

- [x] **Step 6: Create stub bw-agent/Cargo.toml and main.rs**

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

- [x] **Step 7: Delete crates/example directory**

```bash
rm -rf crates/example
```

- [x] **Step 8: Verify workspace compiles**

```bash
cargo check --workspace
```

Expected: Success (stubs only)

- [x] **Step 9: Commit**

```bash
git add -A && git commit -m "scaffold: workspace with bw-core, bw-ui, bw-agent crates"
```

---

### Task 2: Copy Core Crypto Modules (base64, json, locked, error)

These four files are small utilities that need minimal or no changes.

**Files:**
- Create: `crates/bw-core/src/base64.rs`
- Create: `crates/bw-core/src/json.rs`
- Create: `crates/bw-core/src/locked.rs`
- Create: `crates/bw-core/src/error.rs`

- [x] **Step 0: Wire new modules into lib.rs**

Add to `crates/bw-core/src/lib.rs`:

```rust
pub mod base64;
pub mod json;
pub mod locked;
```

- [x] **Step 1: Copy base64.rs verbatim**

Copy `.reference/rbw/src/base64.rs` → `crates/bw-core/src/base64.rs`. No changes needed.

- [x] **Step 2: Copy json.rs, remove blocking impl**

Copy `.reference/rbw/src/json.rs` → `crates/bw-core/src/json.rs`. **Remove** the `impl DeserializeJsonWithPath for reqwest::blocking::Response` block (lines 15-23 in rbw) — we don't enable reqwest's `blocking` feature. Keep only the `String` impl and the async `reqwest::Response` impl.

- [x] **Step 3: Copy locked.rs verbatim**

Copy `.reference/rbw/src/locked.rs` → `crates/bw-core/src/locked.rs`. No changes needed.

- [x] **Step 4: Create error.rs — trimmed version**

Copy `.reference/rbw/src/error.rs` → `crates/bw-core/src/error.rs`. Remove these variants that we don't need:
- `ConfigMissingEmail` (we handle config differently)
- `CreateDirectory` (no disk DB)
- `CreateSSOCallbackServer` (no SSO)
- `FailedToFindFreePort` (no SSO)
- `FailedToOpenWebBrowser` (no SSO)
- `FailedToProcessSSOCallback` (no SSO)
- `FailedToReadFromStdin` (no CLI)
- `FailedToFindEditor` (no CLI)
- `FailedToRunEditor` (no CLI)
- `FailedToParsePinentry` (no pinentry)
- `IncorrectApiKey` (no API key auth)
- `InvalidEditor` (no CLI)
Keep `InvalidTwoFactorProvider` — it's used in `TwoFactorProviderType` parsing in `api.rs`.
- `LoadConfig*` (simplified config)
- `LoadDb*` (no disk DB)
- `LoadDeviceId` (inline)
- `LoadClientCert` (no client certs)
- `ParseMatchType` (not needed)
- `PinentryCancelled` (no pinentry)
- `PinentryErrorMessage` (no pinentry)
- `PinentryReadOutput` (no pinentry)
- `PinentryWait` (no pinentry)
- `RegistrationRequired` (no registration flow)
- `RemoveDb` (no disk DB)
- `SaveConfig*` (simplified)
- `SaveDb*` (no disk DB)
- `Spawn` (no pinentry subprocess)
- `WriteStdin` (no pinentry)

Keep these variants:
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

The resulting `error.rs`:

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

- [x] **Step 5: Verify bw-core compiles**

```bash
cargo check -p bw-core
```

Expected: Success. If there are import errors, fix `crate::` paths.

- [x] **Step 6: Commit**

```bash
git add -A && git commit -m "feat(bw-core): add base64, json, locked, error modules from rbw"
```

---

### Task 3: Copy identity.rs and cipherstring.rs

These are the crypto core — KDF derivation and AES/RSA encrypt/decrypt.

**Files:**
- Create: `crates/bw-core/src/identity.rs`
- Create: `crates/bw-core/src/cipherstring.rs`

- [x] **Step 0: Wire new modules into lib.rs**

Add to `crates/bw-core/src/lib.rs`:

```rust
pub mod identity;
pub mod cipherstring;
```

- [x] **Step 1: Copy identity.rs and adapt to new crypto APIs**

Copy `.reference/rbw/src/identity.rs` → `crates/bw-core/src/identity.rs`. Then fix for new crate versions:
- `sha2 0.11`: Check if `Digest` trait import path changed (`sha2::Digest` → may now be `digest::Digest`).
- `hmac 0.13` / `pbkdf2 0.12`: Verify `pbkdf2::pbkdf2::<hmac::Hmac<sha2::Sha256>>()` signature is unchanged.
- `hkdf 0.13`: Verify `Hkdf::from_prk()` and `.expand()` signatures.
- `rand 0.10`: `rand::rng()` may have changed — check if it's now `rand::rng()` or `rand::thread_rng()`.

- [x] **Step 2: Copy cipherstring.rs and adapt to new crypto APIs**

Copy `.reference/rbw/src/cipherstring.rs` → `crates/bw-core/src/cipherstring.rs`. Then fix for new crate versions:
- `aes 0.9` / `cbc 0.2`: The `BlockDecryptMut`, `BlockEncryptMut`, `KeyIvInit` traits from `aes::cipher` may have moved. Check `cipher` crate re-exports.
- `cbc::Encryptor::<aes::Aes256>::new(key, iv)` constructor may have a different signature.
- `hmac 0.13`: `hmac::Hmac::new_from_slice()` and `Mac` trait — verify API.
- `block-padding 0.4`: `UnpadError` type — verify it matches `thiserror` `#[error]` source.
- `rand 0.10`: `rand::RngCore` → `fill_bytes` API — verify.
- `pkcs8 0.10` / `rsa 0.9.10`: `DecodePrivateKey`, `RsaPrivateKey::from_pkcs8_der()`, `Oaep::new::<sha1::Sha1>()` — verify signatures.

- [x] **Step 3: Verify crypto chain compiles**

```bash
cargo check -p bw-core
```

Expected: Success.

- [x] **Step 4: Run cipherstring tests**

```bash
cargo test -p bw-core -- cipherstring
```

Expected: `test_pkcs7_unpad` passes (the only test in rbw's cipherstring.rs).

- [x] **Step 5: Commit**

```bash
git add -A && git commit -m "feat(bw-core): add identity and cipherstring crypto modules from rbw"
```

---

## Chunk 2: bw-core API Client + Types

### Task 4: Create api.rs types stub + db.rs — Vault Entry Types

db.rs depends on types from api.rs (`CipherRepromptType`, `FieldType`, `LinkedIdType`, `UriMatchType`). We create both files in this task: api.rs with ONLY the type definitions first, then db.rs. The Client impl and action functions are added in Task 5.

**Files:**
- Create: `crates/bw-core/src/api.rs` (types only — enums, serde structs)
- Create: `crates/bw-core/src/db.rs`

- [x] **Step 0: Wire both modules into lib.rs**

Add to `crates/bw-core/src/lib.rs`:

```rust
pub mod api;
pub mod db;
```

- [x] **Step 1: Create api.rs with type definitions only**

Copy from `.reference/rbw/src/api.rs` ONLY the public type definitions (no `Client`, no functions):
- `UriMatchType` enum + Display impl
- `TwoFactorProviderType` enum + all impls
- `KdfType` enum + all impls
- `CipherRepromptType` enum
- `FieldType` enum
- `LinkedIdType` enum

This file will be extended in Task 5 with the Client struct and all private request/response types.

- [x] **Step 2: Create db.rs with types only**

Copy the following from `.reference/rbw/src/db.rs`:
- `Entry` struct
- `Uri` struct + its custom Deserialize impl
- `EntryData` enum (Login, Card, Identity, SecureNote, SshKey)
- `Field` struct
- `HistoryEntry` struct

Do NOT copy:
- `Db` struct (we don't persist to disk)
- `Db::load()`, `Db::save()`, etc.
- Any `std::fs` or `tokio::fs` usage

The resulting file should define the data types that `api.rs` sync response maps into. Keep all `crate::api::*` references intact (they'll resolve to our trimmed api.rs).

- [x] **Step 2: Add a round-trip serde test for EntryData::SshKey**

Create `crates/bw-core/src/db.rs` inline test:

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

Run: `cargo test -p bw-core -- db::tests`
Expected: PASS

- [x] **Step 3: Verify compiles**

```bash
cargo check -p bw-core
```

- [x] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(bw-core): add vault entry types from rbw (no file I/O)"
```

---

### Task 5: Extend api.rs — Bitwarden REST API Client (trimmed + proxy)

Task 4 created api.rs with public type definitions. Now we add the Client, private request/response structs, and action functions.

We keep: prelogin, login (password only), sync, refresh token.
We remove: register, SSO, add/edit/remove ciphers, folders, send_email_login, blocking variants.

**Files:**
- Modify: `crates/bw-core/src/api.rs` (extend with Client + private types)

- [x] **Step 1: Add private request/response types to api.rs**

Append these types to the existing `api.rs` (which already has the public enums from Task 4):
- `PreloginReq` / `PreloginRes`
- `ConnectTokenReq` / `ConnectTokenAuth` / `ConnectTokenPassword` (remove `ConnectTokenAuthCode` and `ConnectTokenClientCredentials`)
- `ConnectTokenRes`
- `ConnectErrorRes` / `ConnectErrorResErrorModel`
- `ConnectRefreshTokenReq` / `ConnectRefreshTokenRes`
- `SyncRes` / `SyncResCipher` + `to_entry()` method
- `SyncResProfile` / `SyncResProfileOrganization`
- `SyncResFolder`
- `CipherLogin` / `CipherLoginUri` / `CipherCard` / `CipherIdentity` / `CipherSshKey` / `CipherSecureNote`
- `CipherField`
- `SyncResPasswordHistory`
- `classify_login_error` function

- [x] **Step 2: Create the Client struct with proxy support**

Replace rbw's `Client` with this version:

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

    /// Create a new Client configured for the official Bitwarden cloud.
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

- [x] **Step 3: Implement prelogin, login, sync, exchange_refresh_token (async only)**

Port the async versions from rbw's `api.rs`. Key changes:
- Use `self.reqwest_client()` (our new proxy-aware version) instead of rbw's.
- Remove `sso_id` parameter from `login()`. Only password auth.
- Remove all `blocking` variants (add, edit, remove, folders, create_folder).
- Remove `register`, `send_email_login`, SSO callback server code.
- Remove `axum` dependency entirely (was only for SSO).
- The `login()` method simplifies to only `ConnectTokenAuth::Password`.

The `login()` signature becomes:
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

The `sync()` stays the same as rbw but uses our reqwest_client().

The `exchange_refresh_token()` becomes async-only (remove blocking version):
```rust
pub async fn exchange_refresh_token(&self, refresh_token: &str) -> crate::error::Result<String>
```

- [x] **Step 4: Add a `device_id()` helper function**

Instead of rbw's file-based device_id, generate in-memory and optionally persist:

```rust
/// Generate or load a stable device ID.
/// For now, generates a new UUID each run. Future: persist to a config file.
pub fn generate_device_id() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}
```

- [x] **Step 5: Add high-level action functions**

Create convenience functions at the bottom of `api.rs` (or in a separate `actions` section) that combine the low-level API calls, mirroring `rbw/src/actions.rs`:

```rust
/// Perform the full login flow: prelogin → derive keys → login → return session data.
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

/// Sync vault and return decryption-ready data.
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

/// Unlock vault: derive user key from master key, decrypt org keys.
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
    // This is identical to rbw::actions::unlock
    let identity = crate::identity::Identity::new(
        email, password, kdf, iterations, memory, parallelism,
    )?;

    let protected_key = crate::cipherstring::CipherString::new(protected_key)?;
    let key = match protected_key.decrypt_locked_symmetric(&identity.keys) {
        Ok(master_keys) => crate::locked::Keys::new(master_keys),
        Err(crate::error::Error::InvalidMac) => {
            return Err(crate::error::Error::IncorrectPassword {
                message: "Password is incorrect. Try again.".to_string(),
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

- [x] **Step 6: Add unit tests for Client construction and URL helpers**

Add inline tests at the bottom of `api.rs`:

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
        // Should not panic — proxy URL is valid
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
        assert_eq!(id.len(), 36); // UUID hyphenated format
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }
}
```

Run: `cargo test -p bw-core -- api::tests`
Expected: All 4 tests PASS.

- [x] **Step 7: Verify bw-core compiles**

```bash
cargo check -p bw-core
```

- [x] **Step 8: Commit**

```bash
git add -A && git commit -m "feat(bw-core): add Bitwarden API client with proxy support and vault unlock logic"
```

---

## Chunk 3: egui Password Dialog

### Task 6: Implement bw-ui Password Prompt

**Files:**
- Modify: `crates/bw-ui/src/lib.rs`

- [x] **Step 1: Implement the egui password dialog**

```rust
use eframe::egui;
use std::sync::{Arc, Mutex};

/// Show a modal password prompt window. Blocks until the user submits or cancels.
/// Returns `Some(password)` on submit, `None` on cancel/close.
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
                ui.heading("Bitwarden Master Password");
                ui.add_space(10.0);

                ui.label("Enter your master password to unlock SSH keys:");
                ui.add_space(5.0);

                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.password)
                        .password(true)
                        .desired_width(300.0)
                        .hint_text("Master Password"),
                );

                // Auto-focus the password field
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
                    if ui.button("Unlock").clicked() {
                        self.submit(ctx);
                    }
                    if ui.button("Cancel").clicked() {
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
            self.error_msg = Some("Password cannot be empty".to_string());
            return;
        }
        *self.result.lock().unwrap() = Some(self.password.clone());
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}
```

- [x] **Step 2: Verify bw-ui compiles**

```bash
cargo check -p bw-ui
```

- [x] **Step 3: Create and run a test example**

Create `crates/bw-ui/examples/test_dialog.rs`:

```rust
fn main() {
    match bw_ui::prompt_master_password() {
        Some(pw) => println!("Got password ({} chars)", pw.len()),
        None => println!("Cancelled"),
    }
}
```

Run:

```bash
cargo run -p bw-ui --example test_dialog
```

Expected: A 400x180 window titled "Bitwarden SSH Agent - Unlock" appears. Type any text, press Enter or click "Unlock" → console prints "Got password (N chars)". Click "Cancel" → console prints "Cancelled". Submitting empty → red error text "Password cannot be empty".

- [x] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(bw-ui): egui master password prompt dialog"
```

---

## Chunk 4: SSH Agent Binary

### Task 7: Implement state.rs — Shared Auth Cache

**Files:**
- Create: `crates/bw-agent/src/state.rs`
- Modify: `crates/bw-agent/src/main.rs` (add module declarations)

- [x] **Step 0: Wire all agent modules into main.rs upfront**

Replace `crates/bw-agent/src/main.rs` with module stubs so Tasks 7-9 can compile and test independently:

```rust
mod auth;
mod ssh_agent;
mod state;

fn main() {
    println!("bw-agent placeholder");
}
```

Also create empty stubs for `auth.rs` and `ssh_agent.rs`:

`crates/bw-agent/src/auth.rs`:
```rust
// Placeholder — implemented in Task 8
```

`crates/bw-agent/src/ssh_agent.rs`:
```rust
// Placeholder — implemented in Task 9
```

- [x] **Step 1: Implement the cached state**

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Cached authentication state with TTL.
pub struct State {
    /// Decrypted vault keys (user key).
    pub keys: Option<bw_core::locked::Keys>,
    /// Decrypted organization keys.
    pub org_keys: Option<HashMap<String, bw_core::locked::Keys>>,
    /// When the keys were last cached.
    pub cached_at: Option<Instant>,
    /// How long cached keys remain valid.
    pub cache_ttl: Duration,
    /// Cached vault entries (encrypted cipherstrings).
    pub entries: Vec<bw_core::db::Entry>,
    /// Login session data for token refresh.
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    /// KDF parameters (needed for re-unlock).
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

    /// Check if keys are cached and still valid.
    pub fn is_unlocked(&self) -> bool {
        if let (Some(_keys), Some(cached_at)) = (&self.keys, &self.cached_at) {
            cached_at.elapsed() < self.cache_ttl
        } else {
            false
        }
    }

    /// Get the decryption key for a given org_id (or the user key if None).
    pub fn key(&self, org_id: Option<&str>) -> Option<&bw_core::locked::Keys> {
        org_id.map_or(self.keys.as_ref(), |id| {
            self.org_keys.as_ref().and_then(|h| h.get(id))
        })
    }

    /// Clear all cached keys (lock).
    pub fn clear(&mut self) {
        self.keys = None;
        self.org_keys = None;
        self.cached_at = None;
    }

    /// Store unlocked keys and refresh the TTL.
    pub fn set_unlocked(
        &mut self,
        keys: bw_core::locked::Keys,
        org_keys: HashMap<String, bw_core::locked::Keys>,
    ) {
        self.keys = Some(keys);
        self.org_keys = Some(org_keys);
        self.cached_at = Some(Instant::now());
    }

    /// Refresh the TTL (called on each successful SSH operation).
    pub fn touch(&mut self) {
        if self.keys.is_some() {
            self.cached_at = Some(Instant::now());
        }
    }
}
```

- [x] **Step 2: Add unit tests for State TTL logic**

Add tests at the bottom of `state.rs`:

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

Run: `cargo test -p bw-agent -- state::tests`
Expected: All 5 tests PASS.

- [x] **Step 3: Verify compiles**

```bash
cargo check -p bw-agent
```

- [x] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(bw-agent): add auth cache state with 15-min TTL"
```

---

### Task 8: Implement auth.rs — Authentication Flow

**Files:**
- Create: `crates/bw-agent/src/auth.rs`

- [x] **Step 1: Implement the auth orchestrator**

This module coordinates: check cache → prompt password (egui) → login → sync → unlock → cache.

```rust
use crate::state::State;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Ensure the agent is unlocked. If cache expired, prompt for password and re-auth.
pub async fn ensure_unlocked(
    state: Arc<Mutex<State>>,
    client: &bw_core::api::Client,
) -> anyhow::Result<()> {
    let needs_auth = {
        let s = state.lock().await;
        !s.is_unlocked()
    };

    if needs_auth {
        // Prompt for master password via egui (blocks on GUI thread)
        let password_str = tokio::task::spawn_blocking(|| {
            bw_ui::prompt_master_password()
        })
        .await?
        .ok_or_else(|| anyhow::anyhow!("Password prompt cancelled by user"))?;

        let mut password_vec = bw_core::locked::Vec::new();
        password_vec.extend(password_str.as_bytes().iter().copied());
        let password = bw_core::locked::Password::new(password_vec);

        let mut s = state.lock().await;

        // Determine if we need a fresh login or just an unlock
        let needs_login = s.access_token.is_none();

        if needs_login {
            let email = s.email.as_ref()
                .ok_or_else(|| anyhow::anyhow!("Email not configured"))?
                .clone();

            // Full login flow
            let session = bw_core::api::full_login(client, &email, &password).await?;

            s.access_token = Some(session.access_token.clone());
            s.refresh_token = Some(session.refresh_token.clone());
            s.kdf = Some(session.kdf);
            s.iterations = Some(session.iterations);
            s.memory = session.memory;
            s.parallelism = session.parallelism;
            s.protected_key = Some(session.protected_key.clone());

            // Sync vault
            let sync_data = bw_core::api::sync_vault(client, &session.access_token).await?;
            s.protected_private_key = Some(sync_data.protected_private_key.clone());
            s.protected_org_keys = sync_data.org_keys.clone();
            s.entries = sync_data.entries;

            // Unlock
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
            // Re-unlock with existing session data
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

            // Also re-sync in background if access_token is valid
            let access_token = s.access_token.clone().unwrap();
            drop(s); // Release lock before network call

            if let Ok(sync_data) = bw_core::api::sync_vault(client, &access_token).await {
                let mut s = state.lock().await;
                s.entries = sync_data.entries;
                s.protected_org_keys = sync_data.org_keys;
            }
        }
    } else {
        // Touch the TTL on each access
        state.lock().await.touch();
    }

    Ok(())
}

/// Decrypt a cipher string using the cached keys.
pub fn decrypt_cipher(
    state: &State,
    cipherstring: &str,
    entry_key: Option<&str>,
    org_id: Option<&str>,
) -> anyhow::Result<String> {
    let keys = state.key(org_id)
        .ok_or_else(|| anyhow::anyhow!("No decryption keys available"))?;

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

- [x] **Step 2: Add unit test for decrypt_cipher with known test vectors**

Add tests at the bottom of `auth.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decrypt_cipher_returns_error_when_locked() {
        let state = State::new(std::time::Duration::from_secs(900));
        let result = decrypt_cipher(&state, "2.fake|data|mac", None, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No decryption keys"));
    }
}
```

Run: `cargo test -p bw-agent -- auth::tests`
Expected: PASS — confirms that `decrypt_cipher` correctly errors when the state is locked.

- [x] **Step 3: Verify compiles**

```bash
cargo check -p bw-agent
```

- [x] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(bw-agent): auth flow with egui password prompt and vault unlock"
```

---

### Task 9: Implement ssh_agent.rs — SSH Agent Protocol

**Files:**
- Create: `crates/bw-agent/src/ssh_agent.rs`

- [x] **Step 1: Port ssh_agent.rs from rbw**

Adapt `.reference/rbw/src/bin/rbw-agent/ssh_agent.rs`. Key changes:
- Replace `crate::state::State` references with our `crate::state::State`
- Replace `crate::actions::get_ssh_public_keys` / `find_ssh_private_key` with inline logic using `crate::auth`
- Add `bw_core::api::Client` to the struct for auth flow
- The `run()` method uses `#[cfg]` for platform-specific listener

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
        // Ensure we're unlocked (may trigger egui prompt)
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

        // Ensure unlocked
        auth::ensure_unlocked(self.state.clone(), &self.client)
            .await
            .map_err(|e| ssh_agent_lib::error::AgentError::Other(e.into()))?;

        let request_pubkey = ssh_agent_lib::ssh_key::PublicKey::new(request.pubkey.clone(), "");
        let request_bytes = request_pubkey.to_bytes();

        // Find matching private key
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

                // Found matching key — decrypt private key
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

                // Sign based on key type — identical to rbw's logic
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
                        format!("Unsupported key type: {other:?}").into(),
                    )),
                };
            }
        }

        Err(ssh_agent_lib::error::AgentError::Other(
            "No matching private key found".into(),
        ))
    }
}
```

- [x] **Step 2: Add unit test for SshAgentHandler construction**

Add tests at the bottom of `ssh_agent.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_request_identities_returns_empty_when_no_entries() {
        let state = Arc::new(Mutex::new(
            crate::state::State::new(std::time::Duration::from_secs(900)),
        ));
        // Pre-unlock with dummy keys so it doesn't trigger egui prompt
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
        assert!(identities.is_empty()); // No entries = no identities
    }
}
```

Run: `cargo test -p bw-agent -- ssh_agent::tests`
Expected: PASS — confirms that an unlocked agent with zero vault entries returns an empty identity list.

- [x] **Step 3: Verify full project compiles**

```bash
cargo check -p bw-agent
```

- [x] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(bw-agent): SSH agent protocol handler with Ed25519 and RSA signing"
```

---

### Task 10: Implement main.rs — Entry Point

**Files:**
- Modify: `crates/bw-agent/src/main.rs`

- [x] **Step 1: Implement main with CLI args and agent startup**

```rust
mod auth;
mod ssh_agent;
mod state;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// Default named pipe path for OpenSSH on Windows.
#[cfg(windows)]
const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\openssh-ssh-agent";

/// Default Unix socket path.
#[cfg(unix)]
fn default_socket_path() -> String {
    std::env::var("SSH_AUTH_SOCK").unwrap_or_else(|_| {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| "/tmp".to_string());
        format!("{runtime_dir}/bw-agent.sock")
    })
}

const DEFAULT_CACHE_TTL_SECS: u64 = 900; // 15 minutes
const DEFAULT_PROXY: &str = "http://127.0.0.1:7890";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Read config from environment variables
    let email = std::env::var("BW_EMAIL")
        .expect("BW_EMAIL environment variable must be set");
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

    log::info!("Starting bw-agent SSH agent...");

    #[cfg(windows)]
    {
        let pipe_name = std::env::var("BW_PIPE_NAME")
            .unwrap_or_else(|_| DEFAULT_PIPE_NAME.to_string());
        log::info!("Listening on named pipe: {pipe_name}");
        let listener = ssh_agent_lib::agent::NamedPipeListener::bind(pipe_name)?;
        ssh_agent_lib::agent::listen(listener, handler).await?;
    }

    #[cfg(unix)]
    {
        let socket_path = std::env::var("BW_SOCKET_PATH")
            .unwrap_or_else(|_| default_socket_path());
        let _ = std::fs::remove_file(&socket_path);
        log::info!("Listening on Unix socket: {socket_path}");
        let listener = tokio::net::UnixListener::bind(&socket_path)?;
        ssh_agent_lib::agent::listen(listener, handler).await?;
    }

    Ok(())
}
```

- [x] **Step 2: Verify the full project compiles**

```bash
cargo check --workspace
```

- [x] **Step 3: Verify the binary builds**

```bash
cargo build -p bw-agent
```

Expected: Success. Binary at `target/debug/bw-agent.exe` (Windows).

- [x] **Step 4: Commit**

```bash
git add -A && git commit -m "feat(bw-agent): main entry point with env-based config and platform listener"
```

---

## Chunk 5: Integration & Polish

### Task 11: Fix Compilation Issues and Dependency Alignment

This task is reserved for fixing any compilation errors that arise during the previous tasks. Common issues:

- [x] **Step 1: Fix version mismatches**

Ensure `rsa`, `sha1`, `sha2`, `signature` crate versions are compatible between `bw-core` and `bw-agent`. The `ssh-agent-lib` 0.5.x re-exports `ssh-key` 0.6.x which uses specific versions of crypto crates. Check with:

```bash
cargo tree -p bw-agent -d
```

Resolve any duplicate versions by aligning `[workspace.dependencies]`.

- [x] **Step 2: Fix `rand` version compatibility**

We use `rand 0.10.1` everywhere. Verify that `ssh-agent-lib 0.5.2` and `rsa 0.9.10` are compatible with `rand 0.10`. If `ssh-agent-lib` or `rsa` re-export or depend on an older `rand`, you may need to add a compat alias like `rand_08 = { package = "rand", version = "0.8" }` for the RSA signing code only. Check with `cargo tree -p bw-agent -i rand`.

Also verify: `rand::rngs::OsRng` in rand 0.10 — it may have moved. In rand 0.10, `OsRng` is at `rand::rngs::OsRng` or just `rand::rngs::OsRng`. Check the docs.

- [x] **Step 3: Full build**

```bash
cargo build --workspace
```

- [x] **Step 4: Run full test suite**

```bash
cargo test --workspace
```

Expected: ALL tests pass — this includes:
- `bw-core::cipherstring::test_pkcs7_unpad`
- `bw-core::db::tests::test_ssh_key_entry_roundtrip`
- `bw-core::api::tests::*` (4 tests)
- `bw-agent::state::tests::*` (5 tests)
- `bw-agent::auth::tests::*` (1 test)
- `bw-agent::ssh_agent::tests::*` (1 test)

If any test fails, fix the issue before committing.

- [x] **Step 5: Commit**

```bash
git add -A && git commit -m "fix: resolve dependency version conflicts across workspace"
```

---

### Task 12: End-to-End Smoke Test

- [x] **Step 1: Set environment and run**

```powershell
$env:BW_EMAIL = "your-email@example.com"
$env:BW_PROXY = "http://127.0.0.1:7890"
$env:RUST_LOG = "debug"
cargo run -p bw-agent
```

Expected: Agent starts, prints "Listening on named pipe: \\.\pipe\openssh-ssh-agent"

- [x] **Step 2: Test with ssh-add**

In another terminal (no env vars needed — Windows `ssh-add.exe` automatically connects to `\\.\pipe\openssh-ssh-agent`):

> **Prerequisite:** Stop the Windows built-in OpenSSH Agent service first, otherwise bw-agent can't bind the default pipe:
> ```powershell
> Stop-Service ssh-agent
> Set-Service ssh-agent -StartupType Disabled
> ```

```powershell
ssh-add -l
```

Expected: egui password dialog pops up. After entering the correct password, SSH keys from Bitwarden vault are listed.

- [x] **Step 3: Test signing with ssh -T**

No extra config needed — `ssh.exe` on Windows automatically uses `\\.\pipe\openssh-ssh-agent`:

```powershell
ssh -T git@github.com
```

Expected: If the Bitwarden vault has an SSH key registered with GitHub, you should see `Hi <username>! You've authenticated...`. If not registered, you'll see `Permission denied (publickey).` — but the important thing is **no egui prompt appears** (within the 15-min cache window), confirming the cache works. Check agent debug logs to verify the signing request was processed.

- [x] **Step 4: Test cache expiry**

Wait 15 minutes (or temporarily set `BW_CACHE_TTL=10` for 10 seconds). Then retry `ssh-add -l`. Expected: egui dialog pops up again.

- [x] **Step 5: Final commit**

```bash
git add -A && git commit -m "chore: integration smoke test passed"
```

---

## Environment Variables Reference

| Variable | Default | Description |
|---|---|---|
| `BW_EMAIL` | (required) | Bitwarden account email |
| `BW_BASE_URL` | `https://api.bitwarden.com` | Bitwarden API base URL |
| `BW_IDENTITY_URL` | `https://identity.bitwarden.com` | Bitwarden identity URL |
| `BW_PROXY` | `http://127.0.0.1:7890` | HTTP proxy for all API requests |
| `BW_CACHE_TTL` | `900` (15 min) | Auth cache TTL in seconds |
| `BW_PIPE_NAME` (Windows) | `\\.\pipe\openssh-ssh-agent` | Named pipe path |
| `BW_SOCKET_PATH` (Unix) | `$SSH_AUTH_SOCK` or `$XDG_RUNTIME_DIR/bw-agent.sock` | Unix socket path |
| `RUST_LOG` | (unset) | Log level (`debug`, `info`, `warn`) |
