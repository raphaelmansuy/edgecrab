use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn edgecrab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgecrab"))
}

#[test]
fn mcp_doctor_reports_static_stdio_failures() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");

    let cwd_file = home.path().join("not-a-directory.txt");
    fs::write(&cwd_file, "x").expect("cwd file");

    let config_path = config_dir.join("config.yaml");
    fs::write(
        &config_path,
        format!(
            "mcp_servers:\n  broken:\n    command: definitely-not-an-edgecrab-command\n    cwd: {}\n    enabled: true\n",
            cwd_file.display()
        ),
    )
    .expect("config");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args(["mcp", "doctor", "broken"])
        .env("HOME", home.path())
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "mcp doctor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("broken  fail"), "stdout:\n{stdout}");
    assert!(stdout.contains("command: fail"), "stdout:\n{stdout}");
    assert!(stdout.contains("cwd: not-a-directory"), "stdout:\n{stdout}");
}

#[test]
fn mcp_install_accepts_path_with_spaces_and_persists_it() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".edgecrab");
    fs::create_dir_all(&config_dir).expect("config dir");

    let workspace_dir = home.path().join("workspace with spaces");
    fs::create_dir_all(&workspace_dir).expect("workspace dir");

    let config_path = config_dir.join("config.yaml");

    let output = edgecrab()
        .arg("--config")
        .arg(&config_path)
        .args(["mcp", "install", "filesystem", "--path"])
        .arg(&workspace_dir)
        .env("HOME", home.path())
        .output()
        .expect("run edgecrab");

    assert!(
        output.status.success(),
        "mcp install failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Configured MCP server 'filesystem'."),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("edgecrab mcp doctor filesystem"),
        "stdout:\n{stdout}"
    );

    let written = fs::read_to_string(&config_path).expect("read config");
    let rendered_path = workspace_dir.display().to_string();
    assert!(
        written.contains(&rendered_path),
        "expected persisted config to contain path {rendered_path}, got:\n{written}"
    );
}
