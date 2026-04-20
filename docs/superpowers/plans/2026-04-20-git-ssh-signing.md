# Git SSH Commit 签名 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 bw-agent 完全替代 ssh-keygen 作为 Git 的 SSH 签名程序，并在 UI 上提供一键配置。

**Architecture:** 新增 `git-sign` CLI 子命令，通过 SSH Agent 协议连接运行中的 bw-agent 进行签名（复用审批机制），并用 `ssh-key` crate 原生构造/验证 SSHSIG 格式。Tauri 端新增 command 执行 `git config --global`，前端设置页添加 "Git Signing" 分区。

**Tech Stack:** Rust (clap, ssh-agent-lib blocking client, ssh-key SshSig), TypeScript/SolidJS (Tauri IPC)

**Spec:** `docs/superpowers/specs/2026-04-20-git-ssh-signing-design.md`

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/bw-agent/Cargo.toml` | 添加 `clap` 依赖 |
| `crates/bw-agent/src/main.rs` | clap 子命令分发：默认启动 agent / `git-sign` 进入签名模式 |
| `crates/bw-agent/src/lib.rs` | 注册 `git_sign` 模块 |
| `crates/bw-agent/src/git_sign.rs` | **新增**：SSH Agent 客户端连接 + SSHSIG 编解码 + 四个 `-Y` 子命令 |
| `src-tauri/src/commands.rs` | 新增 `get_git_signing_status`、`configure_git_signing` |
| `src-tauri/src/main.rs` | 注册新 command 到 `invoke_handler` |
| `src/lib/tauri.ts` | 新增 `GitSigningStatus` 接口 + invoke wrapper |
| `src/pages/SettingsPage.tsx` | "Git Signing" 分区 UI |

---

## Chunk 1: CLI Infrastructure + SSHSIG sign

### Task 1: Add clap dependency

**Files:**
- Modify: `crates/bw-agent/Cargo.toml`

- [x] **Step 1: Add clap to Cargo.toml**

In `crates/bw-agent/Cargo.toml`, add after the `rusqlite` line:

```toml
clap = { version = "4", features = ["derive"] }
```

- [x] **Step 2: Verify it compiles**

Run: `cargo check --package bw-agent`
Expected: Compiles successfully (clap imported but unused)

- [x] **Step 3: Commit**

```bash
git add crates/bw-agent/Cargo.toml
git commit -m "chore: add clap dependency for CLI subcommands"
```

---

### Task 2: Refactor main.rs with clap subcommands

**Files:**
- Modify: `crates/bw-agent/src/main.rs`
- Modify: `crates/bw-agent/src/lib.rs`

- [x] **Step 1: Add `git_sign` module to lib.rs**

In `crates/bw-agent/src/lib.rs`, add after `pub mod git_context;`:

```rust
pub mod git_sign;
```

- [x] **Step 2: Rewrite main.rs with clap**

Replace the entire content of `crates/bw-agent/src/main.rs` with:

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bw-agent", about = "Bitwarden-backed SSH agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Act as Git SSH signing program (gpg.ssh.program)
    GitSign {
        /// Action to perform
        #[arg(short = 'Y', long)]
        action: String,
        /// Namespace (e.g. "git")
        #[arg(short = 'n', long)]
        namespace: Option<String>,
        /// Key file or allowed_signers file
        #[arg(short = 'f', long)]
        key_file: Option<String>,
        /// Principal identity
        #[arg(short = 'I', long)]
        principal: Option<String>,
        /// Signature file
        #[arg(short = 's', long)]
        signature_file: Option<String>,
        /// Use SSH agent for signing
        #[arg(short = 'U')]
        use_agent: bool,
        /// Options (e.g. -Overify-time=...)
        #[arg(short = 'O', number_of_values = 1)]
        options: Vec<String>,
        /// Data file (positional, for sign)
        data_file: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: start SSH agent daemon
            let mut config = bw_agent::config::Config::load();
            config.apply_env_overrides();
            config.validate()?;
            let ui = bw_agent::StubUiCallback;
            bw_agent::start_agent(config, ui).await
        }
        Some(Commands::GitSign {
            action,
            namespace,
            key_file,
            principal,
            signature_file,
            use_agent: _,
            options: _,
            data_file,
        }) => {
            bw_agent::git_sign::run(&action, namespace.as_deref(), key_file.as_deref(),
                principal.as_deref(), signature_file.as_deref(), data_file.as_deref())
        }
    }
}
```

