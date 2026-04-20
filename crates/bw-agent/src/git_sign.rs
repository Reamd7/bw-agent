//! Git SSH signing program — drop-in replacement for ssh-keygen -Y.

use anyhow::{Context, anyhow, bail};
use ssh_agent_lib::proto::{Identity, SignRequest};
use ssh_key::{HashAlg, PublicKey, SshSig};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process,
};

const DEFAULT_NAMESPACE: &str = "git";
const RSA_SHA2_512_FLAG: u32 = 0x04;

#[cfg(windows)]
const DEFAULT_PIPE_NAME: &str = r"\\.\pipe\openssh-ssh-agent";

#[derive(Clone, Debug, Eq, PartialEq)]
struct AllowedSigner {
    principals: Vec<String>,
    public_key: PublicKey,
}

/// Entry point when invoked via argv[0] detection (e.g. as `bw-agent-git-sign`).
/// Git calls: `<program> -Y <action> -n git -f <keyfile> [options] <datafile>`
pub fn run_from_args(args: &[String]) -> anyhow::Result<()> {
    let mut action = None;
    let mut namespace = None;
    let mut key_file = None;
    let mut principal = None;
    let mut signature_file = None;
    let mut data_file = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-Y" => {
                i += 1;
                action = args.get(i).cloned();
            }
            "-n" => {
                i += 1;
                namespace = args.get(i).cloned();
            }
            "-f" => {
                i += 1;
                key_file = args.get(i).cloned();
            }
            "-I" => {
                i += 1;
                principal = args.get(i).cloned();
            }
            "-s" => {
                i += 1;
                signature_file = args.get(i).cloned();
            }
            "-U" => { /* use-agent flag, ignored */ }
            "-O" => {
                i += 1; // skip option value
            }
            other if !other.starts_with('-') => {
                data_file = Some(other.to_string());
            }
            _ => {}
        }
        i += 1;
    }

    let action = action.ok_or_else(|| anyhow!("missing -Y action"))?;
    run(
        &action,
        namespace.as_deref(),
        key_file.as_deref(),
        principal.as_deref(),
        signature_file.as_deref(),
        data_file.as_deref(),
    )
}

/// Entry point for the git-sign subcommand.
pub fn run(
    action: &str,
    namespace: Option<&str>,
    key_file: Option<&str>,
    principal: Option<&str>,
    signature_file: Option<&str>,
    data_file: Option<&str>,
) -> anyhow::Result<()> {
    let namespace = namespace.unwrap_or(DEFAULT_NAMESPACE);

    let result = match action {
        "sign" => sign_action(namespace, key_file, data_file),
        "verify" => verify_action(namespace, key_file, principal, signature_file, data_file),
        "find-principals" => find_principals_action(namespace, key_file, signature_file, data_file),
        "match-principals" => match_principals_action(key_file, principal),
        "check-novalidate" => check_novalidate_action(namespace, signature_file, data_file),
        _ => Err(anyhow!("unsupported action: {action}")),
    };

    if let Err(error) = result {
        eprintln!("error: {error}");
        process::exit(255);
    }

    Ok(())
}

fn sign_action(
    namespace: &str,
    key_file: Option<&str>,
    data_file: Option<&str>,
) -> anyhow::Result<()> {
    let key_path = required_path(key_file, "-f/--key-file")?;
    let data_path = required_path(data_file, "data file")?;
    let public_key = read_signing_public_key(&key_path)?;
    let data = fs::read(&data_path)
        .with_context(|| format!("failed to read data file {}", data_path.display()))?;
    let signed_data = SshSig::signed_data(namespace, HashAlg::Sha512, &data)?;

    let mut agent = connect_agent()?;
    let agent_key = find_agent_identity(&mut agent, &public_key)?;
    let signature = agent.sign(SignRequest {
        pubkey: agent_key.key_data().clone(),
        data: signed_data,
        flags: signature_flags(&public_key),
    })?;

    let sshsig = SshSig::new(
        public_key.key_data().clone(),
        namespace,
        HashAlg::Sha512,
        signature,
    )?;
    let pem = sshsig.to_pem(ssh_key::LineEnding::LF)?;

    let output_path = signature_output_path(&data_path);
    eprintln!("Signing file {}", data_path.display());
    fs::write(&output_path, pem)
        .with_context(|| format!("failed to write signature file {}", output_path.display()))?;
    eprintln!("Write signature to {}", output_path.display());

    Ok(())
}

