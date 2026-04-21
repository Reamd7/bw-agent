use std::path::PathBuf;

use tauri::State;

use crate::{AppState, events};

#[derive(serde::Serialize)]
pub struct SshKeyInfo {
    pub name: String,
    pub key_type: String,
    pub fingerprint: String,
    pub match_patterns: Vec<String>,
}

#[derive(serde::Serialize)]
pub enum UnlockResult {
    Success,
    TwoFactorRequired { providers: Vec<u8> },
}

#[derive(serde::Serialize)]
pub struct GitSigningStatus {
    pub ssh_program: Option<String>,
    pub gpg_format: Option<String>,
    pub commit_gpgsign: bool,
    /// Whether gpg.ssh.program points to our bw-agent binary.
    pub program_correct: bool,
    /// Whether gpg.format == "ssh".
    pub format_correct: bool,
    /// Whether commit.gpgsign == true.
    pub signing_enabled: bool,
}

/// Spawn background sync + vault unlock after successful login.
/// Shared by `unlock` and `unlock_with_two_factor` to avoid duplication.
fn spawn_sync_and_unlock(
    app_handle: tauri::AppHandle,
    client: bw_core::api::Client,
    agent_state: std::sync::Arc<tokio::sync::Mutex<bw_agent::state::State>>,
    email: String,
    password: bw_core::locked::Password,
    session: bw_core::api::LoginSession,
) {
    tauri::async_runtime::spawn(async move {
        let result = sync_and_unlock(client, agent_state, email, password, session).await;
        match result {
            Ok(()) => {
                let _ = crate::events::emit_lock_state_changed(&app_handle, false);
                let _ = crate::events::emit_vault_synced(
                    &app_handle,
                    crate::events::VaultSyncedPayload {
                        success: true,
                        error: None,
                    },
                );
            }
            Err(error) => {
                log::error!("Background sync/unlock failed: {error}");
                let _ = crate::events::emit_vault_synced(
                    &app_handle,
                    crate::events::VaultSyncedPayload {
                        success: false,
                        error: Some(error),
                    },
                );
            }
        }
    });
}

#[tauri::command]
pub async fn unlock(password: String, state: State<'_, AppState>) -> Result<UnlockResult, String> {
    {
        let agent_state = state.agent_state.lock().await;
        if agent_state.is_unlocked() {
            return Ok(UnlockResult::Success);
        }
    }

    let email = state
        .agent_state
        .lock()
        .await
        .email
        .clone()
        .ok_or_else(|| "Email not configured".to_string())?;

    let password = locked_password(password);
    let device_id = bw_core::api::generate_device_id();
    let client = state.client.read().map_err(|e| e.to_string())?.clone();
    let (kdf, iterations, memory, parallelism) = client
        .prelogin(&email)
        .await
        .map_err(|error| error.to_string())?;
    let identity =
        bw_core::identity::Identity::new(&email, &password, kdf, iterations, memory, parallelism)
            .map_err(|error| error.to_string())?;

    let login_result = client
        .login(
            &email,
            &device_id,
            &identity.master_password_hash,
            None,
            None,
        )
        .await;

    let session = match login_result {
        Ok((access_token, refresh_token, protected_key)) => bw_core::api::LoginSession {
            access_token,
            refresh_token,
            kdf,
            iterations,
            memory,
            parallelism,
            protected_key,
            email: email.clone(),
            identity,
        },
        Err(bw_core::error::Error::TwoFactorRequired { providers, .. }) => {
            let pending_state = crate::PendingTwoFactor {
                device_id,
                email,
                password,
                identity,
                kdf,
                iterations,
                memory,
                parallelism,
            };

            match state.pending_two_factor.lock() {
                Ok(mut pending) => *pending = Some(pending_state),
                Err(e) => {
                    log::error!("Failed to save pending 2FA state: {e}");
                    return Err("Internal state error".to_string());
                }
            }

            return Ok(UnlockResult::TwoFactorRequired {
                providers: providers
                    .into_iter()
                    .map(|provider| provider as u8)
                    .collect(),
            });
        }
        Err(error) => return Err(error.to_string()),
    };

    let app_handle = state.app_handle.clone();
    let agent_state = state.agent_state.clone();
    spawn_sync_and_unlock(app_handle, client, agent_state, email, password, session);

    Ok(UnlockResult::Success)
}

