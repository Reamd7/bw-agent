#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod system_events;
mod tray;

use std::sync::{Arc, Mutex};

use tauri::{Manager, WindowEvent};

/// Holds intermediate login state between the first password attempt
/// and the 2FA retry. Created when `unlock` gets `TwoFactorRequired`,
/// consumed when `unlock_with_two_factor` succeeds.
pub struct PendingTwoFactor {
    pub device_id: String,
    pub email: String,
    pub password: bw_core::locked::Password,
    pub identity: bw_core::identity::Identity,
    pub kdf: bw_core::api::KdfType,
    pub iterations: u32,
    pub memory: Option<u32>,
    pub parallelism: Option<u32>,
    /// True if a stored remember token was attempted but rejected by the server.
    pub remember_token_rejected: bool,
}

/// Holds intermediate state for "Login with Device" flow.
/// Created by `create_auth_request`, consumed by `poll_auth_request`.
#[derive(Clone)]
pub struct PendingAuthRequest {
    pub request_id: String,
    pub access_code: String,
    pub private_key_pem: String,
    pub email: String,
}
/// One-shot channel slot for relaying a master-password prompt response to the
/// core agent. Wrapped in `Arc<Mutex<Option<_>>>` so the UI callback can atomically
/// install/consume the sender from any thread.
pub type PasswordPromptSlot = Arc<Mutex<Option<tokio::sync::oneshot::Sender<Option<String>>>>>;

/// One-shot channel slot for relaying a 2FA prompt response (`(provider, token)`)
/// back to the core agent. Same `Arc<Mutex<Option<_>>>` pattern as
/// [`PasswordPromptSlot`].
pub type TwoFactorPromptSlot =
    Arc<Mutex<Option<tokio::sync::oneshot::Sender<Option<(u8, String)>>>>>;

pub struct AppState {
    pub app_handle: tauri::AppHandle,
    pub agent_state: Arc<tokio::sync::Mutex<bw_agent::state::State>>,
    pub client: Arc<std::sync::RwLock<bw_core::api::Client>>,
    pub approval_queue: Arc<bw_agent::approval::ApprovalQueue>,
    pub access_log: Arc<bw_agent::access_log::AccessLog>,
    pub session_store: Arc<bw_agent::session_store::SessionStore>,
    pub password_tx: PasswordPromptSlot,
    pub two_factor_tx: TwoFactorPromptSlot,
    pub pending_two_factor: Arc<Mutex<Option<PendingTwoFactor>>>,
    pub pending_auth_request: Arc<Mutex<Option<PendingAuthRequest>>>,
}

#[derive(Clone)]
pub struct TauriUiCallback {
    app_handle: tauri::AppHandle,
    password_tx: PasswordPromptSlot,
    two_factor_tx: TwoFactorPromptSlot,
}

impl bw_agent::UiCallback for TauriUiCallback {
    async fn request_password(&self, email: &str, error: Option<&str>) -> Option<String> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        match self.password_tx.lock() {
            Ok(mut sender) => *sender = Some(tx),
            Err(_) => {
                log::error!("password prompt state is poisoned");
                return None;
            }
        }

        if let Err(emit_error) = events::emit_password_requested(
            &self.app_handle,
            events::PasswordRequestPayload {
                email: email.to_string(),
                error: error.map(str::to_string),
            },
        ) {
            log::error!("failed to emit password request event: {emit_error}");
            clear_pending_sender(&self.password_tx);
            return None;
        }

        rx.await.ok().flatten()
    }

    async fn request_two_factor(&self, providers: &[u8]) -> Option<(u8, String)> {
        let (tx, rx) = tokio::sync::oneshot::channel();

        match self.two_factor_tx.lock() {
            Ok(mut sender) => *sender = Some(tx),
            Err(_) => {
                log::error!("two-factor prompt state is poisoned");
                return None;
            }
        }

        if let Err(emit_error) = events::emit_two_factor_requested(
            &self.app_handle,
            events::TwoFactorRequestPayload {
                providers: providers.to_vec(),
            },
        ) {
            log::error!("failed to emit two-factor request event: {emit_error}");
            clear_pending_sender(&self.two_factor_tx);
            return None;
        }

        rx.await.ok().flatten()
    }

    async fn request_approval(&self, request: &bw_agent::ApprovalRequest) -> bool {
        if let Err(emit_error) = events::emit_approval_requested(&self.app_handle, request.clone())
        {
            log::error!("failed to emit approval request event: {emit_error}");
            return false;
        }

        // Send system notification.
        send_approval_notification(&self.app_handle, request);

        // Show + focus the main window so user can respond.
        if let Some(window) = self.app_handle.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }

        true
    }
}

