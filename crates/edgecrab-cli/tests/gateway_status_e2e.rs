use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn gateway_status_surfaces_env_backed_channels_and_partial_config() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        r#"
gateway:
  host: 127.0.0.1
  port: 8989
  webhook_enabled: true
"#,
    )
    .expect("config file");

    let output = Command::new(env!("CARGO_BIN_EXE_edgecrab"))
        .arg("--config")
        .arg(&config_path)
        .args(["gateway", "status"])
        .env("HOME", home.path())
        .env("MATRIX_HOMESERVER", "https://matrix.example")
        .env("MATRIX_ACCESS_TOKEN", "matrix-token")
        .env("TWILIO_ACCOUNT_SID", "sid-only")
        .env_remove("TWILIO_AUTH_TOKEN")
        .env_remove("TWILIO_PHONE_NUMBER")
        .env_remove("TELEGRAM_BOT_TOKEN")
        .env_remove("DISCORD_BOT_TOKEN")
        .env_remove("SLACK_BOT_TOKEN")
        .env_remove("SLACK_APP_TOKEN")
        .env_remove("SIGNAL_HTTP_URL")
        .env_remove("SIGNAL_ACCOUNT")
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "gateway status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Enabled platforms: webhook, matrix"));
    assert!(stdout.contains("TWILIO_AUTH_TOKEN"));
}

#[test]
fn gateway_status_respects_disabled_platforms_even_when_env_is_complete() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        r#"
gateway:
  host: 127.0.0.1
  port: 8989
  webhook_enabled: true
  disabled_platforms:
    - matrix
"#,
    )
    .expect("config file");

    let output = Command::new(env!("CARGO_BIN_EXE_edgecrab"))
        .arg("--config")
        .arg(&config_path)
        .args(["gateway", "status"])
        .env("HOME", home.path())
        .env("MATRIX_HOMESERVER", "https://matrix.example")
        .env("MATRIX_ACCESS_TOKEN", "matrix-token")
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "gateway status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Enabled platforms: webhook"));
    assert!(!stdout.contains("Enabled platforms: webhook, matrix"));
}

#[test]
fn gateway_status_reports_api_server_when_enabled() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        r#"
gateway:
  host: 127.0.0.1
  port: 8989
  webhook_enabled: true
"#,
    )
    .expect("config file");

    let output = Command::new(env!("CARGO_BIN_EXE_edgecrab"))
        .arg("--config")
        .arg(&config_path)
        .args(["gateway", "status"])
        .env("HOME", home.path())
        .env("API_SERVER_ENABLED", "true")
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "gateway status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("api_server"));
}

#[test]
fn gateway_status_reports_all_complete_catalog_env_platforms() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        r#"
gateway:
  host: 127.0.0.1
  port: 8989
  webhook_enabled: true
"#,
    )
    .expect("config file");

    let output = Command::new(env!("CARGO_BIN_EXE_edgecrab"))
        .arg("--config")
        .arg(&config_path)
        .args(["gateway", "status"])
        .env("HOME", home.path())
        .env("EMAIL_PROVIDER", "sendgrid")
        .env("EMAIL_API_KEY", "email-key")
        .env("EMAIL_FROM", "bot@example.com")
        .env("FEISHU_APP_ID", "cli-feishu-app")
        .env("FEISHU_APP_SECRET", "cli-feishu-secret")
        .env("TWILIO_ACCOUNT_SID", "sid")
        .env("TWILIO_AUTH_TOKEN", "token")
        .env("TWILIO_PHONE_NUMBER", "+15551234567")
        .env("MATRIX_HOMESERVER", "https://matrix.example")
        .env("MATRIX_ACCESS_TOKEN", "matrix-token")
        .env("MATTERMOST_URL", "https://mattermost.example")
        .env("MATTERMOST_TOKEN", "mattermost-token")
        .env("WECOM_BOT_ID", "cli-wecom-bot")
        .env("WECOM_SECRET", "cli-wecom-secret")
        .env("DINGTALK_APP_KEY", "ding-key")
        .env("DINGTALK_APP_SECRET", "ding-secret")
        .env("HA_URL", "http://homeassistant.local:8123")
        .env("HA_TOKEN", "ha-token")
        .env("API_SERVER_ENABLED", "true")
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "gateway status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(
        "Enabled platforms: feishu, wecom, webhook, email, sms, matrix, mattermost, dingtalk, homeassistant, api_server"
    ));
    assert!(!stdout.contains("Attention needed:"));
}

#[test]
fn gateway_status_reports_partial_feishu_and_wecom_configuration() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        r#"
gateway:
  host: 127.0.0.1
  port: 8989
  webhook_enabled: true
"#,
    )
    .expect("config file");

    let output = Command::new(env!("CARGO_BIN_EXE_edgecrab"))
        .arg("--config")
        .arg(&config_path)
        .args(["gateway", "status"])
        .env("HOME", home.path())
        .env("FEISHU_APP_ID", "only-id")
        .env_remove("FEISHU_APP_SECRET")
        .env("WECOM_BOT_ID", "only-bot")
        .env_remove("WECOM_SECRET")
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "gateway status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Attention needed:"));
    assert!(stdout.contains("FEISHU_APP_SECRET"));
    assert!(stdout.contains("WECOM_SECRET"));
}

#[test]
fn gateway_status_surfaces_provider_specific_email_attention() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        r#"
gateway:
  host: 127.0.0.1
  port: 8989
  webhook_enabled: true
"#,
    )
    .expect("config file");

    let output = Command::new(env!("CARGO_BIN_EXE_edgecrab"))
        .arg("--config")
        .arg(&config_path)
        .args(["gateway", "status"])
        .env("HOME", home.path())
        .env("EMAIL_PROVIDER", "mailgun")
        .env("EMAIL_API_KEY", "email-key")
        .env("EMAIL_FROM", "bot@example.com")
        .env_remove("EMAIL_DOMAIN")
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "gateway status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Attention needed:"));
    assert!(stdout.contains("Email: missing EMAIL_DOMAIN for mailgun"));
}

#[test]
fn gateway_status_respects_disabled_api_server_even_when_env_enables_it() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        r#"
gateway:
  host: 127.0.0.1
  port: 8989
  webhook_enabled: true
  disabled_platforms:
    - api_server
"#,
    )
    .expect("config file");

    let output = Command::new(env!("CARGO_BIN_EXE_edgecrab"))
        .arg("--config")
        .arg(&config_path)
        .args(["gateway", "status"])
        .env("HOME", home.path())
        .env("API_SERVER_ENABLED", "true")
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "gateway status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Enabled platforms: webhook"));
    assert!(!stdout.contains("Enabled platforms: webhook, api_server"));
}

#[test]
fn gateway_status_respects_disabled_typed_platforms_with_legacy_enable_and_token() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        r#"
gateway:
  host: 127.0.0.1
  port: 8989
  webhook_enabled: true
  disabled_platforms:
    - telegram
  telegram:
    enabled: true
"#,
    )
    .expect("config file");

    let output = Command::new(env!("CARGO_BIN_EXE_edgecrab"))
        .arg("--config")
        .arg(&config_path)
        .args(["gateway", "status"])
        .env("HOME", home.path())
        .env("TELEGRAM_BOT_TOKEN", "telegram-token")
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "gateway status failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Enabled platforms: webhook"));
    assert!(!stdout.contains("Enabled platforms: webhook, telegram"));
    assert!(!stdout.contains("Telegram:"));
}
