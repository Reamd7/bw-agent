pub mod access_log;
pub mod approval;
pub mod auth;
pub mod config;
pub mod git_context;
pub mod routing;
#[cfg(windows)]
pub mod pipe;
pub mod process;
pub mod ssh_agent;
pub mod state;

use std::sync::Arc;
use tokio::sync::Mutex;

pub use approval::ApprovalRequest;
pub use process::ProcessInfo;

/// Callback trait for UI interactions (password prompt, 2FA, approval).
/// Tauri implements this; standalone mode uses StubUiCallback.
pub trait UiCallback: Send + Sync + 'static {
    /// Request master password from user. Returns None if user cancels.
    fn request_password(
        &self,
        email: &str,
        error: Option<&str>,
    ) -> impl std::future::Future<Output = Option<String>> + Send;

    /// Request 2FA code. `providers` lists available TwoFactorProviderType values.
    /// Returns (provider_type, code) or None if cancelled.
    fn request_two_factor(
        &self,
        providers: &[u8],
    ) -> impl std::future::Future<Output = Option<(u8, String)>> + Send;

    /// Request approval for an SSH sign operation.
    /// Returns true if approved, false if denied.
    fn request_approval(
        &self,
        request: &ApprovalRequest,
    ) -> impl std::future::Future<Output = bool> + Send;
}

/// Stub UI callback that always denies (for headless/standalone mode).
pub struct StubUiCallback;

impl UiCallback for StubUiCallback {
    async fn request_password(&self, _email: &str, _error: Option<&str>) -> Option<String> {
        log::warn!("StubUiCallback: no UI available for password prompt");
        None
    }

    async fn request_two_factor(&self, _providers: &[u8]) -> Option<(u8, String)> {
        log::warn!("StubUiCallback: no UI available for 2FA prompt");
        None
    }

    async fn request_approval(&self, _request: &ApprovalRequest) -> bool {
        log::warn!("StubUiCallback: no UI available for approval, auto-denying");
        false
    }
}

/// Default Windows pipe name (matching OpenSSH ssh-agent).
#[cfg(windows)]
const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\openssh-ssh-agent";

/// Derive the default Unix socket path.
#[cfg(unix)]
fn default_socket_path() -> String {
    std::env::var("SSH_AUTH_SOCK").unwrap_or_else(|_| {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        format!("{runtime_dir}/bw-agent.sock")
    })
}

/// Start the SSH agent service. Blocks until shutdown.
///
/// This is the main entry point called by both the standalone binary
/// and the Tauri app shell.
pub async fn start_agent<U: UiCallback>(config: config::Config, ui: U) -> anyhow::Result<()> {
    let email = config
        .email
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Email not configured"))?;
    let api_url = config.api_url();
    let identity_url = config.identity_url();

    log::info!("Email: {email}");
    log::info!("API URL: {api_url}");
    log::info!("Identity URL: {identity_url}");
    if let Some(proxy) = &config.proxy {
        log::info!("Proxy: {proxy}");
    }

    let client = bw_core::api::Client::new(&api_url, &identity_url, config.proxy.as_deref());

    let mut initial_state = state::State::new(config.lock_mode.cache_ttl());
    initial_state.email = Some(email);

    let state = Arc::new(Mutex::new(initial_state));

    // Initialize approval queue and access log.
    let approval_queue = Arc::new(approval::ApprovalQueue::new());

    let data_dir = dirs_data_dir().join("bw-agent");
    std::fs::create_dir_all(&data_dir)?;
    let access_log = Arc::new(
        access_log::AccessLog::open(&data_dir.join("access_log.db"))
            .map_err(|e| anyhow::anyhow!("Failed to open access log: {e}"))?,
    );

    start_agent_with_shared_state(config, ui, state, client, approval_queue, access_log).await
}

/// Start the SSH agent service using caller-provided shared state and services.
pub async fn start_agent_with_shared_state<U: UiCallback>(
    config: config::Config,
    ui: U,
    state: Arc<Mutex<state::State>>,
    client: bw_core::api::Client,
    approval_queue: Arc<approval::ApprovalQueue>,
    access_log: Arc<access_log::AccessLog>,
) -> anyhow::Result<()> {
    let email = config.email.clone().unwrap_or_default();
    let api_url = config.api_url();
    let identity_url = config.identity_url();

    {
        let mut state = state.lock().await;
        state.email = if email.is_empty() {
            None
        } else {
            Some(email.clone())
        };
    }

    if email.is_empty() {
        log::info!(
            "Email: not configured — SSH agent running, vault operations will wait for setup"
        );
    } else {
        log::info!("Email: {email}");
    }
    log::info!("API URL: {api_url}");
    log::info!("Identity URL: {identity_url}");
    if let Some(proxy) = &config.proxy {
        log::info!("Proxy: {proxy}");
    }

    let handler =
        ssh_agent::SshAgentHandler::new(state, client, Arc::new(ui), approval_queue, access_log);

    log::info!("Starting bw-agent SSH agent...");

    #[cfg(windows)]
    {
        let pipe_name =
            std::env::var("BW_PIPE_NAME").unwrap_or_else(|_| DEFAULT_PIPE_NAME.to_string());
        log::info!("Listening on named pipe: {pipe_name}");
        let listener = pipe::SecureNamedPipeListener::bind(&pipe_name)?;
        ssh_agent_lib::agent::listen(listener, handler).await?;
    }

    #[cfg(unix)]
    {
        let socket_path = std::env::var("BW_SOCKET_PATH").unwrap_or_else(|_| default_socket_path());
        let _ = std::fs::remove_file(&socket_path);
        log::info!("Listening on Unix socket: {socket_path}");
        let listener = tokio::net::UnixListener::bind(&socket_path)?;
        ssh_agent_lib::agent::listen(listener, handler).await?;
    }
    Ok(())
}

/// Platform-appropriate data directory.
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
