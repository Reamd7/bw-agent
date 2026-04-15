mod auth;
mod config;
#[cfg(windows)]
mod pipe;
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // Load config file, then apply env var overrides
    let mut config = config::Config::load();
    config.apply_env_overrides();
    config.validate()?;

    let email = config.email.clone().unwrap(); // safe: validate() checked
    let api_url = config.api_url();
    let identity_url = config.identity_url();

    log::info!("Email: {email}");
    log::info!("API URL: {api_url}");
    log::info!("Identity URL: {identity_url}");
    if let Some(proxy) = &config.proxy {
        log::info!("Proxy: {proxy}");
    }

    let client = bw_core::api::Client::new(&api_url, &identity_url, config.proxy.as_deref());

    let mut initial_state = state::State::new(Duration::from_secs(config.lock_timeout));
    initial_state.email = Some(email);

    let state = Arc::new(Mutex::new(initial_state));
    let handler = ssh_agent::SshAgentHandler::new(state, client);

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
        let socket_path =
            std::env::var("BW_SOCKET_PATH").unwrap_or_else(|_| default_socket_path());
        let _ = std::fs::remove_file(&socket_path);
        log::info!("Listening on Unix socket: {socket_path}");
        let listener = tokio::net::UnixListener::bind(&socket_path)?;
        ssh_agent_lib::agent::listen(listener, handler).await?;
    }

    Ok(())
}
