use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bw-agent", about = "Bitwarden-backed SSH agent")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Act as Git SSH signing program (gpg.ssh.program)
    GitSign {
        /// Action to perform (sign, verify, find-principals, check-novalidate)
        #[arg(short = 'Y', long)]
        action: String,
        /// Namespace (e.g. "git")
        #[arg(short = 'n', long)]
        namespace: Option<String>,
        /// Key file or allowed_signers file
        #[arg(short = 'f', long)]
        key_file: Option<String>,
        /// Principal identity
        #[arg(short = 'I', long)]
        principal: Option<String>,
        /// Signature file
        #[arg(short = 's', long)]
        signature_file: Option<String>,
        /// Use SSH agent for signing
        #[arg(short = 'U')]
        use_agent: bool,
        /// Options (e.g. -Overify-time=...)
        #[arg(short = 'O', number_of_values = 1)]
        options: Vec<String>,
        /// Data file (positional, for sign)
        data_file: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    // If invoked as a binary whose name ends with "git-sign" (e.g. via a
    // hardlink / symlink named bw-agent-git-sign), skip clap and go straight
    // to the signing code path.  Git calls gpg.ssh.program as:
    //   <program> -Y <action> -n git -f <keyfile> [options] <datafile>
    let argv0 = std::env::args().next().unwrap_or_default();
    let bin_name = std::path::Path::new(&argv0)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();

    if bin_name.ends_with("git-sign") {
        return bw_agent::git_sign::run_from_args(&std::env::args().skip(1).collect::<Vec<_>>());
    }

    let cli = Cli::parse();

    match cli.command {
        None => {
            // Default: start SSH agent daemon
            let mut config = bw_agent::config::Config::load();
            config.apply_env_overrides();
            config.validate()?;
            let ui = bw_agent::StubUiCallback;
            bw_agent::start_agent(config, ui).await
        }
        Some(Commands::GitSign {
            action,
            namespace,
            key_file,
            principal,
            signature_file,
            use_agent: _,
            options: _,
            data_file,
        }) => bw_agent::git_sign::run(
            &action,
            namespace.as_deref(),
            key_file.as_deref(),
            principal.as_deref(),
            signature_file.as_deref(),
            data_file.as_deref(),
        ),
    }
}
