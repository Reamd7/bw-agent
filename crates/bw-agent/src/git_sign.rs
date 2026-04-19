//! Git SSH signing program — drop-in replacement for ssh-keygen -Y.

/// Entry point for the git-sign subcommand.
pub fn run(
    action: &str,
    _namespace: Option<&str>,
    _key_file: Option<&str>,
    _principal: Option<&str>,
    _signature_file: Option<&str>,
    _data_file: Option<&str>,
) -> anyhow::Result<()> {
    eprintln!("error: unsupported action: {action}");
    std::process::exit(1);
}
