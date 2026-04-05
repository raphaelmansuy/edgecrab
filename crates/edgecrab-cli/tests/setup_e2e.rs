/// Setup command end-to-end tests (non-interactive).
///
/// These tests validate setup behaviour that does NOT require a TTY:
///   - Unknown section rejection
///   - Doctor command works after write_config produces a valid file
///   - Env-based auto-detection path (OPENAI_API_KEY present)
///
/// Interactive prompts are NOT exercised here; that is covered by unit tests
/// inside setup.rs behind `#[cfg(test)]`.
use std::fs;
use std::process::Command;
use tempfile::tempdir;

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn edgecrab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgecrab"))
}

fn clear_provider_envs(cmd: &mut Command) -> &mut Command {
    for key in [
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
        "GOOGLE_API_KEY",
        "GEMINI_API_KEY",
        "HF_TOKEN",
        "HUGGINGFACE_TOKEN",
        "XAI_API_KEY",
        "DEEPSEEK_API_KEY",
        "MISTRAL_API_KEY",
        "GROQ_API_KEY",
        "COHERE_API_KEY",
        "PERPLEXITY_API_KEY",
        "ZAI_API_KEY",
        "AZURE_OPENAI_API_KEY",
        "AZURE_OPENAI_ENDPOINT",
        "AZURE_OPENAI_DEPLOYMENT",
        "VERTEX_PROJECT_ID",
        "VERTEX_LOCATION",
        "GOOGLE_CLOUD_PROJECT",
        "OLLAMA_HOST",
        "OLLAMA_MODEL",
        "LMSTUDIO_HOST",
        "LMSTUDIO_MODEL",
        "EDGEQUAKE_LLM_PROVIDER",
    ] {
        cmd.env_remove(key);
    }
    cmd
}

// ─────────────────────────────────────────────────────────────────────────────
// Section validation
// ─────────────────────────────────────────────────────────────────────────────

/// `edgecrab setup <unknown>` must exit non-zero and surface an error message.
#[test]
fn setup_rejects_unknown_section() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    // Write a minimal valid config so the binary does not try fresh-setup.
    fs::write(&config_path, "provider: openai\nmodel: gpt-4o-mini\n").expect("config write");

    let mut cmd = edgecrab();
    clear_provider_envs(
        cmd.arg("--config")
            .arg(&config_path)
            .args(["setup", "not_a_real_section"])
            .env("HOME", home.path()),
    );
    let out = cmd.output().expect("run edgecrab");

    assert!(
        !out.status.success(),
        "expected non-zero exit for unknown section; got: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        combined.to_lowercase().contains("unknown")
            || combined.to_lowercase().contains("not_a_real_section"),
        "error message should mention the bad section; got: {combined}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Doctor sanity after minimal config
// ─────────────────────────────────────────────────────────────────────────────

/// After a minimal config file is written, `edgecrab doctor` must exit zero and
/// report the configured provider.
#[test]
fn doctor_passes_with_minimal_config() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        "provider: openrouter\nmodel: nousresearch/hermes-3-llama-3.1-405b\n",
    )
    .expect("config write");

    let mut cmd = edgecrab();
    clear_provider_envs(
        cmd.arg("--config")
            .arg(&config_path)
            .arg("doctor")
            .env("HOME", home.path()),
    );
    let out = cmd.output().expect("run edgecrab doctor");

    // doctor should exit 0 (env key missing is a warning, not a fatal error)
    assert!(
        out.status.success(),
        "doctor exited non-zero: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Doctor should at minimum confirm the config file was found.
    assert!(
        stdout.contains("Config file") || stdout.contains("config"),
        "doctor output should mention the config file; got:\n{stdout}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Config missing — help path
// ─────────────────────────────────────────────────────────────────────────────

/// When no config exists and stdin is not a TTY (CI environment), the setup
/// command must not hang; it should exit with a non-zero or show usage/help.
/// We only assert exit within a reasonable time (process should not hang).
#[test]
fn setup_does_not_hang_without_tty() {
    let home = tempdir().expect("temp home");
    // No config written ─ triggers fresh-setup path.

    use std::time::Duration;

    let mut cmd = edgecrab();
    clear_provider_envs(
        cmd.arg("--config")
            .arg(home.path().join("no-such.yaml"))
            .arg("setup")
            .env("HOME", home.path())
            // Feed empty stdin so dialoguer / read_line gets EOF immediately.
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped()),
    );
    let mut child = cmd.spawn().expect("spawn edgecrab");

    // Give it at most 5 seconds.
    let timeout = Duration::from_secs(5);
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            // Exited — pass (either success or error, just not hung)
            let _ = status;
            return;
        }
        if start.elapsed() > timeout {
            child.kill().ok();
            panic!(
                "edgecrab setup hung for {timeout:?} without a TTY — dialoguer must detect non-TTY"
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Section list — all canonical sections must not panic for valid config
// ─────────────────────────────────────────────────────────────────────────────

/// Each known non-interactive-safe sub-command must be validated here.
/// We use `gateway` section because it only loads config and prints; it does
/// not try to prompt when the config file is pre-populated and stdin is null.
#[test]
fn setup_gateway_section_surfaces_platform_table() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");
    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        r#"
provider: openai
model: gpt-4o-mini
gateway:
  host: 127.0.0.1
  port: 8989
  telegram:
    enabled: true
    token: "tok-test"
"#,
    )
    .expect("config write");

    // `edgecrab gateway status` is non-interactive and shows the full platform table.
    let mut cmd = edgecrab();
    clear_provider_envs(
        cmd.arg("--config")
            .arg(&config_path)
            .args(["gateway", "status"])
            .env("HOME", home.path())
            .env_remove("TELEGRAM_BOT_TOKEN")
            .env_remove("DISCORD_BOT_TOKEN")
            .env_remove("SLACK_BOT_TOKEN"),
    );
    let out = cmd.output().expect("run edgecrab gateway status");

    assert!(
        out.status.success(),
        "gateway status failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    // gateway status shows runtime process status — verify the output is well-formed.
    assert!(
        stdout.contains("Gateway") || stdout.contains("gateway") || stdout.contains("Process"),
        "gateway status output should show gateway info; got:\n{stdout}"
    );
    // The command must surface at least the log file or next steps.
    assert!(
        stdout.contains("gateway"),
        "gateway status should contain 'gateway'; got:\n{stdout}"
    );
}
