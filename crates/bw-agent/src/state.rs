use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Cached authentication state with TTL.
pub struct State {
    /// Decrypted vault keys (user key).
    pub keys: Option<bw_core::locked::Keys>,
    /// Decrypted organization keys.
    pub org_keys: Option<HashMap<String, bw_core::locked::Keys>>,
    /// When the keys were last cached.
    pub cached_at: Option<Instant>,
    /// How long cached keys remain valid. `None` = no time-based expiry
    /// (lock is triggered by external events or "never" mode).
    pub cache_ttl: Option<Duration>,
    /// Cached vault entries (encrypted cipherstrings).
    pub entries: Vec<bw_core::db::Entry>,
    /// Login session data for token refresh.
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    /// KDF parameters (needed for re-unlock).
    pub email: Option<String>,
    pub kdf: Option<bw_core::api::KdfType>,
    pub iterations: Option<u32>,
    pub memory: Option<u32>,
    pub parallelism: Option<u32>,
    pub protected_key: Option<String>,
    pub protected_private_key: Option<String>,
    pub protected_org_keys: HashMap<String, String>,
}

impl State {
    pub fn new(cache_ttl: Option<Duration>) -> Self {
        Self {
            keys: None,
            org_keys: None,
            cached_at: None,
            cache_ttl,
            entries: Vec::new(),
            access_token: None,
            refresh_token: None,
            email: None,
            kdf: None,
            iterations: None,
            memory: None,
            parallelism: None,
            protected_key: None,
            protected_private_key: None,
            protected_org_keys: HashMap::new(),
        }
    }

    /// Check if keys are cached and still valid.
    pub fn is_unlocked(&self) -> bool {
        match (&self.keys, &self.cached_at) {
            (Some(_), Some(cached_at)) => {
                // If cache_ttl is None, keys never expire by time (event-based / never mode).
                self.cache_ttl.is_none_or(|ttl| cached_at.elapsed() < ttl)
            }
            _ => false,
        }
    }

    /// Returns true when keys were previously unlocked but have since expired
    /// due to TTL. Distinguishes genuine expiry from "never unlocked yet".
    pub fn is_expired(&self) -> bool {
        match (&self.keys, &self.cached_at, self.cache_ttl) {
            // Keys present, cached_at set, TTL set, and elapsed >= TTL → expired
            (Some(_), Some(cached_at), Some(ttl)) => cached_at.elapsed() >= ttl,
            _ => false,
        }
    }

    /// Get the decryption key for a given org_id (or the user key if None).
    pub fn key(&self, org_id: Option<&str>) -> Option<&bw_core::locked::Keys> {
        org_id.map_or(self.keys.as_ref(), |id| {
            self.org_keys.as_ref().and_then(|keys| keys.get(id))
        })
    }

    /// Clear all cached keys (lock).
    pub fn clear(&mut self) {
        self.keys = None;
        self.org_keys = None;
        self.cached_at = None;
    }

    /// Store unlocked keys and refresh the TTL.
    pub fn set_unlocked(
        &mut self,
        keys: bw_core::locked::Keys,
        org_keys: HashMap<String, bw_core::locked::Keys>,
    ) {
        self.keys = Some(keys);
        self.org_keys = Some(org_keys);
        self.cached_at = Some(Instant::now());
    }

    /// Refresh the TTL (called on each successful SSH operation).
    pub fn touch(&mut self) {
        if self.keys.is_some() {
            self.cached_at = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_keys() -> bw_core::locked::Keys {
        let mut v = bw_core::locked::Vec::new();
        v.extend(std::iter::repeat_n(0u8, 64));
        bw_core::locked::Keys::new(v)
    }

    #[test]
    fn test_new_state_is_locked() {
        let state = State::new(Some(Duration::from_secs(900)));
        assert!(!state.is_unlocked());
        assert!(state.key(None).is_none());
    }

    #[test]
    fn test_unlock_and_check() {
        let mut state = State::new(Some(Duration::from_secs(900)));
        state.set_unlocked(dummy_keys(), HashMap::new());
        assert!(state.is_unlocked());
        assert!(state.key(None).is_some());
        std::mem::forget(state);
    }

    #[test]
    fn test_cache_expires() {
        let mut state = State::new(Some(Duration::from_millis(1)));
        state.set_unlocked(dummy_keys(), HashMap::new());
        std::thread::sleep(Duration::from_millis(10));
        assert!(!state.is_unlocked());
        std::mem::forget(state);
    }

    #[test]
    fn test_clear_locks() {
        let mut state = State::new(Some(Duration::from_secs(900)));
        state.set_unlocked(dummy_keys(), HashMap::new());
        state.clear();
        assert!(!state.is_unlocked());
        assert!(state.key(None).is_none());
    }

    #[test]
    fn test_touch_refreshes_ttl() {
        let mut state = State::new(Some(Duration::from_secs(900)));
        state.set_unlocked(dummy_keys(), HashMap::new());
        std::thread::sleep(Duration::from_millis(10));
        state.touch();
        assert!(state.is_unlocked());
        std::mem::forget(state);
    }

    #[test]
    fn test_no_ttl_never_expires() {
        let mut state = State::new(None);
        state.set_unlocked(dummy_keys(), HashMap::new());
        // Even after some time, should still be unlocked (no TTL)
        std::thread::sleep(Duration::from_millis(10));
        assert!(state.is_unlocked());
        std::mem::forget(state);
    }
}
