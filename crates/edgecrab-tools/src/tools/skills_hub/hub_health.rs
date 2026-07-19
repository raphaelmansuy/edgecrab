//! Skills Hub health assessment for `edgecrab doctor` (019 W3).
//!
//! Keeps lock/quarantine/index/token checks in the hub façade so doctor stays a
//! thin renderer (DRY).

use super::index::{INDEX_TTL_SECS, index_age_secs, index_file_exists};
use super::{lock_file_path, quarantine_dir, read_taps};

/// Aggregated hub health for doctor / slash surfaces.
#[derive(Debug, Clone)]
pub struct HubHealth {
    pub taps_count: usize,
    pub lock_present: bool,
    pub lock_parse_ok: bool,
    pub lock_error: Option<String>,
    pub quarantine_orphans: usize,
    pub index_present: bool,
    pub index_age_secs: Option<i64>,
    pub index_stale_beyond_2x_ttl: bool,
    pub github_token_set: bool,
}

impl HubHealth {
    /// Worst status for a single doctor Skills check.
    pub fn doctor_severity(&self) -> HubHealthSeverity {
        if self.lock_present && !self.lock_parse_ok {
            return HubHealthSeverity::Fail;
        }
        if !self.github_token_set
            || self.quarantine_orphans > 0
            || self.index_stale_beyond_2x_ttl
            || (self.lock_present && !self.lock_parse_ok)
        {
            return HubHealthSeverity::Warn;
        }
        HubHealthSeverity::Pass
    }

    pub fn doctor_detail(&self, skills_count: usize, skills_dir_display: &str) -> String {
        let mut parts = vec![format!(
            "{skills_dir_display} ({skills_count} skills; {} taps",
            self.taps_count
        )];
        if self.lock_present {
            if self.lock_parse_ok {
                parts.push("; lock ok".into());
            } else {
                parts.push(format!(
                    "; lock CORRUPT: {}",
                    self.lock_error.as_deref().unwrap_or("parse error")
                ));
            }
        } else if skills_count > 0 {
            parts.push("; lock missing".into());
        }
        if self.quarantine_orphans > 0 {
            parts.push(format!(
                "; {} quarantine orphan(s)",
                self.quarantine_orphans
            ));
        }
        match self.index_age_secs {
            Some(age) if self.index_stale_beyond_2x_ttl => {
                parts.push(format!(
                    "; index age {age}s (> 2×TTL={}s) — run skills index refresh",
                    INDEX_TTL_SECS * 2
                ));
            }
            Some(age) => parts.push(format!("; index age {age}s")),
            None if self.index_present => parts.push("; index unreadable".into()),
            None => parts.push("; index not built".into()),
        }
        if self.github_token_set {
            parts.push("; GITHUB_TOKEN/GH_TOKEN set".into());
        } else {
            parts.push("; warn: set GITHUB_TOKEN or GH_TOKEN (hub search may rate-limit)".into());
        }
        parts.push(")".into());
        parts.concat()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubHealthSeverity {
    Pass,
    Warn,
    Fail,
}

/// Assess hub health under `EDGECRAB_HOME` (via resolve) / skills dir layout.
pub fn assess_hub_health() -> HubHealth {
    let lock_path = lock_file_path();
    let lock_present = lock_path.is_file();
    let (lock_parse_ok, lock_error) = if lock_present {
        match std::fs::read_to_string(&lock_path) {
            Ok(raw) => match validate_lock_json(&raw) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e)),
            },
            Err(e) => (false, Some(e.to_string())),
        }
    } else {
        (true, None)
    };

    let quarantine_orphans = count_quarantine_orphans();
    let index_present = index_file_exists();
    let index_age_secs = index_age_secs();
    let index_stale_beyond_2x_ttl = index_age_secs
        .map(|age| age > INDEX_TTL_SECS * 2)
        .unwrap_or(false);

    let github_token_set = super::resolve_github_token().is_some();

    HubHealth {
        taps_count: read_taps().len(),
        lock_present,
        lock_parse_ok,
        lock_error,
        quarantine_orphans,
        index_present,
        index_age_secs,
        index_stale_beyond_2x_ttl,
        github_token_set,
    }
}

fn count_quarantine_orphans() -> usize {
    let q = quarantine_dir();
    if !q.is_dir() {
        return 0;
    }
    std::fs::read_dir(&q)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .count()
        })
        .unwrap_or(0)
}

/// Validate lock bytes without requiring EDGECRAB_HOME layout.
pub fn validate_lock_json(raw: &str) -> Result<(), String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("lock parse error: {e}"))?;
    if !v.is_object() {
        return Err("lock.json root is not an object".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn validate_lock_rejects_garbage() {
        assert!(validate_lock_json("{not json").is_err());
        assert!(validate_lock_json("[]").is_err());
        assert!(validate_lock_json(r#"{"skill":{}}"#).is_ok());
    }

    #[test]
    fn assess_detects_corrupt_lock_and_orphans() {
        let tmp = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("EDGECRAB_HOME", tmp.path());
        }
        let hub = tmp.path().join("skills").join(".hub");
        std::fs::create_dir_all(hub.join("quarantine").join("orphan-skill")).unwrap();
        std::fs::write(hub.join("lock.json"), "{broken").unwrap();
        let health = assess_hub_health();
        assert!(health.lock_present);
        assert!(!health.lock_parse_ok);
        assert!(health.quarantine_orphans >= 1);
        assert_eq!(health.doctor_severity(), HubHealthSeverity::Fail);
        unsafe {
            std::env::remove_var("EDGECRAB_HOME");
        }
    }
}
