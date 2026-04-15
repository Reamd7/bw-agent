#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    let mut config = bw_agent::config::Config::load();
    config.apply_env_overrides();
    config.validate()?;
    let ui = bw_agent::StubUiCallback;
    bw_agent::start_agent(config, ui).await
}
