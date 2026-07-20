//! Lightweight API-key credential pool (spec 022-014 wave-2 / Hermes credential_pool).
//!
//! SOLID: pure rotation policy — does not construct LLM providers.
//! Callers apply the selected key (env or rebuild) after [`CredentialPool::mark_exhausted_and_rotate`].
//!
//! Config / env:
//! - `EDGECRAB_API_KEY_POOL` — comma-separated keys for a logical pool name
//! - or `EDGECRAB_API_KEY_POOL_<PROVIDER>` (uppercased, `-` → `_`)
//! - optional `EDGECRAB_API_KEY_POOL_ENV` — which env var to write the active key into
//!   (default: leave keys in pool only; caller decides)

use std::collections::HashMap;
use std::sync::Mutex;

/// One pooled credential entry.
#[derive(Debug, Clone)]
pub struct PooledCredential {
    pub label: String,
    pub token: String,
    pub exhausted: bool,
}

/// In-process credential pool keyed by provider/logical name.
#[derive(Debug, Default)]
pub struct CredentialPool {
    /// provider → ordered credentials
    pools: HashMap<String, Vec<PooledCredential>>,
    /// provider → active index
    active: HashMap<String, usize>,
}

impl CredentialPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load pool from env for `provider` if present.
    ///
    /// Looks up `EDGECRAB_API_KEY_POOL_<PROVIDER>` then `EDGECRAB_API_KEY_POOL`.
    pub fn load_from_env_for(provider: &str) -> Self {
        let mut pool = Self::new();
        let specific = format!(
            "EDGECRAB_API_KEY_POOL_{}",
            provider.to_ascii_uppercase().replace(['-', '/'], "_")
        );
        let raw = std::env::var(&specific)
            .ok()
            .or_else(|| std::env::var("EDGECRAB_API_KEY_POOL").ok());
        if let Some(raw) = raw {
            let keys: Vec<PooledCredential> = raw
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .enumerate()
                .map(|(i, token)| PooledCredential {
                    label: format!("{provider}-{i}"),
                    token: token.to_string(),
                    exhausted: false,
                })
                .collect();
            if !keys.is_empty() {
                pool.pools.insert(provider.to_string(), keys);
                pool.active.insert(provider.to_string(), 0);
            }
        }
        pool
    }

    pub fn install_keys(&mut self, provider: &str, tokens: impl IntoIterator<Item = String>) {
        let keys: Vec<PooledCredential> = tokens
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .enumerate()
            .map(|(i, token)| PooledCredential {
                label: format!("{provider}-{i}"),
                token,
                exhausted: false,
            })
            .collect();
        if keys.is_empty() {
            return;
        }
        self.pools.insert(provider.to_string(), keys);
        self.active.insert(provider.to_string(), 0);
    }

    pub fn len(&self, provider: &str) -> usize {
        self.pools.get(provider).map(|v| v.len()).unwrap_or(0)
    }

    pub fn is_empty(&self, provider: &str) -> bool {
        self.len(provider) == 0
    }

    pub fn active_token(&self, provider: &str) -> Option<&str> {
        let idx = *self.active.get(provider)?;
        self.pools
            .get(provider)?
            .get(idx)
            .filter(|c| !c.exhausted)
            .map(|c| c.token.as_str())
    }

    /// Mark current credential exhausted and rotate to next healthy entry.
    ///
    /// Returns the new active token when rotation succeeded.
    pub fn mark_exhausted_and_rotate(&mut self, provider: &str) -> Option<String> {
        let keys = self.pools.get_mut(provider)?;
        if keys.is_empty() {
            return None;
        }
        let cur = *self.active.get(provider).unwrap_or(&0);
        if let Some(c) = keys.get_mut(cur) {
            c.exhausted = true;
        }
        // Find next non-exhausted
        let n = keys.len();
        for offset in 1..=n {
            let idx = (cur + offset) % n;
            if !keys[idx].exhausted {
                self.active.insert(provider.to_string(), idx);
                return Some(keys[idx].token.clone());
            }
        }
        None
    }

    /// Apply active token into an environment variable (optional bridge for providers).
    pub fn apply_active_to_env(&self, provider: &str, env_key: &str) -> bool {
        let Some(token) = self.active_token(provider) else {
            return false;
        };
        // SAFETY: process-local config for next provider construction; tests use temp env.
        unsafe {
            std::env::set_var(env_key, token);
        }
        true
    }
}

