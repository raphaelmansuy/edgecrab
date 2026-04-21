use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn edgecrab() -> Command {
    Command::new(env!("CARGO_BIN_EXE_edgecrab"))
}

fn clear_provider_envs(cmd: &mut Command) -> &mut Command {
    for key in [
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
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

fn run_git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn make_repo_with_remote() -> tempfile::TempDir {
    let temp = tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let bare = temp.path().join("remote.git");
    fs::create_dir_all(&repo).expect("create repo");

    run_git(&repo, &["init", "-b", "main"]);
    run_git(&repo, &["config", "user.email", "test@example.com"]);
    run_git(&repo, &["config", "user.name", "Test User"]);
    fs::write(repo.join("README.md"), "# test\n").expect("write readme");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "-m", "initial"]);

    let output = Command::new("git")
        .args(["init", "--bare"])
        .arg(&bare)
        .output()
        .expect("init bare");
    assert!(
        output.status.success(),
        "git init --bare failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    run_git(
        &repo,
        &["remote", "add", "origin", bare.to_str().expect("utf8")],
    );
    run_git(&repo, &["push", "-u", "origin", "main"]);

    temp
}

#[test]
fn worktree_flag_runs_in_isolated_repo_and_cleans_up() {
    let temp = make_repo_with_remote();
    let repo = temp.path().join("repo");
    let edgecrab_home = temp.path().join("edgecrab-home");
    fs::create_dir_all(&edgecrab_home).expect("edgecrab home");
    fs::write(edgecrab_home.join("config.yaml"), "mcp_servers: {}\n").expect("config");

    let mut cmd = edgecrab();
    clear_provider_envs(
        cmd.current_dir(&repo)
            .arg("--config")
            .arg(edgecrab_home.join("config.yaml"))
            .arg("--model")
            .arg("mock")
            .arg("-w")
            .arg("-q")
            .arg("say hello")
            .env("HOME", temp.path())
            .env("EDGECRAB_HOME", &edgecrab_home),
    );
    let output = cmd.output().expect("run edgecrab");

    assert!(
        output.status.success(),
        "edgecrab -w failed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Running in isolated worktree"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("Worktree cleaned up"), "stderr:\n{stderr}");

    let worktrees_dir = repo.join(".worktrees");
    let remaining_entries = fs::read_dir(&worktrees_dir)
        .expect("read .worktrees")
        .filter_map(Result::ok)
        .count();
    assert_eq!(
        remaining_entries, 0,
        ".worktrees should be empty after cleanup"
    );

    let gitignore = fs::read_to_string(repo.join(".gitignore")).expect("read gitignore");
    assert!(gitignore.lines().any(|line| line.trim() == ".worktrees/"));
}

#[test]
fn worktree_flag_refuses_to_run_outside_git_repo() {
    let temp = tempdir().expect("tempdir");
    let edgecrab_home = temp.path().join("edgecrab-home");
    fs::create_dir_all(&edgecrab_home).expect("edgecrab home");
    fs::write(edgecrab_home.join("config.yaml"), "mcp_servers: {}\n").expect("config");

    let mut cmd = edgecrab();
    clear_provider_envs(
        cmd.current_dir(temp.path())
            .arg("--config")
            .arg(edgecrab_home.join("config.yaml"))
            .arg("--model")
            .arg("mock")
            .arg("-w")
            .arg("-q")
            .arg("say hello")
            .env("HOME", temp.path())
            .env("EDGECRAB_HOME", &edgecrab_home),
    );
    let output = cmd.output().expect("run edgecrab");

    assert!(
        !output.status.success(),
        "command should fail outside git repo"
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("git repository") || combined.contains("--worktree requires"),
        "expected git repo error, got:\n{combined}"
    );
}

#[test]
fn config_worktree_mode_runs_without_explicit_flag() {
    let temp = make_repo_with_remote();
    let repo = temp.path().join("repo");
    let edgecrab_home = temp.path().join("edgecrab-home");
    fs::create_dir_all(&edgecrab_home).expect("edgecrab home");
    fs::write(
        edgecrab_home.join("config.yaml"),
        "worktree: true\nmcp_servers: {}\n",
    )
    .expect("config");

    let mut cmd = edgecrab();
    clear_provider_envs(
        cmd.current_dir(&repo)
            .arg("--config")
            .arg(edgecrab_home.join("config.yaml"))
            .arg("--model")
            .arg("mock")
            .arg("-q")
            .arg("say hello")
            .env("HOME", temp.path())
            .env("EDGECRAB_HOME", &edgecrab_home),
    );
    let output = cmd.output().expect("run edgecrab");

    assert!(
        output.status.success(),
        "edgecrab config worktree failed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Running in isolated worktree"),
        "stderr:\n{stderr}"
    );
    assert!(stderr.contains("Worktree cleaned up"), "stderr:\n{stderr}");
}
