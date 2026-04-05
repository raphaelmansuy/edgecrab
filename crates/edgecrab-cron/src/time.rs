use std::path::PathBuf;

use anyhow::{Context, anyhow};
use chrono::{DateTime, Local, LocalResult, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde_yml::Value;

pub(crate) fn edgecrab_home_dir() -> anyhow::Result<PathBuf> {
    if let Ok(home) = std::env::var("EDGECRAB_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    let home = dirs::home_dir().context("cannot resolve home directory")?;
    Ok(home.join(".edgecrab"))
}

fn configured_timezone_name() -> Option<String> {
    if let Ok(value) = std::env::var("EDGECRAB_TIMEZONE") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }

    let config_path = edgecrab_home_dir().ok()?.join("config.yaml");
    let content = std::fs::read_to_string(config_path).ok()?;
    let raw = serde_yml::from_str::<Value>(&content).ok()?;
    raw.get("timezone")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn configured_timezone() -> Option<Tz> {
    let name = configured_timezone_name()?;
    match name.parse::<Tz>() {
        Ok(tz) => Some(tz),
        Err(err) => {
            tracing::warn!(
                timezone = %name,
                error = %err,
                "invalid EDGECRAB_TIMEZONE/config timezone; falling back to server local time"
            );
            None
        }
    }
}

pub(crate) fn now_in_user_timezone() -> DateTime<chrono::FixedOffset> {
    if let Some(tz) = configured_timezone() {
        return Utc::now().with_timezone(&tz).fixed_offset();
    }
    Local::now().fixed_offset()
}

pub(crate) fn to_user_timezone(dt: DateTime<Utc>) -> DateTime<chrono::FixedOffset> {
    if let Some(tz) = configured_timezone() {
        return dt.with_timezone(&tz).fixed_offset();
    }
    dt.with_timezone(&Local).fixed_offset()
}

pub(crate) fn naive_local_to_utc(ndt: NaiveDateTime) -> anyhow::Result<DateTime<Utc>> {
    if let Some(tz) = configured_timezone() {
        return match tz.from_local_datetime(&ndt) {
            LocalResult::Single(dt) => Ok(dt.with_timezone(&Utc)),
            LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
            LocalResult::None => Err(anyhow!(
                "timestamp does not exist in configured timezone '{}'",
                tz
            )),
        };
    }

    match Local.from_local_datetime(&ndt) {
        LocalResult::Single(dt) => Ok(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, _) => Ok(first.with_timezone(&Utc)),
        LocalResult::None => Err(anyhow!("timestamp does not exist in local timezone")),
    }
}