#[tauri::command]
pub async fn unlock_with_two_factor(
    provider: u8,
    code: String,
    state: State<'_, AppState>,
) -> Result<UnlockResult, String> {
    {
        let agent_state = state.agent_state.lock().await;
        if agent_state.is_unlocked() {
            return Ok(UnlockResult::Success);
        }
    }

    // SAFETY: lock is held briefly (no .await while holding).
    let pending = match state.pending_two_factor.lock() {
        Ok(mut pending) => pending
            .take()
            .ok_or_else(|| "No pending two-factor login".to_string())?,
        Err(e) => {
            log::error!("Failed to access pending 2FA state: {e}");
            return Err("Pending two-factor state is poisoned".to_string());
        }
    };

    let two_factor_provider = bw_core::api::TwoFactorProviderType::try_from(u64::from(provider))
        .map_err(|error| error.to_string())?;

    let client = state.client.read().map_err(|e| e.to_string())?.clone();
    let login_result = client
        .login(
            &pending.email,
            &pending.device_id,
            &pending.identity.master_password_hash,
            Some(&code),
            Some(two_factor_provider),
        )
        .await;

    match login_result {
        Ok((access_token, refresh_token, protected_key)) => {
            let email_for_session = pending.email.clone();
            let session = bw_core::api::LoginSession {
                access_token,
                refresh_token,
                kdf: pending.kdf,
                iterations: pending.iterations,
                memory: pending.memory,
                parallelism: pending.parallelism,
                protected_key,
                email: email_for_session,
                identity: pending.identity,
            };

            let app_handle = state.app_handle.clone();
            let agent_state = state.agent_state.clone();
            spawn_sync_and_unlock(
                app_handle,
                client,
                agent_state,
                pending.email,
                pending.password,
                session,
            );

            Ok(UnlockResult::Success)
        }
        Err(ref err) => {
            let error_msg = if matches!(err, bw_core::error::Error::TwoFactorRequired { .. }) {
                "Verification code is incorrect. Please try again.".to_string()
            } else {
                err.to_string()
            };

            match state.pending_two_factor.lock() {
                Ok(mut pending_state) => *pending_state = Some(pending),
                Err(e) => {
                    log::error!("Failed to restore pending 2FA state: {e}");
                }
            }

            Err(error_msg)
        }
    }
}

#[tauri::command]
pub async fn get_git_signing_status() -> Result<GitSigningStatus, String> {
    let ssh_program = git_config_get("gpg.ssh.program").ok();
    let gpg_format = git_config_get("gpg.format").ok();
    let commit_gpgsign = git_config_get("commit.gpgsign")
        .map(|v| v == "true")
        .unwrap_or(false);

    let program_correct = {
        let expected = ensure_git_sign_binary();
        match (&ssh_program, expected) {
            (Some(program), Ok(expected_path)) => *program == expected_path.display().to_string(),
            _ => false,
        }
    };
    let format_correct = gpg_format.as_deref() == Some("ssh");
    let signing_enabled = commit_gpgsign;

    Ok(GitSigningStatus {
        ssh_program,
        gpg_format,
        commit_gpgsign,
        program_correct,
        format_correct,
        signing_enabled,
    })
}

#[tauri::command]
pub async fn configure_git_signing() -> Result<(), String> {
    let signing_bin = ensure_git_sign_binary()?;

    git_config_set("gpg.ssh.program", &signing_bin.display().to_string())?;
    git_config_set("gpg.format", "ssh")?;
    git_config_set("commit.gpgsign", "true")?;

    Ok(())
}

/// Clear any pending 2FA login state. Call this everywhere the vault is locked.
fn clear_pending_two_factor(state: &AppState) {
    if let Ok(mut pending) = state.pending_two_factor.lock() {
        if pending.take().is_some() {
            log::debug!("Cleared pending two-factor login state");
        }
    }
}