- [x] **Step 3: Create stub git_sign.rs**

Create `crates/bw-agent/src/git_sign.rs` with:

```rust
//! Git SSH signing program (gpg.ssh.program replacement).

pub fn run(
    action: &str,
    namespace: Option<&str>,
    key_file: Option<&str>,
    principal: Option<&str>,
    signature_file: Option<&str>,
    data_file: Option<&str>,
) -> anyhow::Result<()> {
    eprintln!("error: unknown action: {action}");
    std::process::exit(1);
}
```

- [x] **Step 4: Verify it compiles and existing behavior preserved**

Run: `cargo check --package bw-agent`
Run: `cargo test --package bw-agent`
Expected: All compiles, all existing tests pass

- [x] **Step 5: Test CLI subcommand parsing**

Run: `cargo run --package bw-agent -- git-sign -Y sign -n git -f NUL NUL` (Windows)
Or: `cargo run --package bw-agent -- git-sign -Y sign -n git -f /dev/null /dev/null` (Unix)
Expected: The stub outputs `error: unknown action: sign` and exits with code 1

Run: `cargo run --package bw-agent -- --help`
Expected: Shows help with `git-sign` subcommand listed

- [x] **Step 6: Commit**

```bash
git add crates/bw-agent/src/main.rs crates/bw-agent/src/lib.rs crates/bw-agent/src/git_sign.rs
git commit -m "feat: add clap CLI with git-sign subcommand stub"
```

---

### Task 3: Implement SSHSIG sign via SSH Agent

**Files:**
- Modify: `crates/bw-agent/src/git_sign.rs`

- [x] **Step 1: Write test for SSHSIG construction**

Add to `crates/bw-agent/src/git_sign.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_sshsig_roundtrip() {
        // Generate a test Ed25519 keypair
        let private_key = ssh_key::PrivateKey::random(
            &mut ssh_key::rand_core::OsRng,
            ssh_key::Algorithm::Ed25519,
        ).unwrap();
        let public_key = private_key.public_key();

        let data = b"test commit data";
        let namespace = "git";

        // Build signed_data blob
        let hash_alg = ssh_key::HashAlg::Sha512;
        let signed_data = ssh_key::SshSig::signed_data(namespace, hash_alg, data).unwrap();

        // Sign with the private key directly
        use signature::Signer;
        let signature: ssh_key::Signature = private_key.sign(&signed_data);

        // Construct SSHSIG
        let sshsig = ssh_key::SshSig::new(
            public_key.key_data().clone(),
            namespace,
            hash_alg,
            signature,
        ).unwrap();

        // Verify PEM roundtrip
        let pem = sshsig.to_pem(ssh_key::LineEnding::LF).unwrap();
        assert!(pem.starts_with("-----BEGIN SSH SIGNATURE-----"));
        assert!(pem.ends_with("-----END SSH SIGNATURE-----\n"));

        // Parse back and verify
        let parsed = ssh_key::SshSig::from_pem(&pem).unwrap();
        assert!(public_key.verify(namespace, data, &parsed).is_ok());
    }

    #[test]
    fn test_agent_socket_path_unix() {
        // Test that the socket path resolution works
        let path = agent_socket_path();
        assert!(!path.is_empty());
    }
}
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo test --package bw-agent -- git_sign`
Expected: Fails — `agent_socket_path` not yet defined

- [x] **Step 3: Implement the sign action**

Replace the entire content of `crates/bw-agent/src/git_sign.rs` with the full implementation:

