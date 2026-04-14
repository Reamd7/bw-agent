use std::path::PathBuf;

const DEFAULT_LOCK_TIMEOUT: u64 = 900;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Config {
    pub email: Option<String>,
    pub base_url: Option<String>,
    pub identity_url: Option<String>,
    #[serde(default = "default_lock_timeout")]
    pub lock_timeout: u64,
    pub proxy: Option<String>,
}

fn default_lock_timeout() -> u64 {
    DEFAULT_LOCK_TIMEOUT
}

impl Default for Config {
    fn default() -> Self {
        Self {
            email: None,
            base_url: None,
            identity_url: None,
            lock_timeout: DEFAULT_LOCK_TIMEOUT,
            proxy: None,
        }
    }
}

impl Config {
    /// Load config from file, falling back to defaults if file doesn't exist.
    pub fn load() -> Self {
        let file = config_file_path();
        match std::fs::read_to_string(&file) {
            Ok(json) => match serde_json::from_str(&json) {
                Ok(config) => {
                    log::info!("Loaded config from {}", file.display());
                    config
                }
                Err(e) => {
                    log::warn!("Failed to parse config {}: {e}", file.display());
                    Self::default()
                }
            },
            Err(_) => {
                log::info!("No config file found at {}, using defaults", file.display());
                Self::default()
            }
        }
    }

    /// Apply environment variable overrides. Env vars take priority over config file.
    pub fn apply_env_overrides(&mut self) {
        if let Ok(v) = std::env::var("BW_EMAIL") {
            self.email = Some(v);
        }
        if let Ok(v) = std::env::var("BW_BASE_URL") {
            self.base_url = Some(v);
        }
        if let Ok(v) = std::env::var("BW_IDENTITY_URL") {
            self.identity_url = Some(v);
        }
        if let Ok(v) = std::env::var("BW_PROXY") {
            self.proxy = if v.is_empty() { None } else { Some(v) };
        }
        if let Ok(v) = std::env::var("BW_CACHE_TTL") {
            if let Ok(ttl) = v.parse::<u64>() {
                self.lock_timeout = ttl;
            }
        }
    }

    /// Derive the API base URL (like rbw).
    /// - None → "https://api.bitwarden.com"
    /// - Some("https://vault.example.com") → "https://vault.example.com/api"
    pub fn api_url(&self) -> String {
        self.base_url.as_ref().map_or_else(
            || "https://api.bitwarden.com".to_string(),
            |url| {
                let clean = url.trim_end_matches('/');
                format!("{clean}/api")
            },
        )
    }

    /// Derive the identity URL (like rbw).
    /// - Explicit identity_url → use it
    /// - Else derive from base_url
    /// - Neither set → "https://identity.bitwarden.com"
    pub fn identity_url(&self) -> String {
        if let Some(url) = &self.identity_url {
            return url.clone();
        }
        self.base_url.as_ref().map_or_else(
            || "https://identity.bitwarden.com".to_string(),
            |url| {
                let clean = url.trim_end_matches('/');
                format!("{clean}/identity")
            },
        )
    }

    /// Validate that required fields are present.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.email.is_none() {
            anyhow::bail!(
                "Email not configured. Set BW_EMAIL env var or add \"email\" to {}",
                config_file_path().display()
            );
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.api_url(), "https://api.bitwarden.com");
        assert_eq!(config.identity_url(), "https://identity.bitwarden.com");
        assert_eq!(config.lock_timeout, 900);
    }

    #[test]
    fn test_custom_base_url_derives_api_and_identity() {
        let config = Config {
            base_url: Some("https://vault.example.com".to_string()),
            ..Config::default()
        };
        assert_eq!(config.api_url(), "https://vault.example.com/api");
        assert_eq!(config.identity_url(), "https://vault.example.com/identity");
    }

    #[test]
    fn test_trailing_slash_stripped() {
        let config = Config {
            base_url: Some("https://vault.example.com/".to_string()),
            ..Config::default()
        };
        assert_eq!(config.api_url(), "https://vault.example.com/api");
        assert_eq!(config.identity_url(), "https://vault.example.com/identity");
    }

    #[test]
    fn test_explicit_identity_url_overrides_derived() {
        let config = Config {
            base_url: Some("https://vault.example.com".to_string()),
            identity_url: Some("https://id.custom.com".to_string()),
            ..Config::default()
        };
        assert_eq!(config.api_url(), "https://vault.example.com/api");
        assert_eq!(config.identity_url(), "https://id.custom.com");
    }

    #[test]
    fn test_validate_fails_without_email() {
        let config = Config::default();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_passes_with_email() {
        let config = Config {
            email: Some("user@example.com".to_string()),
            ..Config::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_roundtrip_json() {
        let config = Config {
            email: Some("test@test.com".to_string()),
            base_url: Some("https://vault.example.com".to_string()),
            identity_url: None,
            lock_timeout: 600,
            proxy: Some("http://127.0.0.1:7890".to_string()),
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.email, config.email);
        assert_eq!(deserialized.base_url, config.base_url);
        assert_eq!(deserialized.lock_timeout, 600);
    }
}
