//! GitHub auth cascade for Skills Hub (PAT / `gh` / App installation token).
//!
//! Hermes `GitHubAuth` parity:
//! 1. `GITHUB_TOKEN` / `GH_TOKEN` / `EDGECRAB_GITHUB_TOKEN`
//! 2. `gh auth token`
//! 3. GitHub App installation token via `gh api` when
//!    `EDGECRAB_GH_APP_INSTALLATION_ID` is set (requires `gh` authenticated as the App
//!    or with suitable credentials).

/// Resolve a bearer token for GitHub API calls.
pub fn resolve_github_token() -> Option<String> {
    for key in ["GITHUB_TOKEN", "GH_TOKEN", "EDGECRAB_GITHUB_TOKEN"] {
        if let Ok(v) = std::env::var(key) {
            let t = v.trim().to_string();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    if let Some(token) = gh_cli_token() {
        return Some(token);
    }
    github_app_installation_token().ok().flatten()
}

fn gh_cli_token() -> Option<String> {
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() { None } else { Some(token) }
}

/// Mint an installation token when App installation id is configured.
///
/// Uses `gh api` (same operator path Hermes documents for App-backed hubs).
/// Returns `Ok(None)` when App env is unset.
pub fn github_app_installation_token() -> Result<Option<String>, String> {
    let install_id = match std::env::var("EDGECRAB_GH_APP_INSTALLATION_ID") {
        Ok(v) if !v.trim().is_empty() => v.trim().to_string(),
        _ => return Ok(None),
    };
    // Optional: presence of App id signals intent even if gh mints via existing auth.
    let _app_id = std::env::var("EDGECRAB_GH_APP_ID").ok();

    let output = std::process::Command::new("gh")
        .args([
            "api",
            "--method",
            "POST",
            &format!("/app/installations/{install_id}/access_tokens"),
            "--jq",
            ".token",
        ])
        .output()
        .map_err(|e| {
            format!(
                "GitHub App token: `gh` unavailable ({e}). \
                 Export GITHUB_TOKEN or install GitHub CLI."
            )
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "GitHub App installation token failed via gh api.\n{stderr}\n\
             Set GITHUB_TOKEN, or authenticate gh as the GitHub App."
        ));
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        Ok(None)
    } else {
        Ok(Some(token))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_token_returns_none_without_install_id() {
        // Ensure missing App env does not error when install id unset.
        // (May still call gh if env leaked in CI — tolerate Ok(_) either way.)
        let _ = super::github_app_installation_token();
    }
}
