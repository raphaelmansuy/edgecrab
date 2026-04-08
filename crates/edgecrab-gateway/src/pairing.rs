//! # DM Pairing — code-based approval flow for new gateway users
//!
//! WHY pairing: Instead of static allowlists with user IDs, unknown users
//! receive a one-time pairing code that the bot owner approves via the CLI.
//! This is more secure and user-friendly than maintaining ID lists.
//!
//! Mirrors hermes-agent's `gateway/pairing.py`:
//! - 8-char codes from unambiguous alphabet (no 0/O/1/I)
//! - Cryptographic randomness via `rand::rngs::OsRng`
//! - 1-hour code expiry
//! - Max 3 pending codes per platform
//! - Rate limiting: 1 request per user per 10 minutes
//! - Lockout after 5 failed approval attempts (1 hour)
//! - File permissions: mode 0600 on data files (Unix)
//!
//! Storage: `~/.edgecrab/pairing/`

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Unambiguous alphabet — excludes 0/O, 1/I to prevent confusion.
const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LENGTH: usize = 8;

// Timing constants
const CODE_TTL_SECS: f64 = 3600.0; // 1 hour
const RATE_LIMIT_SECS: f64 = 600.0; // 10 minutes
const LOCKOUT_SECS: f64 = 3600.0; // 1 hour lock after too many failures

// Limits
const MAX_PENDING_PER_PLATFORM: usize = 3;
const MAX_FAILED_ATTEMPTS: u32 = 5;

fn pairing_dir() -> PathBuf {
    edgecrab_core::edgecrab_home().join("pairing")
}

