mod auth;
mod ssh_agent;
mod state;

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[cfg(windows)]
const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\openssh-ssh-agent";

#[cfg(unix)]
fn default_socket_path() -> String {
    std::env::var("SSH_AUTH_SOCK").unwrap_or_else(|_| {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
        format!("{runtime_dir}/bw-agent.sock")
    })
}

const DEFAULT_CACHE_TTL_SECS: u64 = 900;
const DEFAULT_PROXY: &str = "http://127.0.0.1:7890";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let email = std::env::var("BW_EMAIL").expect("BW_EMAIL environment variable must be set");
    let base_url =
        std::env::var("BW_BASE_URL").unwrap_or_else(|_| "https://api.bitwarden.com".to_string());
    let identity_url = std::env::var("BW_IDENTITY_URL")
        .unwrap_or_else(|_| "https://identity.bitwarden.com".to_string());
    let proxy = std::env::var("BW_PROXY").unwrap_or_else(|_| DEFAULT_PROXY.to_string());
    let proxy = if proxy.is_empty() { None } else { Some(proxy) };
    let cache_ttl = std::env::var("BW_CACHE_TTL")
        .ok()
        .and_then(|ttl| ttl.parse::<u64>().ok())
        .unwrap_or(DEFAULT_CACHE_TTL_SECS);

    let client = bw_core::api::Client::new(&base_url, &identity_url, proxy.as_deref());

    let mut initial_state = state::State::new(Duration::from_secs(cache_ttl));
    initial_state.email = Some(email);

    let state = Arc::new(Mutex::new(initial_state));
    let handler = ssh_agent::SshAgentHandler::new(state, client);

    log::info!("Starting bw-agent SSH agent...");

    #[cfg(windows)]
    {
        let pipe_name =
            std::env::var("BW_PIPE_NAME").unwrap_or_else(|_| DEFAULT_PIPE_NAME.to_string());
        log::info!("Listening on named pipe: {pipe_name}");
        let listener = ssh_agent_lib::agent::NamedPipeListener::bind(pipe_name)?;
        ssh_agent_lib::agent::listen(listener, handler).await?;
    }

    #[cfg(unix)]
    {
        let socket_path =
            std::env::var("BW_SOCKET_PATH").unwrap_or_else(|_| default_socket_path());
        let _ = std::fs::remove_file(&socket_path);
        log::info!("Listening on Unix socket: {socket_path}");
        let listener = tokio::net::UnixListener::bind(&socket_path)?;
        ssh_agent_lib::agent::listen(listener, handler).await?;
    }

    Ok(())
}