```rust
//! Git SSH signing program — drop-in replacement for ssh-keygen -Y.
//!
//! Implements sign, verify, find-principals, and check-novalidate.

use std::io::{self, Read, Write};

// ---------------------------------------------------------------------------
// Agent connection
// ---------------------------------------------------------------------------

/// Resolve the SSH agent socket/pipe path.
fn agent_socket_path() -> String {
    #[cfg(unix)]
    {
        std::env::var("SSH_AUTH_SOCK").unwrap_or_else(|_| {
            let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
            format!("{runtime_dir}/bw-agent.sock")
        })
    }
    #[cfg(windows)]
    {
        std::env::var("BW_PIPE_NAME").unwrap_or_else(|_| r"\\.\pipe\openssh-ssh-agent".to_string())
    }
}

/// Connect to the running SSH agent via blocking client and request a signature.
fn sign_via_agent(
    pubkey_bytes: &[u8],
    data_to_sign: &[u8],
    is_rsa: bool,
) -> Result<ssh_key::Signature, String> {
    let flags: u32 = if is_rsa { 0x04 } else { 0x00 }; // SSH_AGENT_RSA_SHA2_512 = 0x04

    #[cfg(unix)]
    {
        use ssh_agent_lib::agent::Session;
        let stream =
            std::os::unix::net::UnixStream::connect(agent_socket_path())
                .map_err(|e| format!("Cannot connect to agent: {e}"))?;
        let mut client = ssh_agent_lib::blocking::Client::new(stream);

        let identities = client
            .request_identities()
            .map_err(|e| format!("Failed to list identities: {e}"))?;

        // Find identity matching the public key
        let identity = identities
            .into_iter()
            .find(|id| {
                let pubkey = ssh_key::PublicKey::from(id.key_data().clone());
                pubkey.to_bytes().map_or(false, |b| b == pubkey_bytes)
            })
            .ok_or_else(|| "No matching key found in agent".to_string())?;

        let request = ssh_agent_lib::proto::SignRequest {
            pubkey: identity.key_data().clone().into(),
            data: data_to_sign.to_vec(),
            flags,
        };

        client.sign(request).map_err(|e| format!("Agent sign failed: {e}"))
    }

    #[cfg(windows)]
    {
        // Windows named pipe: connect and use blocking client
        // ssh-agent-lib blocking client implements Read+Write on generic stream
        use ssh_agent_lib::agent::Session;
        let pipe_path = agent_socket_path();
        let stream = std::fs::File::open(&pipe_path)
            .map_err(|e| format!("Cannot open pipe {}: {e}", pipe_path))?;

        // Windows named pipe requires special handling.
        // Fall back to opening as raw pipe and using blocking client.
        let mut client = ssh_agent_lib::blocking::Client::new(stream);

        let identities = client
            .request_identities()
            .map_err(|e| format!("Failed to list identities: {e}"))?;

        let identity = identities
            .into_iter()
            .find(|id| {
                let pubkey = ssh_key::PublicKey::from(id.key_data().clone());
                pubkey.to_bytes().map_or(false, |b| b == pubkey_bytes)
            })
            .ok_or_else(|| "No matching key found in agent".to_string())?;

        let request = ssh_agent_lib::proto::SignRequest {
            pubkey: identity.key_data().clone().into(),
            data: data_to_sign.to_vec(),
            flags,
        };

        client.sign(request).map_err(|e| format!("Agent sign failed: {e}"))
    }
}

// ---------------------------------------------------------------------------
// -Y sign
// ---------------------------------------------------------------------------

fn do_sign(
    namespace: &str,
    key_file: &str,
    data_file: &str,
) -> anyhow::Result<()> {
    // Read public key
    let pubkey_str = std::fs::read_to_string(key_file)
        .map_err(|e| anyhow::anyhow!("Cannot read key file {key_file}: {e}"))?;
    let pubkey = ssh_key::PublicKey::from_openssh(&pubkey_str)
        .map_err(|e| anyhow::anyhow!("Invalid public key: {e}"))?;
    let pubkey_bytes = pubkey.to_bytes()
        .map_err(|e| anyhow::anyhow!("Cannot encode public key: {e}"))?;

    // Detect key type for RSA flag
    let is_rsa = matches!(
        pubkey.key_data(),
        ssh_key::public::KeyData::Rsa(_)
    );

    // Read data to sign
    let data = std::fs::read(data_file)
        .map_err(|e| anyhow::anyhow!("Cannot read data file {data_file}: {e}"))?;

    eprintln!("Signing file {data_file}");

    // Build SSHSIG signed_data blob
    let hash_alg = ssh_key::HashAlg::Sha512;
    let signed_data = ssh_key::SshSig::signed_data(namespace, hash_alg, &data)
        .map_err(|e| anyhow::anyhow!("Failed to build signed data: {e}"))?;

    // Sign via agent
    let signature = sign_via_agent(&pubkey_bytes, &signed_data, is_rsa)
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Construct SSHSIG
    let sshsig = ssh_key::SshSig::new(
        pubkey.key_data().clone(),
        namespace,
        hash_alg,
        signature,
    ).map_err(|e| anyhow::anyhow!("Failed to construct SSHSIG: {e}"))?;

    let pem = sshsig.to_pem(ssh_key::LineEnding::LF)
        .map_err(|e| anyhow::anyhow!("Failed to encode SSHSIG: {e}"))?;

    // Write to <data_file>.sig
    let sig_path = format!("{data_file}.sig");
    std::fs::write(&sig_path, pem.as_bytes())
        .map_err(|e| anyhow::anyhow!("Cannot write {sig_path}: {e}"))?;

    eprintln!("Write signature to {sig_path}");
    Ok(())
}

// ---------------------------------------------------------------------------
// -Y verify / find-principals / check-novalidate (stub for this chunk)
// ---------------------------------------------------------------------------

fn do_verify(
    _namespace: &str,
    _key_file: &str,
    _principal: &str,
    _signature_file: &str,
) -> anyhow::Result<()> {
    eprintln!("error: verify not yet implemented");
    std::process::exit(1);
}

fn do_find_principals(
    _key_file: &str,
    _signature_file: &str,
) -> anyhow::Result<()> {
    eprintln!("error: find-principals not yet implemented");
    std::process::exit(1);
}

fn do_check_novalidate(
    _namespace: &str,
    _signature_file: &str,
) -> anyhow::Result<()> {
    eprintln!("error: check-novalidate not yet implemented");
    std::process::exit(1);
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run(
    action: &str,
    namespace: Option<&str>,
    key_file: Option<&str>,
    principal: Option<&str>,
    signature_file: Option<&str>,
    data_file: Option<&str>,
) -> anyhow::Result<()> {
    match action {
        "sign" => {
            let ns = namespace.ok_or_else(|| anyhow::anyhow!("missing -n namespace"))?;
            let kf = key_file.ok_or_else(|| anyhow::anyhow!("missing -f key_file"))?;
            let df = data_file.ok_or_else(|| anyhow::anyhow!("missing data_file argument"))?;
            do_sign(ns, kf, df)
        }
        "verify" => {
            let ns = namespace.ok_or_else(|| anyhow::anyhow!("missing -n namespace"))?;
            let kf = key_file.ok_or_else(|| anyhow::anyhow!("missing -f allowed_signers"))?;
            let pr = principal.ok_or_else(|| anyhow::anyhow!("missing -I principal"))?;
            let sf = signature_file.ok_or_else(|| anyhow::anyhow!("missing -s signature_file"))?;
            do_verify(ns, kf, pr, sf)
        }
        "find-principals" => {
            let kf = key_file.ok_or_else(|| anyhow::anyhow!("missing -f allowed_signers"))?;
            let sf = signature_file.ok_or_else(|| anyhow::anyhow!("missing -s signature_file"))?;
            do_find_principals(kf, sf)
        }
        "check-novalidate" => {
            let ns = namespace.ok_or_else(|| anyhow::anyhow!("missing -n namespace"))?;
            let sf = signature_file.ok_or_else(|| anyhow::anyhow!("missing -s signature_file"))?;
            do_check_novalidate(ns, sf)
        }
        _ => {
            eprintln!("error: unknown action: {action}");
            std::process::exit(1);
        }
    }
}
```