fn now_epoch() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingEntry {
    user_id: String,
    #[serde(default)]
    user_name: String,
    created_at: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApprovedEntry {
    #[serde(default)]
    user_name: String,
    approved_at: f64,
}

/// Manages pairing codes and approved user lists.
pub struct PairingStore;

impl Default for PairingStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PairingStore {
    pub fn new() -> Self {
        let dir = pairing_dir();
        let _ = std::fs::create_dir_all(&dir);
        Self
    }

    // ── File helpers ──

    fn pending_path(platform: &str) -> PathBuf {
        pairing_dir().join(format!("{platform}-pending.json"))
    }

    fn approved_path(platform: &str) -> PathBuf {
        pairing_dir().join(format!("{platform}-approved.json"))
    }

    fn rate_limit_path() -> PathBuf {
        pairing_dir().join("_rate_limits.json")
    }

    fn load_json<T: serde::de::DeserializeOwned + Default>(path: &PathBuf) -> T {
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => T::default(),
        }
    }

    fn save_json<T: serde::Serialize>(path: &PathBuf, data: &T) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(data) {
            let _ = std::fs::write(path, json);
            // Set restrictive permissions on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    // ── Approved users ──

    /// Check if a user is approved (paired) on a platform.
    pub fn is_approved(&self, platform: &str, user_id: &str) -> bool {
        let approved: HashMap<String, ApprovedEntry> =
            Self::load_json(&Self::approved_path(platform));
        approved.contains_key(user_id)
    }

    /// List approved users, optionally filtered by platform.
    pub fn list_approved(&self, platform: Option<&str>) -> Vec<(String, String, String)> {
        let mut results = Vec::new();
        let platforms = match platform {
            Some(p) => vec![p.to_string()],
            None => self.all_platforms("approved"),
        };
        for p in platforms {
            let approved: HashMap<String, ApprovedEntry> =
                Self::load_json(&Self::approved_path(&p));
            for (uid, info) in approved {
                results.push((p.clone(), uid, info.user_name));
            }
        }
        results
    }

    fn approve_user(&self, platform: &str, user_id: &str, user_name: &str) {
        let path = Self::approved_path(platform);
        let mut approved: HashMap<String, ApprovedEntry> = Self::load_json(&path);
        approved.insert(
            user_id.to_string(),
            ApprovedEntry {
                user_name: user_name.to_string(),
                approved_at: now_epoch(),
            },
        );
        Self::save_json(&path, &approved);
    }

    /// Revoke approval for a user. Returns true if the user was found.
    pub fn revoke(&self, platform: &str, user_id: &str) -> bool {
        let path = Self::approved_path(platform);
        let mut approved: HashMap<String, ApprovedEntry> = Self::load_json(&path);
        if approved.remove(user_id).is_some() {
            Self::save_json(&path, &approved);
            true
        } else {
            false
        }
    }

    // ── Pending codes ──

    /// Generate a pairing code for a new user.
    /// Returns `None` if rate-limited, locked out, or max pending reached.
    pub fn generate_code(&self, platform: &str, user_id: &str, user_name: &str) -> Option<String> {
        self.cleanup_expired(platform);

        if self.is_locked_out(platform) {
            return None;
        }
        if self.is_rate_limited(platform, user_id) {
            return None;
        }

        let path = Self::pending_path(platform);
        let mut pending: HashMap<String, PendingEntry> = Self::load_json(&path);

        if pending.len() >= MAX_PENDING_PER_PLATFORM {
            return None;
        }

        // Generate cryptographically random code
        let mut rng = rand::rng();
        let code: String = (0..CODE_LENGTH)
            .map(|_| {
                let idx = rng.random_range(0..ALPHABET.len());
                ALPHABET[idx] as char
            })
            .collect();

        pending.insert(
            code.clone(),
            PendingEntry {
                user_id: user_id.to_string(),
                user_name: user_name.to_string(),
                created_at: now_epoch(),
            },
        );
        Self::save_json(&path, &pending);
        self.record_rate_limit(platform, user_id);

        Some(code)
    }

    /// Approve a pairing code. Returns `(user_id, user_name)` on success.
    pub fn approve_code(&self, platform: &str, code: &str) -> Option<(String, String)> {
        self.cleanup_expired(platform);
        let code = code.to_uppercase();
        let code = code.trim();

        let path = Self::pending_path(platform);
        let mut pending: HashMap<String, PendingEntry> = Self::load_json(&path);

        match pending.remove(code) {
            Some(entry) => {
                Self::save_json(&path, &pending);
                self.approve_user(platform, &entry.user_id, &entry.user_name);
                Some((entry.user_id, entry.user_name))
            }
            None => {
                self.record_failed_attempt(platform);
                None
            }
        }
    }

    /// List pending pairing requests.
    pub fn list_pending(
        &self,
        platform: Option<&str>,
    ) -> Vec<(String, String, String, String, u64)> {
        let mut results = Vec::new();
        let platforms = match platform {
            Some(p) => vec![p.to_string()],
            None => self.all_platforms("pending"),
        };
        for p in platforms {
            self.cleanup_expired(&p);
            let pending: HashMap<String, PendingEntry> = Self::load_json(&Self::pending_path(&p));
            for (code, info) in pending {
                let age_min = ((now_epoch() - info.created_at) / 60.0) as u64;
                results.push((p.clone(), code, info.user_id, info.user_name, age_min));
            }
        }
        results
    }

    // ── Rate limiting ──

    fn is_rate_limited(&self, platform: &str, user_id: &str) -> bool {
        let limits: HashMap<String, f64> = Self::load_json(&Self::rate_limit_path());
        let key = format!("{platform}:{user_id}");
        match limits.get(&key) {
            Some(&last_request) => (now_epoch() - last_request) < RATE_LIMIT_SECS,
            None => false,
        }
    }

    fn record_rate_limit(&self, platform: &str, user_id: &str) {
        let path = Self::rate_limit_path();
        let mut limits: HashMap<String, f64> = Self::load_json(&path);
        limits.insert(format!("{platform}:{user_id}"), now_epoch());
        Self::save_json(&path, &limits);
    }

    fn is_locked_out(&self, platform: &str) -> bool {
        let limits: HashMap<String, f64> = Self::load_json(&Self::rate_limit_path());
        let key = format!("_lockout:{platform}");
        match limits.get(&key) {
            Some(&lockout_until) => now_epoch() < lockout_until,
            None => false,
        }
    }

    fn record_failed_attempt(&self, platform: &str) {
        let path = Self::rate_limit_path();
        let mut limits: HashMap<String, f64> = Self::load_json(&path);
        let fail_key = format!("_failures:{platform}");
        let fails = limits.get(&fail_key).copied().unwrap_or(0.0) as u32 + 1;
        limits.insert(fail_key.clone(), fails as f64);
        if fails >= MAX_FAILED_ATTEMPTS {
            let lockout_key = format!("_lockout:{platform}");
            limits.insert(lockout_key, now_epoch() + LOCKOUT_SECS);
            limits.insert(fail_key, 0.0);
            tracing::warn!(
                platform,
                "platform locked out after {} failed pairing attempts",
                MAX_FAILED_ATTEMPTS
            );
        }
        Self::save_json(&path, &limits);
    }

    // ── Cleanup ──

    fn cleanup_expired(&self, platform: &str) {
        let path = Self::pending_path(platform);
        let mut pending: HashMap<String, PendingEntry> = Self::load_json(&path);
        let now = now_epoch();
        let before = pending.len();
        pending.retain(|_, entry| (now - entry.created_at) <= CODE_TTL_SECS);
        if pending.len() != before {
            Self::save_json(&path, &pending);
        }
    }

    fn all_platforms(&self, suffix: &str) -> Vec<String> {
        let dir = pairing_dir();
        let mut platforms = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(stripped) = name.strip_suffix(&format!("-{suffix}.json")) {
                    if !stripped.starts_with('_') {
                        platforms.push(stripped.to_string());
                    }
                }
            }
        }
        platforms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn code_generation_length() {
        // Just test the alphabet and code length constants
        assert_eq!(ALPHABET.len(), 32);
        assert_eq!(CODE_LENGTH, 8);
    }

    #[test]
    fn code_excludes_ambiguous_chars() {
        let alpha = std::str::from_utf8(ALPHABET).expect("pairing alphabet should be valid UTF-8");
        assert!(!alpha.contains('0'));
        assert!(!alpha.contains('O'));
        assert!(!alpha.contains('1'));
        assert!(!alpha.contains('I'));
    }

    #[test]
    fn pairing_store_uses_edgecrab_home() {
        let dir = tempdir().expect("tempdir");
        unsafe {
            std::env::set_var("EDGECRAB_HOME", dir.path());
        }

        let store = PairingStore::new();
        let code = store
            .generate_code("telegram", "123456", "alice")
            .expect("pairing code");

        assert_eq!(code.len(), CODE_LENGTH);
        assert!(
            dir.path()
                .join("pairing")
                .join("telegram-pending.json")
                .exists()
        );

        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }
}
