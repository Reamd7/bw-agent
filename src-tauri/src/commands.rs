use std::path::PathBuf;

use tauri::State;

use crate::{events, AppState};

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

    // Step 1: full_login only — return immediately on success.
    let session = match bw_core::api::full_login(&state.client, &email, &password).await {
        Ok(session) => session,
        Err(bw_core::error::Error::TwoFactorRequired { providers, .. }) => {
            return Ok(UnlockResult::TwoFactorRequired {
                providers: providers.into_iter().map(|provider| provider as u8).collect(),
            });
        }
        Err(error) => return Err(error.to_string()),
    };

    // Step 2: sync + unlock in background — don't block the UI.
    let app_handle = state.app_handle.clone();
    let client = state.client.clone();
    let agent_state = state.agent_state.clone();

    tauri::async_runtime::spawn(async move {
        let result = sync_and_unlock(client, agent_state, email, password, session).await;
        match result {
            Ok(()) => {
                let _ = events::emit_lock_state_changed(&app_handle, false);
                let _ = events::emit_vault_synced(&app_handle, events::VaultSyncedPayload {
                    success: true,
                    error: None,
                });
            }
            Err(error) => {
                log::error!("Background sync/unlock failed: {error}");
                let _ = events::emit_vault_synced(&app_handle, events::VaultSyncedPayload {
                    success: false,
                    error: Some(error),
                });
            }
        }
    });

    Ok(UnlockResult::Success)
}

/// Background sync + unlock. Runs after `full_login` succeeds.
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
                fingerprint: parsed_public_key.fingerprint(Default::default()).to_string(),
            })
        })
        .collect()
}

#[tauri::command]
pub async fn get_access_logs(
    limit: u32,
    state: State<'_, AppState>,
) -> Result<Vec<bw_agent::access_log::AccessLogEntry>, String> {
    state.access_log.query(limit).map_err(|error| error.to_string())
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
    log::info!("Lock mode updated to {:?} (cache_ttl={:?})", lock_mode, agent_state.cache_ttl);

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
