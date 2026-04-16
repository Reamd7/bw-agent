#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod events;
mod tray;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use tauri::{Manager, WindowEvent};

pub struct AppState {
    pub app_handle: tauri::AppHandle,
    pub agent_state: Arc<tokio::sync::Mutex<bw_agent::state::State>>,
    pub client: bw_core::api::Client,
    pub approval_queue: Arc<bw_agent::approval::ApprovalQueue>,
    pub access_log: Arc<bw_agent::access_log::AccessLog>,
    pub password_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<Option<String>>>>>,
    pub two_factor_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<Option<(u8, String)>>>>>,
}

#[derive(Clone)]
pub struct TauriUiCallback {
    app_handle: tauri::AppHandle,
    password_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<Option<String>>>>>,
    two_factor_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<Option<(u8, String)>>>>>,
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
        if let Err(emit_error) = events::emit_approval_requested(&self.app_handle, request.clone()) {
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
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            tray::setup_tray(app)?;

            let mut config = bw_agent::config::Config::load();
            config.apply_env_overrides();
            config.validate().map_err(to_tauri_error)?;

            let app_handle = app.handle().clone();
            let client = bw_core::api::Client::new(
                &config.api_url(),
                &config.identity_url(),
                config.proxy.as_deref(),
            );
            let approval_queue = Arc::new(bw_agent::approval::ApprovalQueue::new());
            let access_log = Arc::new(open_access_log().map_err(to_tauri_error)?);
            let password_tx = Arc::new(Mutex::new(None));
            let two_factor_tx = Arc::new(Mutex::new(None));

            let mut initial_state = bw_agent::state::State::new(Duration::from_secs(config.lock_timeout));
            initial_state.email = config.email.clone();
            let agent_state = Arc::new(tokio::sync::Mutex::new(initial_state));

            let ui = TauriUiCallback {
                app_handle: app_handle.clone(),
                password_tx: Arc::clone(&password_tx),
                two_factor_tx: Arc::clone(&two_factor_tx),
            };

            app.manage(AppState {
                app_handle: app_handle.clone(),
                agent_state: Arc::clone(&agent_state),
                client: client.clone(),
                approval_queue: Arc::clone(&approval_queue),
                access_log: Arc::clone(&access_log),
                password_tx,
                two_factor_tx,
            });

            let agent_config = config.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = bw_agent::start_agent_with_shared_state(
                    agent_config,
                    ui,
                    agent_state,
                    client,
                    approval_queue,
                    access_log,
                )
                .await
                {
                    log::error!("bw-agent SSH agent exited with error: {error}");
                }
            });

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
            commands::submit_password,
            commands::submit_two_factor,
            commands::list_keys,
            commands::get_access_logs,
            commands::approve_request,
            commands::get_pending_approvals,
            commands::lock_vault,
            commands::get_config,
            commands::save_config,
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