fn main() {
    // In debug builds, always write Rust logs to a temp file so e2e tests
    // can capture the desktop console output. The file path is written to
    // a well-known marker file that tests can read.
    #[cfg(debug_assertions)]
    {
        let log_dir = std::env::var("TEMP")
            .or_else(|_| std::env::var("TMP"))
            .unwrap_or_else(|_| "/tmp".to_string());
        let log_path = std::path::Path::new(&log_dir).join("bw-agent-e2e.log");
        let marker_path = std::path::Path::new(&log_dir).join("bw-agent-e2e.log.path");

        if let Ok(file) = std::fs::File::create(&log_path) {
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
                .target(env_logger::Target::Pipe(Box::new(file)))
                .format_timestamp_secs()
                .init();

            // Write marker so tests know where to find the log
            if let Ok(mut m) = std::fs::File::create(&marker_path) {
                use std::io::Write;
                let _ = m.write_all(log_path.to_string_lossy().as_bytes());
            }
        } else {
            env_logger::init();
        }
    }
    #[cfg(not(debug_assertions))]
    {
        env_logger::init();
    }

    // Install a panic hook that writes crash details to a log file
    // before the process terminates. This gives users something to
    // report when the agent disappears silently.
    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        log::error!("PANIC: {msg}");

        let crash_dir = dirs_data_dir().join("bw-agent");
        let _ = std::fs::create_dir_all(&crash_dir);
        let crash_path = crash_dir.join("crash.log");

        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&crash_path)
        {
            use std::io::Write;
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[unix:{timestamp}] PANIC: {msg}");
        }
    }));

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            tray::setup_tray(app)?;

            let mut config = bw_agent::config::Config::load();
            config.apply_env_overrides();

            let app_handle = app.handle().clone();
            let client = Arc::new(std::sync::RwLock::new(bw_core::api::Client::new(
                &config.api_url(),
                &config.identity_url(),
                config.proxy.as_deref(),
            )));
            let approval_queue = Arc::new(bw_agent::approval::ApprovalQueue::new());
            let access_log = Arc::new(open_access_log().map_err(to_tauri_error)?);
            let session_store = Arc::new(bw_agent::session_store::SessionStore::new());
            let password_tx = Arc::new(Mutex::new(None));
            let two_factor_tx = Arc::new(Mutex::new(None));
            let pending_two_factor = Arc::new(Mutex::new(None));
            let pending_auth_request = Arc::new(Mutex::new(None));
            let pending_two_factor_for_bg = Arc::clone(&pending_two_factor);

            let mut initial_state = bw_agent::state::State::new(config.lock_mode.cache_ttl());
            initial_state.email = config.email.clone();
            if config.email.is_none() {
                log::info!("Email not configured — waiting for setup via UI");
            }
            let agent_state = Arc::new(tokio::sync::Mutex::new(initial_state));

            let ui = TauriUiCallback {
                app_handle: app_handle.clone(),
                password_tx: Arc::clone(&password_tx),
                two_factor_tx: Arc::clone(&two_factor_tx),
            };

            app.manage(AppState {
                app_handle: app_handle.clone(),
                agent_state: Arc::clone(&agent_state),
                client: Arc::clone(&client),
                approval_queue: Arc::clone(&approval_queue),
                access_log: Arc::clone(&access_log),
                session_store: Arc::clone(&session_store),
                password_tx,
                two_factor_tx,
                pending_two_factor,
                pending_auth_request,
            });

            let agent_config = config.clone();
            let agent_state_for_agent = Arc::clone(&agent_state);
            let client_for_agent = client.read().unwrap().clone();
            let approval_queue_for_agent = Arc::clone(&approval_queue);
            let access_log_for_agent = Arc::clone(&access_log);
            let session_store_for_agent = Arc::clone(&session_store);
            tauri::async_runtime::spawn(async move {
                if let Err(error) = bw_agent::start_agent_with_shared_state(
                    agent_config,
                    ui,
                    agent_state_for_agent,
                    client_for_agent,
                    approval_queue_for_agent,
                    access_log_for_agent,
                    session_store_for_agent,
                )
                .await
                {
                    log::error!("bw-agent SSH agent exited with error: {error}");
                }
            });

            // Start periodic vault sync (every 60s while unlocked).
            start_background_tasks(
                app_handle.clone(),
                Arc::clone(&agent_state),
                Arc::clone(&client),
                pending_two_factor_for_bg,
            );

            // Initialize system event listeners (idle, sleep, lock, shutdown).
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            if let Err(error) =
                system_events::init(&app_handle, &config.lock_mode, Arc::clone(&agent_state))
            {
                log::error!("Failed to initialize system event listeners: {error}");
            }

            let main_window = app
                .get_webview_window("main")
                .expect("main window should exist");
            let window_to_hide = main_window.clone();

            main_window.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();

                    if let Err(error) = window_to_hide.hide() {
                        log::error!("failed to hide main window on close request: {error}");
                    }
                }
            });

            log::info!("Tauri app setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::unlock,
            commands::unlock_with_two_factor,
            commands::submit_password,
            commands::submit_two_factor,
            commands::list_keys,
            commands::get_access_logs,
            commands::approve_request,
            commands::get_pending_approvals,
            commands::lock_vault,
            commands::manual_sync,
            commands::get_config,
            commands::save_config,
            commands::update_lock_mode,
            commands::get_git_signing_status,
            commands::configure_git_signing,
            commands::get_git_sign_program_path,
            commands::update_key_fields,
            commands::approve_request_with_session,
            commands::list_active_sessions,
            commands::revoke_session,
            commands::create_auth_request,
            commands::poll_auth_request,
            commands::cancel_auth_request,
            commands::submit_auth_request_two_factor,
            commands::has_two_factor_remember,
            commands::revoke_two_factor_remember,
            commands::has_registered_device,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn send_approval_notification(app_handle: &tauri::AppHandle, request: &bw_agent::ApprovalRequest) {
    use tauri_plugin_notification::NotificationExt;

    let chain_display = if request.process_chain.is_empty() {
        request.client_exe.clone()
    } else {
        request
            .process_chain
            .iter()
            .map(|p| {
                p.exe
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(&p.exe)
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join(" → ")
    };

    let body = format!(
        "{} requests access to key \"{}\"",
        chain_display, request.key_name,
    );

    if let Err(error) = app_handle
        .notification()
        .builder()
        .title("SSH Key Access Requested")
        .body(body)
        .show()
    {
        log::warn!("Failed to send notification: {error}");
    }
}

fn clear_pending_sender<T>(sender: &Mutex<Option<tokio::sync::oneshot::Sender<T>>>) {
    if let Ok(mut sender) = sender.lock() {
        sender.take();
    }
}

fn open_access_log() -> anyhow::Result<bw_agent::access_log::AccessLog> {
    let data_dir = dirs_data_dir().join("bw-agent");
    std::fs::create_dir_all(&data_dir)?;

    bw_agent::access_log::AccessLog::open(&data_dir.join("access_log.db"))
        .map_err(|error| anyhow::anyhow!("Failed to open access log: {error}"))
}

fn to_tauri_error(error: impl std::fmt::Display) -> tauri::Error {
    tauri::Error::Anyhow(anyhow::anyhow!(error.to_string()))
}

fn dirs_data_dir() -> std::path::PathBuf {
    #[cfg(windows)]
    {
        std::env::var("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("C:\\ProgramData"))
    }
    #[cfg(unix)]
    {
        std::env::var("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                std::path::PathBuf::from(home).join(".local/share")
            })
    }
}

/// Background loop that:
/// 1. Checks TTL expiry every 5s — if expired, clears state and pushes lock event to frontend.
/// 2. Syncs vault data every 60s — on auth failure, forces re-login.
fn start_background_tasks(
    app_handle: tauri::AppHandle,
    agent_state: Arc<tokio::sync::Mutex<bw_agent::state::State>>,
    client: Arc<std::sync::RwLock<bw_core::api::Client>>,
    pending_two_factor: Arc<Mutex<Option<PendingTwoFactor>>>,
) {
    tauri::async_runtime::spawn(async move {
        const SYNC_INTERVAL_TICKS: u64 = 12; // 12 * 5s = 60s
        let mut tick_count: u64 = 0;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        interval.tick().await; // Skip the first immediate tick

        loop {
            interval.tick().await;
            tick_count += 1;

            // ── Expiry check (every 5s) ──────────────────────────────
            {
                let state = agent_state.lock().await;
                if state.is_expired() {
                    drop(state);
                    // Keys exist but TTL exceeded — lock the vault proactively.
                    let mut state = agent_state.lock().await;
                    state.clear();
                    drop(state);
                    if let Ok(mut pending) = pending_two_factor.lock() {
                        pending.take();
                    }
                    log::info!("Vault TTL expired — locking and notifying frontend");
                    if let Err(e) = events::emit_lock_state_changed(&app_handle, true) {
                        log::warn!("Failed to emit lock-state-changed: {e}");
                    }
                    continue;
                }
            }

            // ── Periodic sync (every 60s = 12 ticks of 5s) ──────────
            if tick_count % SYNC_INTERVAL_TICKS != 0 {
                continue;
            }

            let (access_token, refresh_token) = {
                let state = agent_state.lock().await;
                if !state.is_unlocked() {
                    continue;
                }
                match state.access_token.clone() {
                    Some(token) => (token, state.refresh_token.clone()),
                    None => continue,
                }
            };

            // Try to refresh token before sync
            let access_token = match refresh_token {
                Some(rt) => {
                    let client_clone = client.read().unwrap().clone();
                    match client_clone.exchange_refresh_token(&rt).await {
                        Ok(new_token) => {
                            agent_state.lock().await.access_token = Some(new_token.clone());
                            log::debug!("Token refreshed during periodic sync");
                            new_token
                        }
                        Err(e) => {
                            log::debug!("Token refresh failed during periodic sync: {e}");
                            access_token
                        }
                    }
                }
                None => access_token,
            };

            {
                let client = client.read().unwrap().clone();
                match bw_core::api::sync_vault(&client, &access_token).await {
                    Ok(sync_data) => {
                        let mut state = agent_state.lock().await;
                        if state.is_unlocked() {
                            state.entries = sync_data.entries;
                            state.protected_org_keys = sync_data.org_keys;
                            log::debug!("Periodic sync: updated {} entries", state.entries.len());
                            if let Err(e) = events::emit_vault_synced(
                                &app_handle,
                                events::VaultSyncedPayload {
                                    success: true,
                                    error: None,
                                },
                            ) {
                                log::warn!("Failed to emit vault-synced: {e}");
                            }
                        }
                    }
                    Err(error) => {
                        let error_msg = error.to_string();
                        log::warn!("Periodic sync failed: {error_msg}");

                        let is_auth_error = error_msg.contains("401")
                            || error_msg.to_lowercase().contains("unauthorized")
                            || error_msg.to_lowercase().contains("token");
                        if is_auth_error {
                            log::warn!("Auth failure during sync — forcing re-login");
                            agent_state.lock().await.clear();
                            if let Err(e) = events::emit_lock_state_changed(&app_handle, true) {
                                log::warn!("Failed to emit lock-state-changed: {e}");
                            }
                        }

                        if let Err(e) = events::emit_vault_synced(
                            &app_handle,
                            events::VaultSyncedPayload {
                                success: false,
                                error: Some(error_msg),
                            },
                        ) {
                            log::warn!("Failed to emit vault-synced: {e}");
                        }
                    }
                }
            }
        }
    });
}
