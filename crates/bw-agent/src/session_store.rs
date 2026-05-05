use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Serializable scope for IPC (matches LockMode serde pattern: tagged enum)
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionScope {
    AnyProcess,
    Executable { exe_path: String, exe_hash: Vec<u8> },
}

/// Internal session representation
pub struct ApprovalSession {
    pub id: String,
    pub key_fingerprint: String,
    pub scope: SessionScope,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub usage_count: AtomicU64,
}

/// Serializable session info for IPC
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub key_fingerprint: String,
    pub scope: SessionScope,
    pub created_at_unix: u64,
    pub expires_at_unix: u64,
    pub remaining_secs: u64,
    pub usage_count: u64,
}

/// In-memory session store. Sessions are NEVER persisted.
pub struct SessionStore {
    sessions: Mutex<Vec<ApprovalSession>>,
    /// Cache: exe_path → (mtime, hash). Avoids re-hashing large binaries on every check.
    /// Hit: mtime unchanged → return cached hash. Miss/mtime changed → re-read + re-hash.
    hash_cache: Mutex<HashMap<String, (SystemTime, Vec<u8>)>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(Vec::new()),
            hash_cache: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new session. Returns the session ID.
    pub fn create_session(
        &self,
        key_fingerprint: &str,
        scope: SessionScope,
        duration: Duration,
    ) -> String {
        let now = Instant::now();
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = ApprovalSession {
            id: session_id.clone(),
            key_fingerprint: key_fingerprint.to_string(),
            scope,
            created_at: now,
            expires_at: now + duration,
            usage_count: AtomicU64::new(0),
        };

        log::debug!(
            "Creating approval session {} for fingerprint {}",
            session_id,
            key_fingerprint
        );

        self.lock_sessions().push(session);
        session_id
    }

    /// Check if a matching active session exists for the given fingerprint + exe.
    /// Returns Some(session_id) if auto-approve should proceed, None otherwise.
    /// For Executable scope: re-reads the file and verifies SHA-256 hash matches
    /// (with mtime-based cache to avoid re-hashing large binaries).
    /// Increments usage_count on match.
    /// Lazily cleans up expired sessions.
    pub fn check_session(&self, key_fingerprint: &str, client_exe: &str) -> Option<String> {
        let now = Instant::now();
        let sessions = &mut *self.lock_sessions();

        let before = sessions.len();
        sessions.retain(|session| session.expires_at > now);
        let removed = before.saturating_sub(sessions.len());
        if removed > 0 {
            log::debug!("Removed {removed} expired approval sessions during check");
        }

        for session in sessions.iter() {
            if session.key_fingerprint != key_fingerprint {
                continue;
            }

            let matches = match &session.scope {
                SessionScope::AnyProcess => true,
                SessionScope::Executable { exe_path, exe_hash } => {
                    if exe_path != client_exe {
                        false
                    } else {
                        match self.hash_file_cached(client_exe) {
                            Ok(current_hash) => current_hash == *exe_hash,
                            Err(err) => {
                                log::debug!(
                                    "Failed to hash executable {} for session {}: {err}",
                                    client_exe,
                                    session.id
                                );
                                false
                            }
                        }
                    }
                }
            };

            if matches {
                let count = session.usage_count.fetch_add(1, Ordering::Relaxed) + 1;
                log::debug!(
                    "Approval session {} matched fingerprint {} (usage_count={count})",
                    session.id,
                    key_fingerprint
                );
                return Some(session.id.clone());
            }
        }

        log::debug!(
            "No approval session matched fingerprint {} for executable {}",
            key_fingerprint,
            client_exe
        );
        None
    }

    /// Revoke a specific session. Returns true if found and removed.
    pub fn revoke_session(&self, session_id: &str) -> bool {
        let sessions = &mut *self.lock_sessions();
        let before = sessions.len();
        sessions.retain(|session| session.id != session_id);
        let removed = before != sessions.len();

        if removed {
            log::debug!("Revoked approval session {session_id}");
        } else {
            log::debug!("Approval session {session_id} not found during revoke");
        }

        removed
    }

    /// Revoke all sessions (called on vault lock).
    pub fn revoke_all(&self) {
        let sessions = &mut *self.lock_sessions();
        let count = sessions.len();
        sessions.clear();
        self.hash_cache.lock().unwrap().clear();
        log::debug!("Revoked all approval sessions ({count} removed, hash cache cleared)");
    }

