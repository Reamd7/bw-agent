use std::path::PathBuf;

use tauri::State;

use crate::{AppState, events};

#[derive(serde::Serialize, Clone)]
pub struct CustomFieldInfo {
    pub name: String,
    pub value: String,
    pub field_type: u16,
}

#[derive(serde::Deserialize)]
pub struct CustomFieldInput {
    pub name: String,
    pub value: String,
    pub field_type: u16,
}

#[derive(serde::Serialize)]
pub struct SshKeyInfo {
    pub entry_id: String,
    pub name: String,
    pub key_type: String,
    pub fingerprint: String,
    pub public_key: String,
    pub match_patterns: Vec<String>,
    pub custom_fields: Vec<CustomFieldInfo>,
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
                if let Err(e) = crate::events::emit_lock_state_changed(&app_handle, false) {
                    log::warn!("Failed to emit lock-state-changed: {e}");
                }
                if let Err(e) = crate::events::emit_vault_synced(
                    &app_handle,
                    crate::events::VaultSyncedPayload {
                        success: true,
                        error: None,
                    },
                ) {
                    log::warn!("Failed to emit vault-synced: {e}");
                }
            }
            Err(error) => {
                log::error!("Background sync/unlock failed: {error}");
                if let Err(e) = crate::events::emit_vault_synced(
                    &app_handle,
                    crate::events::VaultSyncedPayload {
                        success: false,
                        error: Some(error),
                    },
                ) {
                    log::warn!("Failed to emit vault-synced: {e}");
                }
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
    // Reuse existing device_id if we have one (vaultwarden requires known devices for auth-request).
    // Otherwise generate a new one and save it after successful login.
    let existing_config = bw_agent::config::Config::load();
    let device_id = existing_config.device_id.clone().unwrap_or_else(|| {
        let new_id = bw_core::api::generate_device_id();
        log::info!("Generated new device_id: {new_id}");
        new_id
    });
    log::info!(
        "Using device_id: {device_id} (saved: {})",
        existing_config.device_id.is_some()
    );
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
            false,
            None,
        )
        .await;

    let session = match login_result {
        Ok(resp) => {
            // Save remember token if server returned one
            if let Some(token) = &resp.two_factor_token {
                save_remember_token(&state, token);
            }
            // Persist device_id so auth-request can reuse it
            eprintln!("[bw-agent] unlock OK, saving device_id: {device_id}");
            save_device_id(&state, &device_id);
            bw_core::api::LoginSession {
                access_token: resp.access_token,
                refresh_token: resp.refresh_token,
                kdf,
                iterations,
                memory,
                parallelism,
                protected_key: resp.key,
                email: email.clone(),
                identity,
            }
        }
        Err(bw_core::error::Error::TwoFactorRequired { providers, .. }) => {
            // Try stored remember token before asking user
            let remember_token = load_remember_token(&state);
            if let Some(ref token) = remember_token {
                let remember_login = client
                    .login(
                        &email,
                        &device_id,
                        &identity.master_password_hash,
                        Some(token),
                        Some(bw_core::api::TwoFactorProviderType::Remember),
                        false,
                        None,
                    )
                    .await;

                match remember_login {
                    Ok(resp) => {
                        // Remember token worked — save new one if rotated
                        if let Some(new_token) = &resp.two_factor_token {
                            save_remember_token(&state, new_token);
                        }
                        save_device_id(&state, &device_id);
                        let session = bw_core::api::LoginSession {
                            access_token: resp.access_token,
                            refresh_token: resp.refresh_token,
                            kdf,
                            iterations,
                            memory,
                            parallelism,
                            protected_key: resp.key,
                            email: email.clone(),
                            identity,
                        };
                        let app_handle = state.app_handle.clone();
                        let agent_state = state.agent_state.clone();
                        spawn_sync_and_unlock(
                            app_handle,
                            client,
                            agent_state,
                            email,
                            password,
                            session,
                        );
                        return Ok(UnlockResult::Success);
                    }
                    Err(_) => {
                        // Remember token rejected — clear it, fall through to normal 2FA prompt
                        clear_remember_token(&state);
                    }
                }
            }

            let pending_state = crate::PendingTwoFactor {
                device_id,
                email,
                password,
                identity,
                kdf,
                iterations,
                memory,
                parallelism,
                remember_token_rejected: remember_token.is_some(),
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
    remember: bool,
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
            remember,
            None,
        )
        .await;

    match login_result {
        Ok(resp) => {
            // Save remember token if server returned one (user asked to remember)
            if remember {
                if let Some(token) = &resp.two_factor_token {
                    save_remember_token(&state, token);
                }
            }

            // Persist device_id so auth-request can reuse it
            save_device_id(&state, &pending.device_id);

            let email_for_session = pending.email.clone();
            let session = bw_core::api::LoginSession {
                access_token: resp.access_token,
                refresh_token: resp.refresh_token,
                kdf: pending.kdf,
                iterations: pending.iterations,
                memory: pending.memory,
                parallelism: pending.parallelism,
                protected_key: resp.key,
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

#[tauri::command]
pub async fn get_git_sign_program_path() -> Result<String, String> {
    let path = ensure_git_sign_binary()?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub async fn update_key_fields(
    entry_id: String,
    fields: Vec<CustomFieldInput>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (access_token, entry_key, org_id) = {
        let agent_state = state.agent_state.lock().await;
        if !agent_state.is_unlocked() {
            return Err("Vault is locked".to_string());
        }
        let access_token = agent_state.access_token.clone().ok_or("No access token")?;
        let entry = agent_state
            .entries
            .iter()
            .find(|e| e.id == entry_id)
            .ok_or_else(|| format!("Entry not found: {entry_id}"))?;
        (access_token, entry.key.clone(), entry.org_id.clone())
    };

    let encrypted_fields: Vec<serde_json::Value> = {
        let agent_state = state.agent_state.lock().await;
        fields
            .iter()
            .map(|f| {
                let enc_name = bw_agent::auth::encrypt_cipher(
                    &agent_state,
                    &f.name,
                    entry_key.as_deref(),
                    org_id.as_deref(),
                )
                .map_err(|e| format!("Failed to encrypt field name: {e}"))?;
                let enc_value = bw_agent::auth::encrypt_cipher(
                    &agent_state,
                    &f.value,
                    entry_key.as_deref(),
                    org_id.as_deref(),
                )
                .map_err(|e| format!("Failed to encrypt field value: {e}"))?;
                Ok(serde_json::json!({
                    "type": f.field_type,
                    "name": enc_name,
                    "value": enc_value,
                    "linkedId": null,
                }))
            })
            .collect::<Result<Vec<_>, String>>()?
    };

    let client = state.client.read().map_err(|e| e.to_string())?.clone();
    let cipher_response = client
        .get_cipher(&access_token, &entry_id)
        .await
        .map_err(|e| format!("Failed to get cipher: {e}"))?;

    let put_body = build_cipher_update_body(&cipher_response, encrypted_fields.clone());
    client
        .update_cipher(&access_token, &entry_id, &put_body)
        .await
        .map_err(|e| format!("Failed to update cipher: {e}"))?;

    {
        let mut agent_state = state.agent_state.lock().await;
        if let Some(entry) = agent_state.entries.iter_mut().find(|e| e.id == entry_id) {
            entry.fields = fields
                .iter()
                .zip(encrypted_fields.iter())
                .map(|(input, enc_json)| bw_core::db::Field {
                    ty: match input.field_type {
                        0 => Some(bw_core::api::FieldType::Text),
                        1 => Some(bw_core::api::FieldType::Hidden),
                        2 => Some(bw_core::api::FieldType::Boolean),
                        3 => Some(bw_core::api::FieldType::Linked),
                        _ => Some(bw_core::api::FieldType::Text),
                    },
                    name: enc_json
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    value: enc_json
                        .get("value")
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    linked_id: None,
                })
                .collect();
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn approve_request_with_session(
    request_id: String,
    duration_secs: u64,
    scope_type: String,
    scope_exe_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    use bw_agent::session_store::SessionScope;
    use std::time::Duration;

    // Look up the pending request to get key_fingerprint
    let request = state
        .approval_queue
        .get_request(&request_id)
        .await
        .ok_or_else(|| "Request not found or already responded to".to_string())?;

    // Build the session scope
    let scope = match scope_type.as_str() {
        "any_process" => SessionScope::AnyProcess,
        "executable" => {
            let exe_path = scope_exe_path
                .ok_or_else(|| "exe_path required for executable scope".to_string())?;
            let exe_hash = state
                .session_store
                .hash_file_cached(&exe_path)
                .map_err(|e| format!("Failed to hash executable: {e}"))?;
            SessionScope::Executable { exe_path, exe_hash }
        }
        other => return Err(format!("Invalid scope_type: {other}")),
    };

    // Create the session
    let session_id = state.session_store.create_session(
        &request.key_fingerprint,
        scope,
        Duration::from_secs(duration_secs),
    );
    log::info!(
        "Created approval session {session_id} for key {}",
        request.key_fingerprint
    );

    // Respond to the approval (approve = true)
    state.approval_queue.respond(&request_id, true).await;
    Ok(())
}

#[tauri::command]
pub async fn list_active_sessions(
    state: State<'_, AppState>,
) -> Result<Vec<bw_agent::session_store::SessionInfo>, String> {
    Ok(state.session_store.list_active())
}

#[tauri::command]
pub async fn revoke_session(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    Ok(state.session_store.revoke_session(&session_id))
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

            let custom_fields: Vec<CustomFieldInfo> = entry
                .fields
                .iter()
                .filter_map(|f| {
                    let field_name = bw_agent::auth::decrypt_cipher(
                        &agent_state,
                        f.name.as_deref()?,
                        entry.key.as_deref(),
                        entry.org_id.as_deref(),
                    );
                    if let Err(e) = &field_name {
                        log::debug!("Failed to decrypt field name for entry {}: {e}", entry.id);
                    }
                    let field_name = field_name.ok()?;

                    let field_value = f
                        .value
                        .as_deref()
                        .and_then(|v| {
                            let decrypted = bw_agent::auth::decrypt_cipher(
                                &agent_state,
                                v,
                                entry.key.as_deref(),
                                entry.org_id.as_deref(),
                            );
                            if let Err(e) = &decrypted {
                                log::debug!(
                                    "Failed to decrypt field value for entry {}: {e}",
                                    entry.id
                                );
                            }
                            decrypted.ok()
                        })
                        .unwrap_or_default();

                    Some(CustomFieldInfo {
                        name: field_name,
                        value: field_value,
                        field_type: f.ty.map(|t| t as u16).unwrap_or(0),
                    })
                })
                .collect();

            let match_patterns: Vec<String> = custom_fields
                .iter()
                .filter(|f| f.name == "gh-match")
                .map(|f| f.value.clone())
                .collect();

            Ok(SshKeyInfo {
                entry_id: entry.id.clone(),
                name,
                key_type: key_type_from_public_key(&public_key),
                fingerprint: parsed_public_key
                    .fingerprint(Default::default())
                    .to_string(),
                public_key: public_key.trim().to_string(),
                match_patterns,
                custom_fields,
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
    state.session_store.revoke_all();
    clear_pending_two_factor(&state);
    events::emit_lock_state_changed(&state.app_handle, true).map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn manual_sync(state: State<'_, AppState>) -> Result<(), String> {
    let (access_token, refresh_token) = {
        let agent_state = state.agent_state.lock().await;
        if !agent_state.is_unlocked() {
            return Err("Vault is locked".to_string());
        }
        let at = agent_state.access_token.clone().ok_or("No access token")?;
        (at, agent_state.refresh_token.clone())
    };

    // Try to refresh token before sync
    let access_token = match refresh_token {
        Some(rt) => {
            let client = state.client.read().map_err(|e| e.to_string())?.clone();
            match client.exchange_refresh_token(&rt).await {
                Ok(new_token) => {
                    state.agent_state.lock().await.access_token = Some(new_token.clone());
                    new_token
                }
                Err(_) => access_token,
            }
        }
        None => access_token,
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

fn json_get(obj: &serde_json::Value, pascal: &str, camel: &str) -> serde_json::Value {
    obj.get(pascal)
        .or_else(|| obj.get(camel))
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

fn build_cipher_update_body(
    cipher_response: &serde_json::Value,
    encrypted_fields: Vec<serde_json::Value>,
) -> serde_json::Value {
    let ssh_key_raw = json_get(cipher_response, "SshKey", "sshKey");
    let ssh_key = if ssh_key_raw.is_null() {
        serde_json::Value::Null
    } else {
        serde_json::json!({
            "privateKey": json_get(&ssh_key_raw, "PrivateKey", "privateKey"),
            "publicKey": json_get(&ssh_key_raw, "PublicKey", "publicKey"),
            "keyFingerprint": json_get(&ssh_key_raw, "KeyFingerprint", "keyFingerprint")
        })
    };

    serde_json::json!({
        "type": json_get(cipher_response, "Type", "type"),
        "name": json_get(cipher_response, "Name", "name"),
        "notes": json_get(cipher_response, "Notes", "notes"),
        "organizationId": json_get(cipher_response, "OrganizationId", "organizationId"),
        "folderId": json_get(cipher_response, "FolderId", "folderId"),
        "login": json_get(cipher_response, "Login", "login"),
        "card": json_get(cipher_response, "Card", "card"),
        "identity": json_get(cipher_response, "Identity", "identity"),
        "secureNote": json_get(cipher_response, "SecureNote", "secureNote"),
        "sshKey": ssh_key,
        "fields": encrypted_fields,
        "key": json_get(cipher_response, "Key", "key"),
        "reprompt": json_get(cipher_response, "Reprompt", "reprompt"),
    })
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

// ── Remember Token helpers ────────────────────────────────────────

/// Load the stored 2FA remember token from config.
fn load_remember_token(_state: &AppState) -> Option<String> {
    let config = bw_agent::config::Config::load();
    config.two_factor_remember_token.clone()
}

/// Persist a 2FA remember token to config file.
fn save_remember_token(_state: &AppState, token: &str) {
    let mut config = bw_agent::config::Config::load();
    config.two_factor_remember_token = Some(token.to_string());
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(&config) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!("Failed to save remember token to config: {e}");
            }
        }
        Err(e) => log::warn!("Failed to serialize config for remember token: {e}"),
    }
}

/// Remove the stored 2FA remember token.
fn clear_remember_token(_state: &AppState) {
    let mut config = bw_agent::config::Config::load();
    config.two_factor_remember_token = None;
    let path = config_file_path();
    match serde_json::to_string_pretty(&config) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!("Failed to clear remember token from config: {e}");
            }
        }
        Err(e) => log::warn!("Failed to serialize config for remember token clear: {e}"),
    }
}

/// Check whether a 2FA remember token is stored.
#[tauri::command]
pub async fn has_two_factor_remember(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(load_remember_token(&state).is_some())
}

/// Revoke the stored 2FA remember token.
/// Clears the local token. The server-side token is a JWT that will
/// naturally expire. If the user logs in again without the remember token,
/// the server will invalidate the old one.
#[tauri::command]
pub async fn revoke_two_factor_remember(state: State<'_, AppState>) -> Result<(), String> {
    if load_remember_token(&state).is_some() {
        clear_remember_token(&state);
        log::info!("2FA remember token revoked");
    }
    Ok(())
}

/// Check whether a device_id has been registered (required for device login).
#[tauri::command]
pub async fn has_registered_device() -> Result<bool, String> {
    Ok(bw_agent::config::Config::load().device_id.is_some())
}

/// Persist device_id to config file so auth-request can reuse it.
/// vaultwarden requires the device to be registered before accepting auth-request creation.
fn save_device_id(_state: &AppState, device_id: &str) {
    log::info!("Saving device_id to config: {device_id}");
    let mut config = bw_agent::config::Config::load();
    config.device_id = Some(device_id.to_string());
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(&config) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!("Failed to save device_id to config: {e}");
            }
        }
        Err(e) => log::warn!("Failed to serialize config for device_id: {e}"),
    }
}

// ── Auth Request (Login with Device) ──────────────────────────────

/// Result returned when a device auth request is created.
#[derive(serde::Serialize)]
pub struct AuthRequestResult {
    /// The auth request ID (used for polling).
    pub request_id: String,
    /// The fingerprint phrase for the user to verify on the approving device.
    pub fingerprint: String,
}

/// Create an anonymous auth request for device login.
/// This is step 1 of the "Login with Device" flow — it posts a new auth request
/// and returns the request ID + fingerprint for the frontend to display.
#[tauri::command]
pub async fn create_auth_request(state: State<'_, AppState>) -> Result<AuthRequestResult, String> {
    use base64::Engine as _;
    use rand::Rng as _;
    use rsa::pkcs8::EncodePrivateKey as _;
    use rsa::pkcs8::EncodePublicKey as _;

    let email = state
        .agent_state
        .lock()
        .await
        .email
        .clone()
        .ok_or_else(|| "Email not configured".to_string())?;

    let client = state.client.read().map_err(|e| e.to_string())?.clone();
    // Use the device_id that was registered during the last successful login.
    // vaultwarden requires the device to already exist before accepting auth-request creation.
    let device_id = bw_agent::config::Config::load()
        .device_id
        .ok_or_else(|| "No registered device. Please log in with password first.".to_string())?;
    log::info!("create_auth_request using device_id: {device_id}");

    // Generate an RSA keypair for this auth request
    let mut rng = rand::rngs::OsRng;
    let private_key = rsa::RsaPrivateKey::new(&mut rng, 2048)
        .map_err(|e| format!("Failed to generate RSA key: {e}"))?;
    let public_key = private_key.to_public_key();

    // Encode public key as SPKI DER (SubjectPublicKeyInfo) and then base64.
    // This matches the official Bitwarden SDK which uses to_public_key_der().
    // The approving client parses it with RsaPublicKey::from_public_key_der()
    // which expects SPKI format, NOT PKCS1.
    let spki_document = public_key
        .to_public_key_der()
        .map_err(|e| format!("Failed to encode public key: {e}"))?;
    let spki_der = spki_document.as_bytes();
    let public_key_b64 = base64::engine::general_purpose::STANDARD.encode(spki_der);

    // Generate access code: 25-char random alphanumeric (A-Za-z0-9).
    // Bitwarden official clients use password generator with these defaults.
    let access_code: String = rand::rngs::OsRng
        .sample_iter(rand::distributions::Alphanumeric)
        .take(25)
        .map(char::from)
        .collect();

    let req = bw_core::api::AuthRequestCreateReq {
        email: email.clone(),
        public_key: public_key_b64,
        device_identifier: device_id.clone(),
        access_code: access_code.clone(),
        request_type: 0, // AuthenticateAndUnlock = 0
    };

    eprintln!("[bw-agent] create_auth_request: email={email}, device_id={device_id}");
    let res = client.create_auth_request(&req).await.map_err(|e| {
        eprintln!("[bw-agent] create_auth_request FAILED: {e}");
        e.to_string()
    })?;

    eprintln!("[bw-agent] create_auth_request OK: id={}", res.id);

    // Compute fingerprint phrase for the user to verify
    let fingerprint = bw_core::fingerprint::fingerprint_phrase(&email, spki_der)
        .map_err(|e| format!("Failed to compute fingerprint: {e}"))?;

    // Store pending auth request state (encode private key as PKCS8 PEM)
    let private_key_der = private_key
        .to_pkcs8_der()
        .map_err(|e| format!("Failed to encode private key: {e}"))?;
    let private_key_pem = pem::encode(&pem::Pem::new("PRIVATE KEY", private_key_der.as_bytes()));

    let pending = crate::PendingAuthRequest {
        request_id: res.id.clone(),
        access_code,
        private_key_pem,
        email: email.clone(),
    };

    match state.pending_auth_request.lock() {
        Ok(mut slot) => *slot = Some(pending),
        Err(e) => {
            log::error!("Failed to save pending auth request state: {e}");
            return Err("Internal state error".to_string());
        }
    }

    Ok(AuthRequestResult {
        request_id: res.id,
        fingerprint,
    })
}

/// Poll for an auth request response (device approval).
/// Returns:
///   - `Ok(None)` if not yet approved (frontend should keep polling)
///   - `Ok(Some(access_token))` if approved and vault unlocked
///   - `Err(msg)` on failure
#[derive(serde::Serialize)]
pub struct PollAuthRequestResult {
    pub approved: bool,
    pub fingerprint_validated: bool,
    /// If the token exchange requires 2FA, this will contain the list of
    /// available 2FA providers.
    pub two_factor_required: Option<Vec<u8>>,
}

#[tauri::command]
pub async fn poll_auth_request(
    state: State<'_, AppState>,
) -> Result<PollAuthRequestResult, String> {
    let pending = {
        let guard = state.pending_auth_request.lock();
        match guard {
            Ok(slot) => slot.clone(),
            Err(e) => {
                log::error!("Failed to access pending auth request state: {e}");
                return Err("Internal state error".to_string());
            }
        }
    };

    let pending = match pending {
        Some(p) => p,
        None => return Err("No pending auth request".to_string()),
    };

    let client = state.client.read().map_err(|e| e.to_string())?.clone();

    eprintln!(
        "[bw-agent] poll_auth_request: id={}, code={}",
        pending.request_id, pending.access_code
    );
    let response = client
        .poll_auth_response(&pending.request_id, &pending.access_code)
        .await
        .map_err(|e| {
            eprintln!("[bw-agent] poll_auth_response FAILED: {e}");
            e.to_string()
        })?;

    match response {
        Some(data) if data.request_approved => {
            // Approved! Now exchange the auth request for tokens.
            // Use the same device_id that was used to create the auth request.
            let device_id = bw_agent::config::Config::load()
                .device_id
                .ok_or_else(|| "No registered device".to_string())?;
            eprintln!(
                "[bw-agent] Exchanging auth request: id={}, device_id={}",
                pending.request_id, device_id
            );
            let resp = client
                .exchange_auth_request(
                    &pending.request_id,
                    &device_id,
                    &pending.email,
                    &pending.access_code,
                    None,  // no 2FA token on first attempt
                    None,  // no 2FA provider on first attempt
                    false, // no remember on first attempt
                )
                .await;

            match resp {
                Ok(resp) => {
                    // Decrypt the user key from the auth response
                    let key_encrypted = data
                        .key
                        .ok_or_else(|| "Auth request approved but no key returned".to_string())?;

                    let user_key_bytes = decrypt_auth_request_key(
                        &key_encrypted,
                        &pending.private_key_pem,
                        &pending.access_code,
                    )?;

                    // Sync vault data using the access token
                    let sync_data = bw_core::api::sync_vault(&client, &resp.access_token)
                        .await
                        .map_err(|e| e.to_string())?;

                    // Unlock vault using the decrypted user key
                    let (keys, org_keys) = bw_core::api::unlock_vault_with_user_key(
                        &user_key_bytes,
                        &sync_data.protected_private_key,
                        &sync_data.org_keys,
                    )
                    .map_err(|e| e.to_string())?;

                    // Store everything in agent state
                    let mut agent_state = state.agent_state.lock().await;
                    agent_state.access_token = Some(resp.access_token);
                    agent_state.refresh_token = Some(resp.refresh_token);
                    agent_state.email = Some(pending.email.clone());
                    agent_state.protected_key = Some(sync_data.protected_key);
                    agent_state.protected_private_key = Some(sync_data.protected_private_key);
                    agent_state.protected_org_keys = sync_data.org_keys;
                    agent_state.entries = sync_data.entries;
                    agent_state.set_unlocked(keys, org_keys);

                    // Save 2FA remember token if provided
                    if let Some(ref token) = resp.two_factor_token {
                        save_remember_token(&state, token);
                    }

                    // Clear pending auth request
                    if let Ok(mut slot) = state.pending_auth_request.lock() {
                        *slot = None;
                    }

                    // Emit events
                    if let Err(e) = crate::events::emit_lock_state_changed(&state.app_handle, false)
                    {
                        log::warn!("Failed to emit lock-state-changed: {e}");
                    }

                    Ok(PollAuthRequestResult {
                        approved: true,
                        fingerprint_validated: true,
                        two_factor_required: None,
                    })
                }
                Err(bw_core::error::Error::TwoFactorRequired { providers, .. }) => {
                    // Token exchange requires 2FA.
                    // First try the stored remember token to auto-skip 2FA.
                    let remember_token = load_remember_token(&state);
                    if let Some(ref token) = remember_token {
                        let remember_resp = client
                            .exchange_auth_request(
                                &pending.request_id,
                                &device_id,
                                &pending.email,
                                &pending.access_code,
                                Some(token),
                                Some(bw_core::api::TwoFactorProviderType::Remember),
                                false,
                            )
                            .await;

                        match remember_resp {
                            Ok(resp) => {
                                // Remember token worked!
                                save_remember_token(
                                    &state,
                                    resp.two_factor_token.as_deref().unwrap_or(token),
                                );

                                // Re-poll to get the encrypted key
                                let response = client
                                    .poll_auth_response(&pending.request_id, &pending.access_code)
                                    .await
                                    .map_err(|e| format!("Failed to re-poll auth response: {e}"))?;
                                let data = response
                                    .and_then(|r| if r.request_approved { Some(r) } else { None })
                                    .ok_or_else(|| "Auth request no longer approved".to_string())?;

                                let key_encrypted = data.key.ok_or_else(|| {
                                    "Auth request approved but no key returned".to_string()
                                })?;
                                let user_key_bytes = decrypt_auth_request_key(
                                    &key_encrypted,
                                    &pending.private_key_pem,
                                    &pending.access_code,
                                )?;

                                let sync_data =
                                    bw_core::api::sync_vault(&client, &resp.access_token)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                let (keys, org_keys) = bw_core::api::unlock_vault_with_user_key(
                                    &user_key_bytes,
                                    &sync_data.protected_private_key,
                                    &sync_data.org_keys,
                                )
                                .map_err(|e| e.to_string())?;

                                let mut agent_state = state.agent_state.lock().await;
                                agent_state.access_token = Some(resp.access_token);
                                agent_state.refresh_token = Some(resp.refresh_token);
                                agent_state.email = Some(pending.email.clone());
                                agent_state.protected_key = Some(sync_data.protected_key);
                                agent_state.protected_private_key =
                                    Some(sync_data.protected_private_key);
                                agent_state.protected_org_keys = sync_data.org_keys;
                                agent_state.entries = sync_data.entries;
                                agent_state.set_unlocked(keys, org_keys);

                                if let Ok(mut slot) = state.pending_auth_request.lock() {
                                    *slot = None;
                                }
                                if let Err(e) =
                                    crate::events::emit_lock_state_changed(&state.app_handle, false)
                                {
                                    log::warn!("Failed to emit lock-state-changed: {e}");
                                }

                                return Ok(PollAuthRequestResult {
                                    approved: true,
                                    fingerprint_validated: true,
                                    two_factor_required: None,
                                });
                            }
                            Err(_) => {
                                // Remember token rejected — clear it, fall through to 2FA prompt
                                clear_remember_token(&state);
                            }
                        }
                    }

                    // No remember token or it was rejected — return to frontend for 2FA code
                    Ok(PollAuthRequestResult {
                        approved: true,
                        fingerprint_validated: true,
                        two_factor_required: Some(providers.iter().map(|p| *p as u8).collect()),
                    })
                }
                Err(e) => Err(format!("Token exchange failed: {e}")),
            }
        }
        Some(_) => {
            // Response exists but not approved (or declined)
            Ok(PollAuthRequestResult {
                approved: false,
                fingerprint_validated: true,
                two_factor_required: None,
            })
        }
        None => {
            // Not yet responded
            Ok(PollAuthRequestResult {
                approved: false,
                fingerprint_validated: false,
                two_factor_required: None,
            })
        }
    }
}

/// Cancel a pending auth request.
#[tauri::command]
pub async fn cancel_auth_request(state: State<'_, AppState>) -> Result<(), String> {
    if let Ok(mut slot) = state.pending_auth_request.lock() {
        *slot = None;
    }
    Ok(())
}

/// Submit 2FA code for a pending auth request token exchange.
/// Called when poll_auth_request returns two_factor_required: Some(providers).
#[derive(serde::Serialize)]
pub struct SubmitAuthRequest2FaResult {
    pub success: bool,
}

#[tauri::command]
pub async fn submit_auth_request_two_factor(
    state: State<'_, AppState>,
    provider: u8,
    code: String,
    remember: bool,
) -> Result<SubmitAuthRequest2FaResult, String> {
    let pending = {
        let guard = state.pending_auth_request.lock();
        match guard {
            Ok(slot) => slot.clone(),
            Err(e) => {
                log::error!("Failed to access pending auth request state: {e}");
                return Err("Internal state error".to_string());
            }
        }
    };

    let pending = match pending {
        Some(p) => p,
        None => return Err("No pending auth request".to_string()),
    };

    let device_id = bw_agent::config::Config::load()
        .device_id
        .ok_or_else(|| "No registered device".to_string())?;

    let client = state.client.read().map_err(|e| e.to_string())?.clone();

    let two_factor_provider = bw_core::api::TwoFactorProviderType::try_from(provider as u64)
        .map_err(|e| format!("Invalid 2FA provider: {e}"))?;

    let resp = client
        .exchange_auth_request(
            &pending.request_id,
            &device_id,
            &pending.email,
            &pending.access_code,
            Some(&code),
            Some(two_factor_provider),
            remember,
        )
        .await
        .map_err(|e| format!("2FA token exchange failed: {e}"))?;

    // Re-poll to get the key (the approval data should still be valid)
    let response = client
        .poll_auth_response(&pending.request_id, &pending.access_code)
        .await
        .map_err(|e| format!("Failed to re-poll auth response: {e}"))?;

    let data = response
        .and_then(|r| if r.request_approved { Some(r) } else { None })
        .ok_or_else(|| "Auth request no longer approved".to_string())?;

    let key_encrypted = data
        .key
        .ok_or_else(|| "Auth request approved but no key returned".to_string())?;

    let user_key_bytes = decrypt_auth_request_key(
        &key_encrypted,
        &pending.private_key_pem,
        &pending.access_code,
    )?;

    // Sync vault data using the access token
    let sync_data = bw_core::api::sync_vault(&client, &resp.access_token)
        .await
        .map_err(|e| e.to_string())?;

    // Unlock vault using the decrypted user key
    let (keys, org_keys) = bw_core::api::unlock_vault_with_user_key(
        &user_key_bytes,
        &sync_data.protected_private_key,
        &sync_data.org_keys,
    )
    .map_err(|e| e.to_string())?;

    // Store everything in agent state
    let mut agent_state = state.agent_state.lock().await;
    agent_state.access_token = Some(resp.access_token);
    agent_state.refresh_token = Some(resp.refresh_token);
    agent_state.email = Some(pending.email.clone());
    agent_state.protected_key = Some(sync_data.protected_key);
    agent_state.protected_private_key = Some(sync_data.protected_private_key);
    agent_state.protected_org_keys = sync_data.org_keys;
    agent_state.entries = sync_data.entries;
    agent_state.set_unlocked(keys, org_keys);

    // Save 2FA remember token if provided
    if let Some(ref token) = resp.two_factor_token {
        save_remember_token(&state, token);
    }

    // Clear pending auth request
    if let Ok(mut slot) = state.pending_auth_request.lock() {
        *slot = None;
    }

    // Emit events
    if let Err(e) = crate::events::emit_lock_state_changed(&state.app_handle, false) {
        log::warn!("Failed to emit lock-state-changed: {e}");
    }

    Ok(SubmitAuthRequest2FaResult { success: true })
}

/// Decrypt the user key returned by the auth request.
/// The key is encrypted with the public key we provided, and can be decrypted
/// using our private key. The access code is used as additional entropy.
fn decrypt_auth_request_key(
    encrypted_key: &str,
    private_key_pem: &str,
    _access_code: &str,
) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    use rsa::pkcs8::DecodePrivateKey as _;

    // The encrypted key comes in EncString format: "4.<base64_ciphertext>"
    // Type 4 = Rsa2048_OaepSha1_B64. Strip the prefix if present.
    let b64_data = if let Some(dot_pos) = encrypted_key.find('.') {
        &encrypted_key[dot_pos + 1..]
    } else {
        // Fallback: treat entire string as base64 (for non-standard servers)
        encrypted_key
    };

    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(b64_data)
        .map_err(|e| format!("Failed to decode encrypted key: {e}"))?;

    // Parse our private key from PEM
    let private_key = rsa::RsaPrivateKey::from_pkcs8_pem(private_key_pem)
        .map_err(|e| format!("Failed to parse private key: {e}"))?;

    // RSA-OAEP decrypt with SHA-1 (Bitwarden's default)
    let padding = rsa::Oaep::new::<sha1::Sha1>();
    let decrypted = private_key
        .decrypt(padding, &ciphertext)
        .map_err(|e| format!("Failed to decrypt user key: {e}"))?;

    // The decrypted RSA output is the raw user key bytes (64 bytes: 32 AES + 32 HMAC)
    Ok(decrypted)
}
