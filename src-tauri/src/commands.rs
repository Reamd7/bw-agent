use std::path::PathBuf;

use tauri::State;

use crate::{AppState, events};

#[derive(serde::Serialize)]
pub struct SshKeyInfo {
    pub name: String,
    pub key_type: String,
    pub fingerprint: String,
}

#[derive(serde::Serialize)]
pub enum UnlockResult {
    Success,
    TwoFactorRequired { providers: Vec<u8> },
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
    let (kdf, iterations, memory, parallelism) = state
        .client
        .prelogin(&email)
        .await
        .map_err(|error| error.to_string())?;
    let identity = bw_core::identity::Identity::new(
        &email,
        &password,
        kdf,
        iterations,
        memory,
        parallelism,
    )
    .map_err(|error| error.to_string())?;

    let login_result = state
        .client
        .login(&email, &device_id, &identity.master_password_hash, None, None)
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
    let client = state.client.clone();
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

    let login_result = state
        .client
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
            let client = state.client.clone();
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

/// Clear any pending 2FA login state. Call this everywhere the vault is locked.
fn clear_pending_two_factor(state: &AppState) {
    if let Ok(mut pending) = state.pending_two_factor.lock() {
        if pending.take().is_some() {
            log::debug!("Cleared pending two-factor login state");
        }
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

            Ok(SshKeyInfo {
                name,
                key_type: key_type_from_public_key(&public_key),
                fingerprint: parsed_public_key
                    .fingerprint(Default::default())
                    .to_string(),
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
pub async fn get_config() -> Result<bw_agent::config::Config, String> {
    Ok(bw_agent::config::Config::load())
}

#[tauri::command]
pub async fn save_config(config: bw_agent::config::Config) -> Result<(), String> {
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let json = serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?;
    std::fs::write(path, json).map_err(|error| error.to_string())
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
