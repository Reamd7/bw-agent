use std::path::PathBuf;

/// How the vault should be locked.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LockMode {
    /// Lock after `seconds` since last SSH operation (TTL-based).
    Timeout { seconds: u64 },
    /// Lock when the OS reports no user input for `seconds`.
    SystemIdle { seconds: u64 },
    /// Lock when the system goes to sleep / suspends.
    OnSleep,
    /// Lock when the OS session is locked (Win+L / screen lock).
    OnLock,
    /// Lock on system restart / shutdown.
    OnRestart,
    /// Never lock automatically.
    Never,
}

impl Default for LockMode {
    fn default() -> Self {
        Self::Timeout { seconds: 900 }
    }
}

impl LockMode {
    /// Duration-based TTL for the `State` cache, if applicable.
    /// Returns `None` for event-only and never modes (no time-based expiry).
    pub fn cache_ttl(&self) -> Option<std::time::Duration> {
        match self {
            Self::Timeout { seconds } => Some(std::time::Duration::from_secs(*seconds)),
            _ => None,
        }
    }

    /// Idle threshold for the `SystemIdle` mode, if applicable.
    pub fn idle_threshold(&self) -> Option<std::time::Duration> {
        match self {
            Self::SystemIdle { seconds } => Some(std::time::Duration::from_secs(*seconds)),
            _ => None,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default)]
pub struct Config {
    pub email: Option<String>,
    pub base_url: Option<String>,
    pub identity_url: Option<String>,

    /// How the vault should be locked.
    #[serde(default)]
    pub lock_mode: LockMode,

    /// Legacy field — kept for backward-compat deserialization only.
    /// If `lock_mode` is absent in JSON but `lock_timeout` is present, we migrate.
    #[serde(default, skip_serializing)]
    lock_timeout: Option<u64>,

    pub proxy: Option<String>,
}

impl Config {
    /// Load config from file, falling back to defaults if file doesn't exist.
    /// Handles migration from the old `lock_timeout` (u64 seconds) field.
    pub fn load() -> Self {
        let file = config_file_path();
        let mut config: Self = match std::fs::read_to_string(&file) {
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
        };

        // Migrate legacy lock_timeout → lock_mode if lock_mode was not explicitly set.
        // Serde will have set lock_mode to default (Timeout 900) when the field was absent,
        // so we detect migration by checking if the old field was present.
        if let Some(old_timeout) = config.lock_timeout.take() {
            // Only migrate if lock_mode is still at its default value (meaning it wasn't in JSON).
            if config.lock_mode == LockMode::default() {
                config.lock_mode = if old_timeout == 0 {
                    LockMode::Never
                } else {
                    LockMode::Timeout {
                        seconds: old_timeout,
                    }
                };
                log::info!(
                    "Migrated legacy lock_timeout={old_timeout} → lock_mode={:?}",
                    config.lock_mode
                );
            }
        }

        config
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
                self.lock_mode = LockMode::Timeout { seconds: ttl };
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
        assert_eq!(config.lock_mode, LockMode::Timeout { seconds: 900 });
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
            lock_mode: LockMode::Timeout { seconds: 600 },
            lock_timeout: None,
            proxy: Some("http://127.0.0.1:7890".to_string()),
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.email, config.email);
        assert_eq!(deserialized.base_url, config.base_url);
        assert_eq!(deserialized.lock_mode, LockMode::Timeout { seconds: 600 });
    }

    #[test]
    fn test_lock_mode_cache_ttl() {
        assert_eq!(
            LockMode::Timeout { seconds: 300 }.cache_ttl(),
            Some(std::time::Duration::from_secs(300))
        );
        assert_eq!(LockMode::Never.cache_ttl(), None);
        assert_eq!(LockMode::OnSleep.cache_ttl(), None);
        assert_eq!(LockMode::OnLock.cache_ttl(), None);
        assert_eq!(LockMode::OnRestart.cache_ttl(), None);
        assert_eq!(LockMode::SystemIdle { seconds: 60 }.cache_ttl(), None);
    }

    #[test]
    fn test_lock_mode_idle_threshold() {
        assert_eq!(
            LockMode::SystemIdle { seconds: 120 }.idle_threshold(),
            Some(std::time::Duration::from_secs(120))
        );
        assert_eq!(LockMode::Timeout { seconds: 300 }.idle_threshold(), None);
        assert_eq!(LockMode::Never.idle_threshold(), None);
    }

    #[test]
    fn test_legacy_lock_timeout_migration() {
        // Simulate old config JSON with lock_timeout but no lock_mode
        let json = r#"{"email":"test@test.com","lock_timeout":600}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        // After deserialization, lock_timeout is Some(600) and lock_mode is default.
        // Migration happens in load(), not in deserialization.
        // But we can test the field is captured:
        assert_eq!(config.lock_timeout, Some(600));
    }

    #[test]
    fn test_lock_mode_serialization() {
        let config = Config {
            lock_mode: LockMode::OnSleep,
            ..Config::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""type":"on_sleep"#));
        // lock_timeout should NOT be serialized
        assert!(!json.contains("lock_timeout"));
    }

    #[test]
    fn test_lock_mode_deserialization_variants() {
        let json = r#"{"type":"system_idle","seconds":120}"#;
        let mode: LockMode = serde_json::from_str(json).unwrap();
        assert_eq!(mode, LockMode::SystemIdle { seconds: 120 });

        let json = r#"{"type":"never"}"#;
        let mode: LockMode = serde_json::from_str(json).unwrap();
        assert_eq!(mode, LockMode::Never);

        let json = r#"{"type":"on_lock"}"#;
        let mode: LockMode = serde_json::from_str(json).unwrap();
        assert_eq!(mode, LockMode::OnLock);
    }
}