fn git_config_get(key: &str) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(["config", "--global", "--get", key])
        .output()
        .map_err(|e| format!("Failed to run git: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(format!("git config --get {key} failed"))
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

/// Find the bw-agent-git-sign(.exe) sidecar sitting next to the desktop binary.
/// This is the path written to gpg.ssh.program.
fn ensure_git_sign_binary() -> Result<PathBuf, String> {
    let current_exe =
        std::env::current_exe().map_err(|e| format!("Cannot determine executable path: {e}"))?;

    let current_dir = current_exe.parent().ok_or_else(|| {
        format!(
            "Executable has no parent directory: {}",
            current_exe.display()
        )
    })?;

    let sidecar_name = if cfg!(windows) {
        "bw-agent-git-sign.exe"
    } else {
        "bw-agent-git-sign"
    };

    let sidecar_path = current_dir.join(sidecar_name);

    if sidecar_path.exists() {
        Ok(sidecar_path)
    } else {
        Err(format!(
            "Git signing sidecar not found: {}",
            sidecar_path.display()
        ))
    }
}

/// Background sync + unlock. Runs after login succeeds.
async fn sync_and_unlock(
    client: bw_core::api::Client,
    agent_state: std::sync::Arc<tokio::sync::Mutex<bw_agent::state::State>>,
    email: String,
    password: bw_core::locked::Password,
    session: bw_core::api::LoginSession,
) -> Result<(), String> {
    let sync_data = bw_core::api::sync_vault(&client, &session.access_token)
        .await
        .map_err(|e| e.to_string())?;

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
    )
    .map_err(|e| e.to_string())?;

    let mut state = agent_state.lock().await;
    state.access_token = Some(session.access_token);
    state.refresh_token = Some(session.refresh_token);
    state.email = Some(email);
    state.kdf = Some(session.kdf);
    state.iterations = Some(session.iterations);
    state.memory = session.memory;
    state.parallelism = session.parallelism;
    state.protected_key = Some(session.protected_key);
    state.protected_private_key = Some(sync_data.protected_private_key);
    state.protected_org_keys = sync_data.org_keys;
    state.entries = sync_data.entries;
    state.set_unlocked(keys, org_keys);

    Ok(())
}

#[tauri::command]
pub async fn submit_password(
    password: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sender = take_password_sender(&state)?;
    sender
        .send(password)
        .map_err(|_| "Password request is no longer pending".to_string())
}

#[tauri::command]
pub async fn submit_two_factor(
    provider: u8,
    code: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let sender = take_two_factor_sender(&state)?;
    sender
        .send(Some((provider, code)))
        .map_err(|_| "Two-factor request is no longer pending".to_string())
}

#[tauri::command]
pub async fn list_keys(state: State<'_, AppState>) -> Result<Vec<SshKeyInfo>, String> {
    let agent_state = state.agent_state.lock().await;
    if !agent_state.is_unlocked() {
        return Err("Vault is locked".to_string());
    }

    agent_state
        .entries
        .iter()
        .filter_map(|entry| match &entry.data {
            bw_core::db::EntryData::SshKey {
                public_key: Some(public_key),
                ..
            } => Some((entry, public_key)),
            _ => None,
        })
        .map(|(entry, encrypted_public_key)| {
            let name = bw_agent::auth::decrypt_cipher(
                &agent_state,
                &entry.name,
                entry.key.as_deref(),
                entry.org_id.as_deref(),
            )
            .unwrap_or_else(|_| entry.name.clone());

            let public_key = bw_agent::auth::decrypt_cipher(
                &agent_state,
                encrypted_public_key,
                entry.key.as_deref(),
                entry.org_id.as_deref(),
            )
            .map_err(|error| error.to_string())?;
            let parsed_public_key = ssh_agent_lib::ssh_key::PublicKey::from_openssh(&public_key)
                .map_err(|error| error.to_string())?;

            let match_patterns: Vec<String> = entry
                .fields
                .iter()
                .filter_map(|f| {
                    let field_name = bw_agent::auth::decrypt_cipher(
                        &agent_state,
                        f.name.as_deref()?,
                        entry.key.as_deref(),
                        entry.org_id.as_deref(),
                    )
                    .ok()?;
                    if field_name == "gh-match" {
                        let field_value = f.value.as_deref()?;
                        bw_agent::auth::decrypt_cipher(
                            &agent_state,
                            field_value,
                            entry.key.as_deref(),
                            entry.org_id.as_deref(),
                        )
                        .ok()
                    } else {
                        None
                    }
                })
                .collect();

            Ok(SshKeyInfo {
                name,
                key_type: key_type_from_public_key(&public_key),
                fingerprint: parsed_public_key
                    .fingerprint(Default::default())
                    .to_string(),
                match_patterns,
            })
        })
        .collect()
}

#[tauri::command]
pub async fn get_access_logs(
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<bw_agent::access_log::AccessLogEntry>, String> {
    state
        .access_log
        .query(limit)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn approve_request(
    request_id: String,
    approved: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.approval_queue.respond(&request_id, approved).await;
    Ok(())
}

#[tauri::command]
pub async fn get_pending_approvals(
    state: State<'_, AppState>,
) -> Result<Vec<bw_agent::ApprovalRequest>, String> {
    Ok(state.approval_queue.pending().await)
}

#[tauri::command]
pub async fn lock_vault(state: State<'_, AppState>) -> Result<(), String> {
    state.agent_state.lock().await.clear();
    clear_pending_two_factor(&state);
    events::emit_lock_state_changed(&state.app_handle, true).map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn manual_sync(state: State<'_, AppState>) -> Result<(), String> {
    let access_token = {
        let agent_state = state.agent_state.lock().await;
        if !agent_state.is_unlocked() {
            return Err("Vault is locked".to_string());
        }
        agent_state.access_token.clone().ok_or("No access token")?
    };

    let sync_data = {
        let client = state.client.read().map_err(|e| e.to_string())?.clone();
        bw_core::api::sync_vault(&client, &access_token)
            .await
            .map_err(|e| e.to_string())?
    };

    {
        let mut agent_state = state.agent_state.lock().await;
        if agent_state.is_unlocked() {
            agent_state.entries = sync_data.entries;
            agent_state.protected_org_keys = sync_data.org_keys;
        }
    }

    events::emit_vault_synced(
        &state.app_handle,
        events::VaultSyncedPayload {
            success: true,
            error: None,
        },
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn get_config() -> Result<bw_agent::config::Config, String> {
    Ok(bw_agent::config::Config::load())
}

#[tauri::command]
pub async fn save_config(
    config: bw_agent::config::Config,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let json = serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| error.to_string())?;

    // Keep in-memory state in sync so subsequent commands (e.g. unlock)
    // can use the new email / base_url without restarting the app.
    if let Some(ref email) = config.email {
        state.agent_state.lock().await.email = Some(email.clone());
    }

    // Update the HTTP client's URLs so requests go to the right server.
    {
        let mut client = state.client.write().map_err(|e| e.to_string())?;
        client.update(
            &config.api_url(),
            &config.identity_url(),
            config.proxy.as_deref(),
        );
    }

    Ok(())
}

/// Hot-reload lock mode at runtime without restarting the app.
/// Updates the in-memory State cache_ttl and system event listeners.
#[tauri::command]
pub async fn update_lock_mode(
    lock_mode: bw_agent::config::LockMode,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let mut agent_state = state.agent_state.lock().await;
    agent_state.cache_ttl = lock_mode.cache_ttl();
    log::info!(
        "Lock mode updated to {:?} (cache_ttl={:?})",
        lock_mode,
        agent_state.cache_ttl
    );

    // Also update system event listeners (idle threshold, active mode).
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    crate::system_events::set_lock_mode(&lock_mode);

    Ok(())
}

fn locked_password(password: String) -> bw_core::locked::Password {
    let mut password_vec = bw_core::locked::Vec::new();
    password_vec.extend(password.bytes());
    bw_core::locked::Password::new(password_vec)
}

fn take_password_sender(
    state: &State<'_, AppState>,
) -> Result<tokio::sync::oneshot::Sender<Option<String>>, String> {
    state
        .password_tx
        .lock()
        .map_err(|_| "Password prompt state is poisoned".to_string())?
        .take()
        .ok_or_else(|| "No pending password request".to_string())
}

fn take_two_factor_sender(
    state: &State<'_, AppState>,
) -> Result<tokio::sync::oneshot::Sender<Option<(u8, String)>>, String> {
    state
        .two_factor_tx
        .lock()
        .map_err(|_| "Two-factor prompt state is poisoned".to_string())?
        .take()
        .ok_or_else(|| "No pending two-factor request".to_string())
}

fn key_type_from_public_key(public_key: &str) -> String {
    match public_key.split_whitespace().next().unwrap_or_default() {
        "ssh-ed25519" => "ed25519".to_string(),
        "ssh-rsa" | "rsa-sha2-256" | "rsa-sha2-512" => "rsa".to_string(),
        other => other.to_string(),
    }
}

fn config_file_path() -> PathBuf {
    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata).join("bw-agent").join("config.json")
    }

    #[cfg(unix)]
    {
        let config_dir = std::env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            format!("{home}/.config")
        });
        PathBuf::from(config_dir)
            .join("bw-agent")
            .join("config.json")
    }
}