    /// List all active (non-expired) sessions as serializable info.
    /// Lazily cleans up expired sessions.
    pub fn list_active(&self) -> Vec<SessionInfo> {
        let now = Instant::now();
        let now_unix = unix_now_secs();
        let sessions = &mut *self.lock_sessions();

        let before = sessions.len();
        sessions.retain(|session| session.expires_at > now);
        let removed = before.saturating_sub(sessions.len());
        if removed > 0 {
            log::debug!("Removed {removed} expired approval sessions during list");
        }

        sessions
            .iter()
            .map(|session| SessionInfo {
                id: session.id.clone(),
                key_fingerprint: session.key_fingerprint.clone(),
                scope: session.scope.clone(),
                created_at_unix: instant_to_unix_secs(now, now_unix, session.created_at),
                expires_at_unix: instant_to_unix_secs(now, now_unix, session.expires_at),
                remaining_secs: session.expires_at.saturating_duration_since(now).as_secs(),
                usage_count: session.usage_count.load(Ordering::Relaxed),
            })
            .collect()
    }

    fn lock_sessions(&self) -> MutexGuard<'_, Vec<ApprovalSession>> {
        self.sessions.lock().unwrap_or_else(|e| {
            log::warn!("Session store mutex was poisoned, recovering");
            e.into_inner()
        })
    }

    /// Hash a file with mtime-based cache. Returns cached hash if the file's
    /// modification time hasn't changed since last hash. Otherwise re-reads
    /// and re-hashes the file, updating the cache.
    pub fn hash_file_cached(&self, path: &str) -> std::io::Result<Vec<u8>> {
        let metadata = std::fs::metadata(path)?;
        let mtime = metadata.modified()?;

        {
            let cache = self.hash_cache.lock().unwrap();
            if let Some((cached_mtime, cached_hash)) = cache.get(path) {
                if *cached_mtime == mtime {
                    return Ok(cached_hash.clone());
                }
            }
        }

        let hash = hash_file(path)?;
        self.hash_cache
            .lock()
            .unwrap()
            .insert(path.to_string(), (mtime, hash.clone()));
        Ok(hash)
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute SHA-256 hash of a file. Returns Vec<u8> (32 bytes).
pub fn hash_file(path: &str) -> std::io::Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(hasher.finalize().to_vec())
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn instant_to_unix_secs(
    reference_instant: Instant,
    reference_unix_secs: u64,
    target: Instant,
) -> u64 {
    if target >= reference_instant {
        reference_unix_secs.saturating_add(target.duration_since(reference_instant).as_secs())
    } else {
        reference_unix_secs.saturating_sub(reference_instant.duration_since(target).as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::thread;

    fn temp_file_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "bw-agent-session-store-{name}-{}",
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn test_create_and_check_any_process() {
        let store = SessionStore::new();
        let session_id = store.create_session(
            "SHA256:any",
            SessionScope::AnyProcess,
            Duration::from_secs(60),
        );

        let matched =
            store.check_session("SHA256:any", "C:\\Program Files\\Git\\usr\\bin\\ssh.exe");

        assert_eq!(matched, Some(session_id));
    }

    #[test]
    fn test_create_and_check_executable() {
        let store = SessionStore::new();
        let exe_path = temp_file_path("executable");
        fs::write(&exe_path, b"ssh-client-binary").unwrap();

        let exe_path_str = exe_path.to_string_lossy().into_owned();
        let exe_hash = hash_file(&exe_path_str).unwrap();
        let session_id = store.create_session(
            "SHA256:exe",
            SessionScope::Executable {
                exe_path: exe_path_str.clone(),
                exe_hash,
            },
            Duration::from_secs(60),
        );

        let matched = store.check_session("SHA256:exe", &exe_path_str);

        assert_eq!(matched, Some(session_id));
        fs::remove_file(exe_path).unwrap();
    }

    #[test]
    fn test_session_expiry() {
        let store = SessionStore::new();
        store.create_session(
            "SHA256:expired",
            SessionScope::AnyProcess,
            Duration::from_millis(10),
        );

        thread::sleep(Duration::from_millis(30));

        assert_eq!(store.check_session("SHA256:expired", "ssh.exe"), None);
    }

    #[test]
    fn test_scope_mismatch_fingerprint() {
        let store = SessionStore::new();
        store.create_session(
            "SHA256:expected",
            SessionScope::AnyProcess,
            Duration::from_secs(60),
        );

        assert_eq!(store.check_session("SHA256:other", "ssh.exe"), None);
    }

    #[test]
    fn test_scope_mismatch_exe() {
        let store = SessionStore::new();
        let expected_exe = temp_file_path("expected-exe");
        let other_exe = temp_file_path("other-exe");
        fs::write(&expected_exe, b"expected").unwrap();
        fs::write(&other_exe, b"other").unwrap();

        let expected_exe_str = expected_exe.to_string_lossy().into_owned();
        let other_exe_str = other_exe.to_string_lossy().into_owned();

        store.create_session(
            "SHA256:exe-mismatch",
            SessionScope::Executable {
                exe_path: expected_exe_str.clone(),
                exe_hash: hash_file(&expected_exe_str).unwrap(),
            },
            Duration::from_secs(60),
        );

        assert_eq!(
            store.check_session("SHA256:exe-mismatch", &other_exe_str),
            None
        );

        fs::remove_file(expected_exe).unwrap();
        fs::remove_file(other_exe).unwrap();
    }

    #[test]
    fn test_hash_mismatch_rejects() {
        let store = SessionStore::new();
        let exe_path = temp_file_path("hash-mismatch");
        fs::write(&exe_path, b"original-binary").unwrap();

        let exe_path_str = exe_path.to_string_lossy().into_owned();
        store.create_session(
            "SHA256:hash-mismatch",
            SessionScope::Executable {
                exe_path: exe_path_str.clone(),
                exe_hash: hash_file(&exe_path_str).unwrap(),
            },
            Duration::from_secs(60),
        );

        fs::write(&exe_path, b"modified-binary").unwrap();

        assert_eq!(
            store.check_session("SHA256:hash-mismatch", &exe_path_str),
            None
        );
        fs::remove_file(exe_path).unwrap();
    }

    #[test]
    fn test_revoke_session() {
        let store = SessionStore::new();
        let session_id = store.create_session(
            "SHA256:revoke-one",
            SessionScope::AnyProcess,
            Duration::from_secs(60),
        );

        assert!(store.revoke_session(&session_id));
        assert_eq!(store.check_session("SHA256:revoke-one", "ssh.exe"), None);
    }

    #[test]
    fn test_revoke_all() {
        let store = SessionStore::new();
        store.create_session(
            "SHA256:one",
            SessionScope::AnyProcess,
            Duration::from_secs(60),
        );
        store.create_session(
            "SHA256:two",
            SessionScope::AnyProcess,
            Duration::from_secs(60),
        );

        store.revoke_all();

        assert!(store.list_active().is_empty());
    }

    #[test]
    fn test_list_active_cleans_expired() {
        let store = SessionStore::new();
        store.create_session(
            "SHA256:expired-list",
            SessionScope::AnyProcess,
            Duration::from_millis(10),
        );
        store.create_session(
            "SHA256:active-list",
            SessionScope::AnyProcess,
            Duration::from_secs(60),
        );

        thread::sleep(Duration::from_millis(30));

        let active = store.list_active();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].key_fingerprint, "SHA256:active-list");
    }

    #[test]
    fn test_usage_count_increments() {
        let store = SessionStore::new();
        let session_id = store.create_session(
            "SHA256:usage",
            SessionScope::AnyProcess,
            Duration::from_secs(60),
        );

        assert_eq!(
            store.check_session("SHA256:usage", "ssh.exe"),
            Some(session_id.clone())
        );
        assert_eq!(
            store.check_session("SHA256:usage", "ssh.exe"),
            Some(session_id)
        );

        let sessions = store.list_active();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].usage_count, 2);
    }

    #[test]
    fn test_hash_file_missing() {
        let missing = temp_file_path("missing");
        let missing_str = missing.to_string_lossy().into_owned();

        assert!(hash_file(&missing_str).is_err());
    }

    #[test]
    fn test_hash_cache_uses_mtime() {
        let store = SessionStore::new();
        let exe_path = temp_file_path("cache-test");
        fs::write(&exe_path, b"original").unwrap();

        let exe_str = exe_path.to_string_lossy().into_owned();

        // First call — computes hash
        let hash1 = store.hash_file_cached(&exe_str).unwrap();
        assert_eq!(hash1, hash_file(&exe_str).unwrap());

        // Same mtime, within TTL — should return cached (same result)
        let hash2 = store.hash_file_cached(&exe_str).unwrap();
        assert_eq!(hash1, hash2);

        // Modify file — mtime changes, should rehash
        thread::sleep(Duration::from_millis(10));
        fs::write(&exe_path, b"modified").unwrap();
        let hash3 = store.hash_file_cached(&exe_str).unwrap();
        assert_ne!(hash1, hash3);
        assert_eq!(hash3, hash_file(&exe_str).unwrap());

        fs::remove_file(exe_path).unwrap();
    }

    #[test]
    fn test_revoke_all_clears_hash_cache() {
        let store = SessionStore::new();
        let exe_path = temp_file_path("cache-clear");
        fs::write(&exe_path, b"binary").unwrap();

        let exe_str = exe_path.to_string_lossy().into_owned();
        store.hash_file_cached(&exe_str).unwrap();

        // Cache should have an entry
        assert!(store.hash_cache.lock().unwrap().contains_key(&exe_str));

        store.revoke_all();

        // Cache should be empty after revoke_all
        assert!(store.hash_cache.lock().unwrap().is_empty());

        fs::remove_file(exe_path).unwrap();
    }
}