/// Process-global pool for retry path (Mutex).
static GLOBAL_POOL: Mutex<Option<CredentialPool>> = Mutex::new(None);

/// Ensure global pool is loaded for `provider` (lazy).
pub fn global_pool_ensure(provider: &str) {
    let mut guard = GLOBAL_POOL.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(CredentialPool::load_from_env_for(provider));
    } else if let Some(pool) = guard.as_mut()
        && pool.is_empty(provider)
    {
        let extra = CredentialPool::load_from_env_for(provider);
        if let Some(keys) = extra.pools.get(provider) {
            pool.install_keys(
                provider,
                keys.iter().map(|k| k.token.clone()).collect::<Vec<_>>(),
            );
        }
    }
}

/// Rotate global pool for provider; returns new token if any.
pub fn global_mark_exhausted_and_rotate(provider: &str) -> Option<String> {
    global_pool_ensure(provider);
    let mut guard = GLOBAL_POOL.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_mut()?.mark_exhausted_and_rotate(provider)
}

/// Environment variable consumed when rebuilding a provider after rotation.
pub fn provider_api_key_env(provider: &str) -> Option<String> {
    if let Ok(explicit) = std::env::var("EDGECRAB_API_KEY_POOL_ENV")
        && !explicit.trim().is_empty()
    {
        return Some(explicit);
    }
    default_provider_api_key_env(provider).map(str::to_string)
}

fn default_provider_api_key_env(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" | "openai-compatible" => "OPENAI_API_KEY",
        "xai" => "XAI_API_KEY",
        "google" | "gemini" => "GOOGLE_API_KEY",
        "groq" => "GROQ_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "together" => "TOGETHER_API_KEY",
        "fireworks" => "FIREWORKS_API_KEY",
        _ => return None,
    })
}

/// Install a rotated token for the next provider construction.
pub fn apply_rotated_token(provider: &str, token: &str) -> Result<&'static str, String> {
    let env_key = provider_api_key_env(provider)
        .ok_or_else(|| format!("no API-key environment mapping for provider '{provider}'"))?;
    // SAFETY: this is called only after the failed provider future has ended,
    // immediately before constructing its replacement.
    unsafe {
        std::env::set_var(&env_key, token);
    }
    // Return a static status rather than the key name/value so callers cannot
    // accidentally put credential details in logs.
    Ok("applied")
}

/// Test helper: replace global pool.
#[cfg(test)]
pub fn global_pool_install_for_test(pool: CredentialPool) {
    let mut guard = GLOBAL_POOL.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(pool);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_skips_exhausted() {
        let mut pool = CredentialPool::new();
        pool.install_keys("openai", ["key-a".into(), "key-b".into(), "key-c".into()]);
        assert_eq!(pool.active_token("openai"), Some("key-a"));
        let next = pool.mark_exhausted_and_rotate("openai");
        assert_eq!(next.as_deref(), Some("key-b"));
        assert_eq!(pool.active_token("openai"), Some("key-b"));
        let next = pool.mark_exhausted_and_rotate("openai");
        assert_eq!(next.as_deref(), Some("key-c"));
        assert!(pool.mark_exhausted_and_rotate("openai").is_none());
    }

    #[test]
    fn empty_pool_rotate_none() {
        let mut pool = CredentialPool::new();
        assert!(pool.mark_exhausted_and_rotate("x").is_none());
    }

    #[test]
    fn provider_key_environment_mapping_covers_primary_providers() {
        assert_eq!(
            default_provider_api_key_env("anthropic"),
            Some("ANTHROPIC_API_KEY")
        );
        assert_eq!(
            default_provider_api_key_env("openai"),
            Some("OPENAI_API_KEY")
        );
        assert!(default_provider_api_key_env("unknown-provider").is_none());
    }
}