fn verify_action(
    namespace: &str,
    key_file: Option<&str>,
    principal: Option<&str>,
    signature_file: Option<&str>,
    data_file: Option<&str>,
) -> anyhow::Result<()> {
    let allowed_signers_path = required_path(key_file, "-f/--key-file")?;
    let principal =
        principal.ok_or_else(|| anyhow!("missing required principal (-I/--principal)"))?;
    let sshsig = read_signature(required_path(signature_file, "-s/--signature-file")?)?;
    let data = read_data_or_stdin(data_file)?;
    let allowed_signers = parse_allowed_signers_file(&allowed_signers_path)?;

    let signer = allowed_signers
        .iter()
        .find(|entry| {
            entry.public_key.key_data() == sshsig.public_key()
                && entry
                    .principals
                    .iter()
                    .any(|candidate| candidate == principal)
        })
        .ok_or_else(|| anyhow!("principal {principal} is not allowed for this signature"))?;

    if let Err(error) = signer.public_key.verify(namespace, &data, &sshsig) {
        println!("Could not verify signature.");
        eprintln!("error: Signature verification failed: {error}");
        process::exit(255);
    }

    println!(
        "Good \"{}\" signature for {} with {} key {}",
        namespace,
        principal,
        key_type_string(&signer.public_key),
        signer.public_key.fingerprint(Default::default())
    );

    Ok(())
}

fn find_principals_action(
    namespace: &str,
    key_file: Option<&str>,
    signature_file: Option<&str>,
    data_file: Option<&str>,
) -> anyhow::Result<()> {
    let allowed_signers_path = required_path(key_file, "-f/--key-file")?;
    let sshsig = read_signature(required_path(signature_file, "-s/--signature-file")?)?;
    let allowed_signers = parse_allowed_signers_file(&allowed_signers_path)?;

    // ssh-keygen -Y find-principals does NOT require data — it matches the
    // public key embedded in the signature against allowed signers.  When git
    // calls us there is no data file on the command line.
    let matched_principals = match data_file {
        Some(data_path) => {
            let data = fs::read(data_path)
                .with_context(|| format!("failed to read data file {data_path}"))?;
            matching_principals(namespace, &data, &sshsig, &allowed_signers)
        }
        None => {
            // Without data we can only match by public key fingerprint.
            let sig_fingerprint = sshsig.public_key().fingerprint(Default::default());
            allowed_signers
                .iter()
                .filter(|entry| {
                    entry.public_key.fingerprint(Default::default()) == sig_fingerprint
                })
                .flat_map(|entry| entry.principals.iter().map(String::as_str))
                .map(String::from)
                .collect()
        }
    };
    if matched_principals.is_empty() {
        eprintln!("No principal matched.");
        process::exit(255);
    }

    for matched in matched_principals {
        println!("{matched}");
    }

    Ok(())
}

fn match_principals_action(key_file: Option<&str>, principal: Option<&str>) -> anyhow::Result<()> {
    let allowed_signers_path = required_path(key_file, "-f/--key-file")?;
    let principal =
        principal.ok_or_else(|| anyhow!("missing required principal (-I/--principal)"))?;
    let allowed_signers = parse_allowed_signers_file(&allowed_signers_path)?;

    let matched: Vec<&str> = allowed_signers
        .iter()
        .filter(|entry| entry.principals.iter().any(|p| p == principal))
        .flat_map(|entry| entry.principals.iter().map(String::as_str))
        .collect();

    if matched.is_empty() {
        eprintln!("No principal matched.");
        process::exit(255);
    }

    let mut seen = BTreeSet::new();
    for p in matched {
        if seen.insert(p) {
            println!("{p}");
        }
    }

    Ok(())
}