**IMPORTANT NOTE for Windows implementation**: The Windows named pipe client in ssh-agent-lib may require using `tokio` or a custom `Read+Write` impl for named pipes. The implementer should verify the blocking client works with Windows named pipes, and if not, use `tokio::net::windows::named_pipe` with a small async runtime (`#[tokio::main]`) for the `git-sign` subcommand. The core logic (SSHSIG construction) remains the same either way.

- [x] **Step 4: Run tests**

Run: `cargo test --package bw-agent -- git_sign`
Expected: Tests pass

- [x] **Step 5: Verify compilation**

Run: `cargo check --package bw-agent`
Expected: Compiles successfully

- [x] **Step 6: Commit**

```bash
git add crates/bw-agent/src/git_sign.rs
git commit -m "feat: implement SSHSIG sign via SSH agent client"
```

---

## Chunk 2: verify / find-principals / check-novalidate

### Task 4: Implement SSHSIG parse + verify

**Files:**
- Modify: `crates/bw-agent/src/git_sign.rs`

- [x] **Step 1: Write tests for SSHSIG parsing**

Add to the `tests` module in `git_sign.rs`:

```rust
#[test]
fn test_sshsig_parse_valid_pem() {
    // Generate a keypair and sign some data
    let private_key = ssh_key::PrivateKey::random(
        &mut ssh_key::rand_core::OsRng,
        ssh_key::Algorithm::Ed25519,
    ).unwrap();
    let public_key = private_key.public_key();
    let data = b"test data";
    let namespace = "git";

    use signature::Signer;
    let hash_alg = ssh_key::HashAlg::Sha512;
    let signed_data = ssh_key::SshSig::signed_data(namespace, hash_alg, data).unwrap();
    let signature: ssh_key::Signature = private_key.sign(&signed_data);
    let sshsig = ssh_key::SshSig::new(
        public_key.key_data().clone(), namespace, hash_alg, signature,
    ).unwrap();
    let pem = sshsig.to_pem(ssh_key::LineEnding::LF).unwrap();

    // Parse it back
    let parsed = ssh_key::SshSig::from_pem(&pem).unwrap();
    assert!(public_key.verify(namespace, data, &parsed).is_ok());
    // Wrong data should fail
    assert!(public_key.verify(namespace, b"wrong data", &parsed).is_err());
    // Wrong namespace should fail
    assert!(public_key.verify("file", data, &parsed).is_err());
}

#[test]
fn test_parse_allowed_signers() {
    let content = "user@example.com ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAITestKey\n";
    let entries = parse_allowed_signers(content);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].principal, "user@example.com");
}

#[test]
fn test_key_type_string() {
    let key = ssh_key::PrivateKey::random(
        &mut ssh_key::rand_core::OsRng,
        ssh_key::Algorithm::Ed25519,
    ).unwrap();
    let pubkey = key.public_key();
    assert_eq!(key_type_str(pubkey.key_data()), "ED25519");
}
```