fn check_novalidate_action(
    namespace: &str,
    signature_file: Option<&str>,
    data_file: Option<&str>,
) -> anyhow::Result<()> {
    let sshsig = read_signature(required_path(signature_file, "-s/--signature-file")?)?;
    let data = read_data_or_stdin(data_file)?;
    let public_key = PublicKey::from(sshsig.public_key().clone());

    if let Err(error) = public_key.verify(namespace, &data, &sshsig) {
        println!("Could not verify signature.");
        eprintln!("error: Signature verification failed: {error}");
        process::exit(255);
    }

    println!(
        "Good \"{}\" signature with {} key {}",
        namespace,
        key_type_string(&public_key),
        public_key.fingerprint(Default::default())
    );

    Ok(())
}

fn matching_principals(
    namespace: &str,
    data: &[u8],
    sshsig: &SshSig,
    allowed_signers: &[AllowedSigner],
) -> Vec<String> {
    let mut principals = BTreeSet::new();

    for entry in allowed_signers {
        if entry.public_key.key_data() != sshsig.public_key() {
            continue;
        }

        if entry.public_key.verify(namespace, data, sshsig).is_ok() {
            for principal in &entry.principals {
                principals.insert(principal.clone());
            }
        }
    }

    principals.into_iter().collect()
}

fn required_path(value: Option<&str>, label: &str) -> anyhow::Result<PathBuf> {
    value
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("missing required {label}"))
}

/// Read data from a file path, or fall back to stdin when git pipes the
/// commit contents without a positional file argument.
fn read_data_or_stdin(data_file: Option<&str>) -> anyhow::Result<Vec<u8>> {
    match data_file {
        Some(path) => fs::read(path)
            .with_context(|| format!("failed to read data file {path}")),
        None => {
            use std::io::Read;
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf)?;
            Ok(buf)
        }
    }
}

fn read_signing_public_key(path: &Path) -> anyhow::Result<PublicKey> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read key file {}", path.display()))?;

    if let Ok(public_key) = PublicKey::from_openssh(&contents) {
        return Ok(public_key);
    }

    let private_key = ssh_key::PrivateKey::from_openssh(&contents).with_context(|| {
        format!(
            "failed to parse key file {} as SSH public or private key",
            path.display()
        )
    })?;

    Ok(private_key.public_key().clone())
}

fn read_signature(path: PathBuf) -> anyhow::Result<SshSig> {
    let pem = fs::read(&path)
        .with_context(|| format!("failed to read signature file {}", path.display()))?;
    SshSig::from_pem(pem)
        .with_context(|| format!("failed to parse signature file {}", path.display()))
}

fn signature_output_path(data_path: &Path) -> PathBuf {
    let mut display = data_path.as_os_str().to_os_string();
    display.push(".sig");
    PathBuf::from(display)
}

fn signature_flags(public_key: &PublicKey) -> u32 {
    if matches!(public_key.algorithm(), ssh_key::Algorithm::Rsa { .. }) {
        RSA_SHA2_512_FLAG
    } else {
        0
    }
}

fn key_type_string(public_key: &PublicKey) -> &'static str {
    match public_key.algorithm() {
        ssh_key::Algorithm::Ed25519 => "ED25519",
        ssh_key::Algorithm::Rsa { .. } => "RSA",
        _ => "UNKNOWN",
    }
}

fn parse_allowed_signers_file(path: &Path) -> anyhow::Result<Vec<AllowedSigner>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read allowed signers file {}", path.display()))?;

    parse_allowed_signers(&contents)
}

fn parse_allowed_signers(contents: &str) -> anyhow::Result<Vec<AllowedSigner>> {
    let mut allowed_signers = Vec::new();

    for (index, raw_line) in contents.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let principals = parts
            .next()
            .ok_or_else(|| anyhow!("invalid allowed_signers line {}", index + 1))?;
        let algorithm = parts
            .next()
            .ok_or_else(|| anyhow!("invalid allowed_signers line {}", index + 1))?;
        let key_blob = parts
            .next()
            .ok_or_else(|| anyhow!("invalid allowed_signers line {}", index + 1))?;

        let normalized_algorithm = normalize_allowed_signers_algorithm(algorithm);
        let public_key = PublicKey::from_openssh(&format!("{normalized_algorithm} {key_blob}"))
            .with_context(|| format!("invalid public key on allowed_signers line {}", index + 1))?;

        let principals = principals
            .split(',')
            .filter(|principal| !principal.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        if principals.is_empty() {
            bail!("invalid allowed_signers line {}", index + 1);
        }

        allowed_signers.push(AllowedSigner {
            principals,
            public_key,
        });
    }

    Ok(allowed_signers)
}

fn normalize_allowed_signers_algorithm(algorithm: &str) -> &str {
    match algorithm {
        "rsa-sha2-256" | "rsa-sha2-512" => "ssh-rsa",
        other => other,
    }
}

fn find_agent_identity<S>(
    client: &mut ssh_agent_lib::blocking::Client<S>,
    public_key: &PublicKey,
) -> anyhow::Result<PublicKey>
where
    S: std::io::Read + std::io::Write,
{
    let identities = client.request_identities()?;
    find_matching_identity(public_key, &identities)
}

fn find_matching_identity(
    public_key: &PublicKey,
    identities: &[Identity],
) -> anyhow::Result<PublicKey> {
    identities
        .iter()
        .find(|identity| identity.pubkey == *public_key.key_data())
        .map(|identity| PublicKey::from(identity.pubkey.clone()))
        .ok_or_else(|| anyhow!("requested key is not available in SSH agent"))
}

#[cfg(unix)]
fn connect_agent() -> anyhow::Result<ssh_agent_lib::blocking::Client<std::os::unix::net::UnixStream>>
{
    use std::os::unix::net::UnixStream;

    // Match server's socket resolution order (lib.rs):
    // 1. BW_SOCKET_PATH (bw-agent specific)
    // 2. SSH_AUTH_SOCK (standard SSH agent)
    // 3. $XDG_RUNTIME_DIR/bw-agent.sock (default)
    let socket_path = std::env::var("BW_SOCKET_PATH")
        .ok()
        .or_else(|| std::env::var("SSH_AUTH_SOCK").ok())
        .unwrap_or_else(|| {
            let runtime_dir =
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".to_string());
            format!("{runtime_dir}/bw-agent.sock")
        });

    let stream = UnixStream::connect(&socket_path)
        .with_context(|| format!("failed to connect to SSH agent at {socket_path}"))?;
    Ok(ssh_agent_lib::blocking::Client::new(stream))
}

#[cfg(windows)]
fn connect_agent() -> anyhow::Result<ssh_agent_lib::blocking::Client<std::fs::File>> {
    let pipe_path = std::env::var("BW_PIPE_NAME").unwrap_or_else(|_| DEFAULT_PIPE_NAME.to_string());
    let stream = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pipe_path)
        .with_context(|| format!("failed to connect to SSH agent named pipe {pipe_path}"))?;

    Ok(ssh_agent_lib::blocking::Client::new(stream))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ED25519_PRIVATE_KEY: &str = r#"-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYgAAAJgAIAxdACAM
XQAAAAtzc2gtZWQyNTUxOQAAACCzPq7zfqLffKoBDe/eo04kH2XxtSmk9D7RQyf1xUqrYg
AAAEC2BsIi0QwW2uFscKTUUXNHLsYX4FxlaSDSblbAj7WR7bM+rvN+ot98qgEN796jTiQf
ZfG1KaT0PtFDJ/XFSqtiAAAAEHVzZXJAZXhhbXBsZS5jb20BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----"#;

    const RSA_PRIVATE_KEY: &str = r#"-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAABFwAAAAdzc2gtcn