- [x] **Step 2: Run tests to verify they fail**

Run: `cargo test --package bw-agent -- git_sign`
Expected: Fails — helper functions not defined

- [x] **Step 3: Implement helper functions and verify actions**

Add these helper functions to `git_sign.rs` (outside any module):

```rust
/// Parse an allowed_signers file into (principal, PublicKey) pairs.
fn parse_allowed_signers(content: &str) -> Vec<(String, ssh_key::PublicKey)> {
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, whitespace).collect();
        if parts.len() < 3 {
            continue;
        }
        // Format: <principal> <key_type> <base64_key> [comment...]
        let key_str = format!("{} {}", parts[1], parts[2]);
        if let Ok(pubkey) = ssh_key::PublicKey::from_openssh(&key_str) {
            entries.push((parts[0].to_string(), pubkey));
        }
    }
    entries
}

/// Get the key type string matching ssh-keygen output.
fn key_type_str(key_data: &ssh_key::public::KeyData) -> &'static str {
    match key_data {
        ssh_key::public::KeyData::Ed25519(_) => "ED25519",
        ssh_key::public::KeyData::Rsa(_) => "RSA",
        ssh_key::public::KeyData::Ecdsa(_) => "ECDSA",
        ssh_key::public::KeyData::SkEd25519(_) => "SK-ED25519",
        ssh_key::public::KeyData::SkEcdsa(_) => "SK-ECDSA",
        _ => "UNKNOWN",
    }
}

/// Parse SSHSIG from a .sig file.
fn parse_signature_file(path: &str) -> anyhow::Result<ssh_key::SshSig> {
    let pem = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Cannot read signature file {path}: {e}"))?;
    ssh_key::SshSig::from_pem(&pem)
        .map_err(|e| anyhow::anyhow!("Invalid SSHSIG format: {e}"))
}
```

Now replace the stub implementations:

```rust
fn do_verify(
    namespace: &str,
    key_file: &str,
    principal: &str,
    signature_file: &str,
) -> anyhow::Result<()> {
    let sshsig = parse_signature_file(signature_file)?;
    let sign_key = sshsig.public_key();

    // Read data from stdin
    let mut data = Vec::new();
    io::stdin().read_to_end(&mut data)?;

    // Verify signature
    let verify_result = sign_key.verify(namespace, &data, &sshsig);
    if let Err(e) = verify_result {
        println!("Could not verify signature.");
        eprintln!("error: Signature verification failed: {e}");
        std::process::exit(255);
    }

    // Load allowed_signers and check principal
    let allowed = std::fs::read_to_string(key_file)
        .map_err(|e| anyhow::anyhow!("Cannot read allowed_signers: {e}"))?;
    let entries = parse_allowed_signers(&allowed);

    // Find matching principal
    let fp = sign_key.fingerprint(Default::default());
    let matched = entries.iter().find(|(_, pk)| {
        pk.to_bytes().map_or(false, |b| b == sign_key.to_bytes().unwrap_or_default())
    });

    if let Some((matched_principal, _)) = matched {
        println!(
            "Good \"{}\" signature for {} with {} key {}",
            namespace,
            matched_principal,
            key_type_str(sign_key.key_data()),
            fp,
        );
    } else {
        // Key verified but not in allowed_signers
        println!(
            "Good \"{}\" signature with {} key {}",
            namespace,
            key_type_str(sign_key.key_data()),
            fp,
        );
    }

    Ok(())
}

fn do_find_principals(
    key_file: &str,
    signature_file: &str,
) -> anyhow::Result<()> {
    let sshsig = parse_signature_file(signature_file)?;
    let sign_key = sshsig.public_key();

    let allowed = std::fs::read_to_string(key_file)
        .map_err(|e| anyhow::anyhow!("Cannot read allowed_signers: {e}"))?;
    let entries = parse_allowed_signers(&allowed);

    let sign_key_bytes = sign_key.to_bytes().map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut found = false;
    for (principal, pk) in &entries {
        if let Ok(b) = pk.to_bytes() {
            if b == sign_key_bytes {
                println!("{principal}");
                found = true;
            }
        }
    }

    if !found {
        eprintln!("No principal matched.");
        std::process::exit(255);
    }

    Ok(())
}

fn do_check_novalidate(
    namespace: &str,
    signature_file: &str,
) -> anyhow::Result<()> {
    let sshsig = parse_signature_file(signature_file)?;
    let sign_key = sshsig.public_key();
    let fp = sign_key.fingerprint(Default::default());

    // Read data from stdin
    let mut data = Vec::new();
    io::stdin().read_to_end(&mut data)?;

    // Verify signature cryptographically (no trust chain check)
    match sign_key.verify(namespace, &data, &sshsig) {
        Ok(()) => {
            println!(
                "Good \"{}\" signature with {} key {}",
                namespace,
                key_type_str(sign_key.key_data()),
                fp,
            );
            Ok(())
        }
        Err(e) => {
            println!("Could not verify signature.");
            eprintln!("error: Signature verification failed: {e}");
            std::process::exit(255);
        }
    }
}
```

- [x] **Step 4: Run all tests**

Run: `cargo test --package bw-agent -- git_sign`
Expected: All tests pass

- [x] **Step 5: Run full workspace check**

Run: `cargo check --workspace`
Expected: Compiles successfully

- [x] **Step 6: Commit**

```bash
git add crates/bw-agent/src/git_sign.rs
git commit -m "feat: implement verify, find-principals, check-novalidate"
```

---

## Chunk 3: Tauri backend commands

### Task 5: Add git signing status + configure commands

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/main.rs`

- [x] **Step 1: Add GitSigningStatus struct and commands to commands.rs**

Add to the top of `src-tauri/src/commands.rs` (after the existing structs):

```rust
#[derive(serde::Serialize)]
pub struct GitSigningStatus {
    pub ssh_program: Option<String>,
    pub gpg_format: Option<String>,
    pub commit_gpgsign: bool,
}
```

Add the two commands (before the helper functions at the bottom):

```rust
#[tauri::command]
pub async fn get_git_signing_status() -> Result<GitSigningStatus, String> {
    let ssh_program = git_config_get("gpg.ssh.program").ok();
    let gpg_format = git_config_get("gpg.format").ok();
    let commit_gpgsign = git_config_get("commit.gpgsign")
        .map(|v| v == "true")
        .unwrap_or(false);

    Ok(GitSigningStatus {
        ssh_program,
        gpg_format,
        commit_gpgsign,
    })
}