NhAAAAAwEAAQAAAQEAyng6J3IE5++Ji7EfVNTANDnhYH46LnZW+bwW45etzKswQkc/AvSA
9ih2VAhE8FFUR0Z6pyl4hEn/878x50pGt1FHplbbe4wZ5aornT1hcGGYy313Glt+zyn96M
BTAjO0yULa1RrhBBmeY3yXIEAApUIVdvxcLOvJgltSFmFURtbY5cZkweuspwnHBE/JUPBX
9/Njb+z2R4BTnf0UrudxRKA/TJx9mL3Pb2JjkXfQ07pZqp+oEiUoGMvdfN9vYW4J5LTbXo
n20kRt5UKSxKggBBa0rzGabF+P/BTd39ZrI27WRYhDAzeYJoLq/xfO6qCgAM3TKxe0tDeT
gV4akFJ9CwAAA7hN/dPaTf3T2gAAAAdzc2gtcnNhAAABAQDKeDoncgTn74mLsR9U1MA0Oe
Fgfjoudlb5vBbjl63MqzBCRz8C9ID2KHZUCETwUVRHRnqnKXiESf/zvzHnSka3UUemVtt7
jBnlqiudPWFwYZjLfXcaW37PKf3owFMCM7TJQtrVGuEEGZ5jfJcgQAClQhV2/Fws68mCW1
IWYVRG1tjlxmTB66ynCccET8lQ8Ff382Nv7PZHgFOd/RSu53FEoD9MnH2Yvc9vYmORd9DT
ulmqn6gSJSgYy918329hbgnktNteifbSRG3lQpLEqCAEFrSvMZpsX4/8FN3f1msjbtZFiE
MDN5gmgur/F87qoKAAzdMrF7S0N5OBXhqQUn0LAAAAAwEAAQAAAQAxxSgWdjK6iOl4y0t2
YO32aJv8SksnDLQIo7HEtI5ml1Y/lJ/qrAvfdsbPlVDM+lELTEnuOYWEj2Q5mLA9uMZ1Xa
eNPiCp2CCtkg0yk9oV9AfJTcgvVHpxllLyGgTNr8QrDSIZ7IePqHSE5CWKKfF+riX0n8hQ
yo04XBZrpfU/jDQV8ENKiNQd3Aiy6ppSbnDhyTzZEYIxtvnh1FmvU0Ct1jQRd8p42gurEn
sq6nAPE9pnn0otKmjRdfGCnM9X/ZbUcaUcU/X8pPYG1pW0GZR7eTO+1f9s8TS5LIqz2Eru
L4gBQweASh9mhatsMqJX/ZRrdHvdIuH8N1VDSahf1ZTxAAAAgF1+qA6ZVBEaoCj+fAJZyU
EYf7NMI/nPqEVxiIjg4WKmRYKC9Pb9cuGehOs/XTi3KMEHzYJIKT1K+uO0OG025XVH06qk
9qyWcBwtRbCPVFJPSkKyGBPaUIxMI07x1+434vig6z7iwVROxy3vyhslgiJNpIkaWVUhQN
EGEHX0oWLfAAAAgQDLd25QLAb1kngTsuwQ+xo3S6UcQvOTiDnVRvxWPaW4yn/3qO55+esd
dzxUujFXhUO/POeUJiHv0B1QlDm/sHYL6YVI5+XRaWAst/z0T93mM4ts63Z1OoJbAtE5qH
yGlKVPQ5ZG8SUVElbX+SZE2CcnsPx53trW8qQu/R2bPdDN7QAAAIEA/r7nlgz6D93vMVkn
wq38d49h+PTfyBQ1bum8AhxCEfTaK94YrH9BeizO6Ma5MIjY6WHWbq7Co93J3fl8f4eTCo
CpHJYWfbBqrf/5PUoOIjdMdfFHK6GpUCQNxhbSpnL4l75sxrhkEXtBHVKRXCNR5T4JnOcx
R6qbyo6hPuCiV9cAAAAAAQID
-----END OPENSSH PRIVATE KEY-----"#;

    const ED25519_PUBLIC_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti user@example.com";

    #[test]
    fn sshsig_roundtrip() {
        let private_key = ssh_key::PrivateKey::from_openssh(ED25519_PRIVATE_KEY).unwrap();
        let data = b"hello from git-sign";
        let sshsig = private_key
            .sign(DEFAULT_NAMESPACE, HashAlg::Sha512, data)
            .unwrap();

        let pem = sshsig.to_pem(ssh_key::LineEnding::LF).unwrap();
        let reparsed = SshSig::from_pem(pem).unwrap();

        private_key
            .public_key()
            .verify(DEFAULT_NAMESPACE, data, &reparsed)
            .unwrap();
    }

    #[test]
    fn parses_allowed_signers() {
        let rsa_public_key = ssh_key::PrivateKey::from_openssh(RSA_PRIVATE_KEY)
            .unwrap()
            .public_key()
            .to_openssh()
            .unwrap();
        let mut rsa_parts = rsa_public_key.split_whitespace();
        let _ = rsa_parts.next().unwrap();
        let rsa_blob = rsa_parts.next().unwrap();

        let allowed_signers = parse_allowed_signers(&format!(
            "alice,bob ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti\ncarol rsa-sha2-512 {rsa_blob}\n"
        ))
        .unwrap();

        assert_eq!(allowed_signers.len(), 2);
        assert_eq!(allowed_signers[0].principals, vec!["alice", "bob"]);
        assert_eq!(allowed_signers[1].principals, vec!["carol"]);
        assert_eq!(key_type_string(&allowed_signers[0].public_key), "ED25519");
        assert_eq!(key_type_string(&allowed_signers[1].public_key), "RSA");
    }

    #[test]
    fn key_type_string_mapping() {
        let ed25519 = PublicKey::from_openssh(ED25519_PUBLIC_KEY).unwrap();
        let rsa = ssh_key::PrivateKey::from_openssh(RSA_PRIVATE_KEY)
            .unwrap()
            .public_key()
            .clone();

        assert_eq!(key_type_string(&ed25519), "ED25519");
        assert_eq!(key_type_string(&rsa), "RSA");
    }

    #[test]
    fn match_principals_finds_entries() {
        let rsa_public_key = ssh_key::PrivateKey::from_openssh(RSA_PRIVATE_KEY)
            .unwrap()
            .public_key()
            .to_openssh()
            .unwrap();
        let mut rsa_parts = rsa_public_key.split_whitespace();
        let _ = rsa_parts.next().unwrap();
        let rsa_blob = rsa_parts.next().unwrap();

        let allowed_signers = parse_allowed_signers(&format!(
            "alice,bob ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILM+rvN+ot98qgEN796jTiQfZfG1KaT0PtFDJ/XFSqti\ncarol rsa-sha2-512 {rsa_blob}\n"
        ))
        .unwrap();

        // "alice" matches the first entry, which has principals ["alice", "bob"]
        let matched: Vec<&str> = allowed_signers
            .iter()
            .filter(|entry| entry.principals.iter().any(|p| p == "alice"))
            .flat_map(|entry| entry.principals.iter().map(String::as_str))
            .collect();
        assert_eq!(matched, vec!["alice", "bob"]);

        // "carol" matches the second entry
        let matched: Vec<&str> = allowed_signers
            .iter()
            .filter(|entry| entry.principals.iter().any(|p| p == "carol"))
            .flat_map(|entry| entry.principals.iter().map(String::as_str))
            .collect();
        assert_eq!(matched, vec!["carol"]);

        // "unknown" matches nothing
        let matched: Vec<&str> = allowed_signers
            .iter()
            .filter(|entry| entry.principals.iter().any(|p| p == "unknown"))
            .flat_map(|entry| entry.principals.iter().map(String::as_str))
            .collect();
        assert!(matched.is_empty());
    }
}