#[tauri::command]
pub async fn configure_git_signing() -> Result<(), String> {
    let exe_path = std::env::current_exe()
        .map_err(|e| format!("Cannot determine executable path: {e}"))?;
    let program = format!("{} git-sign", exe_path.display());

    git_config_set("gpg.ssh.program", &program)?;
    git_config_set("gpg.format", "ssh")?;
    git_config_set("commit.gpgsign", "true")?;

    Ok(())
}

fn git_config_get(key: &str) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["config", "--global", "--get", key])
        .output()
        .map_err(|e| format!("Failed to run git: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!("git config --get {} failed", key))
    }
}

fn git_config_set(key: &str, value: &str) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(["config", "--global", key, value])
        .output()
        .map_err(|e| format!("Failed to run git: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git config --global {} {} failed: {}",
            key,
            value,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
```

- [x] **Step 2: Register new commands in main.rs**

In `src-tauri/src/main.rs`, add to the `invoke_handler` list (after `commands::update_lock_mode`):

```rust
commands::get_git_signing_status,
commands::configure_git_signing,
```

- [x] **Step 3: Verify compilation**

Run: `cargo check --workspace`
Expected: Compiles successfully

- [x] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/main.rs
git commit -m "feat: add get_git_signing_status and configure_git_signing commands"
```

---

## Chunk 4: Frontend UI

### Task 6: Add TypeScript types and invoke wrappers

**Files:**
- Modify: `src/lib/tauri.ts`

- [x] **Step 1: Add GitSigningStatus interface and wrappers**

Add to `src/lib/tauri.ts` (after the existing interfaces, before the invoke wrappers):

```typescript
export interface GitSigningStatus {
  ssh_program: string | null;
  gpg_format: string | null;
  commit_gpgsign: boolean;
}
```

Add after the last `export const` line:

```typescript
export const getGitSigningStatus = () => invoke<GitSigningStatus>("get_git_signing_status");
export const configureGitSigning = () => invoke<void>("configure_git_signing");
```

- [x] **Step 2: Verify no TypeScript errors**

Run: `pnpm build`
Expected: Builds successfully

- [x] **Step 3: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat: add GitSigningStatus type and Tauri invoke wrappers"
```

---

### Task 7: Add "Git Signing" section to Settings page

**Files:**
- Modify: `src/pages/SettingsPage.tsx`

- [x] **Step 1: Add imports and state signals**

Add `getGitSigningStatus, configureGitSigning, type GitSigningStatus` to the existing import from `"../lib/tauri"`.

Add new signals after the existing signal declarations (after `let originalConfig`):

```typescript
const [gitSigningStatus, setGitSigningStatus] = createSignal<GitSigningStatus | null>(null);
const [configuring, setConfiguring] = createSignal(false);
```

- [x] **Step 2: Fetch git signing status on mount**

Inside the `onMount` callback, after the existing `getConfig` call, add:

```typescript
try {
  const status = await getGitSigningStatus();
  setGitSigningStatus(status);
} catch (e) {
  console.error("Failed to get git signing status:", e);
}
```

- [x] **Step 3: Add configure handler**

Add after `handlePresetChange`:

```typescript
const handleConfigureGitSigning = async () => {
  setConfiguring(true);
  setToast(null);
  try {
    await configureGitSigning();
    const status = await getGitSigningStatus();
    setGitSigningStatus(status);
    setToast({ message: "Git SSH signing configured successfully", type: "success" });
    setTimeout(() => setToast(null), 3000);
  } catch (e) {
    console.error("Failed to configure git signing:", e);
    setToast({ message: "Failed to configure git signing", type: "error" });
  } finally {
    setConfiguring(false);
  }
};
```

- [x] **Step 4: Add "Git Signing" section to the form**

In the `<form>` element, after the Network section's proxy input `<div>` (the one with `sm:col-span-6` wrapping the proxy field), and before the closing `</div>` of the grid, add:

```tsx
<div class="sm:col-span-6 pt-6">
  <h2 class="text-lg font-medium leading-6 text-gray-900">Git Signing</h2>
  <p class="mt-1 text-sm text-gray-500">
    Configure git to use bw-agent for SSH commit signing.
  </p>
</div>

<div class="sm:col-span-6">
  <Show
    when={gitSigningStatus()?.ssh_program != null && gitSigningStatus()?.gpg_format === "ssh" && gitSigningStatus()?.commit_gpgsign}
    fallback={
      <div class="rounded-md bg-yellow-50 p-4">
        <div class="flex">
          <div class="flex-shrink-0">
            <svg class="h-5 w-5 text-yellow-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z" />
            </svg>
          </div>
          <div class="ml-3 flex-1">
            <p class="text-sm text-yellow-700">
              Git SSH signing is not configured.
            </p>
            <div class="mt-3">
              <button
                type="button"
                onClick={handleConfigureGitSigning}
                disabled={configuring()}
                class="inline-flex items-center rounded-md bg-yellow-50 px-3 py-2 text-sm font-medium text-yellow-800 hover:bg-yellow-100 focus:outline-none focus:ring-2 focus:ring-yellow-600 focus:ring-offset-2 focus:ring-offset-yellow-50 disabled:opacity-50"
              >
                {configuring() ? "Configuring..." : "Configure Git SSH Signing"}
              </button>
            </div>
          </div>
        </div>
      </div>
    }
  >
    <div class="rounded-md bg-green-50 p-4">
      <div class="flex">
        <div class="flex-shrink-0">
          <svg class="h-5 w-5 text-green-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
          </svg>
        </div>
        <div class="ml-3">
          <p class="text-sm font-medium text-green-800">
            Git SSH signing is configured
          </p>
          <p class="mt-1 text-sm text-green-700">
            gpg.ssh.program: {gitSigningStatus()?.ssh_program}
          </p>
        </div>
      </div>
    </div>
  </Show>
</div>
```

- [x] **Step 5: Verify build**

Run: `pnpm build`
Expected: Builds successfully

- [x] **Step 6: Commit**

```bash
git add src/pages/SettingsPage.tsx
git commit -m "feat: add Git Signing section to settings page"
```

---

## Chunk 5: Integration test + cleanup

### Task 8: End-to-end verification

**Files:** None (testing only)

- [x] **Step 1: Run full workspace check**

Run: `cargo check --workspace`
Expected: All compiles

- [x] **Step 2: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass

- [x] **Step 3: Run clippy**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: No warnings

- [x] **Step 4: Test CLI help**

Run: `cargo run --package bw-agent -- --help`
Expected: Shows both default behavior and `git-sign` subcommand

Run: `cargo run --package bw-agent -- git-sign -Y sign`
Expected: Error about missing arguments, exit code 1

- [x] **Step 5: Verify Tauri commands via pnpm dev**

Run: `pnpm tauri dev`

In the running app:
1. Navigate to Settings page
2. Verify "Git Signing" section appears below "Network"
3. If not configured: verify yellow warning banner with "Configure Git SSH Signing" button
4. Click "Configure Git SSH Signing" button
5. Verify green success banner appears showing `gpg.ssh.program` path
6. Verify toast notification "Git SSH signing configured successfully"
7. Open terminal, run `git config --global --get gpg.ssh.program`
   Expected: output contains `bw-agent git-sign`
8. Run `git config --global --get gpg.format`
   Expected: `ssh`
9. Run `git config --global --get commit.gpgsign`
   Expected: `true`

- [x] **Step 6: Verify git_sign unit tests cover SSHSIG roundtrip**

Run: `cargo test --package bw-agent -- git_sign`
Expected: All git_sign tests pass, including `test_build_sshsig_roundtrip`, `test_sshsig_parse_valid_pem`, `test_parse_allowed_signers`, `test_key_type_string`

- [x] **Step 7: Final commit (if any fixes needed)**

```bash
git add -A
git commit -m "chore: fix lint issues from git-sign implementation"
```
