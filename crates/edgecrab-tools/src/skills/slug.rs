use regex::Regex;
use std::sync::LazyLock;

static INVALID_CHARS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9-]").expect("regex"));
static MULTI_HYPHEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-{2,}").expect("regex"));

/// Normalize a skill or bundle name into a slash-command slug.
pub fn slugify(name: &str) -> String {
    let mut cmd = name.to_ascii_lowercase().replace([' ', '_'], "-");
    cmd = INVALID_CHARS.replace_all(&cmd, "").into_owned();
    cmd = MULTI_HYPHEN.replace_all(&cmd, "-").into_owned();
    cmd.trim_matches('-').to_string()
}

/// Normalize user-typed command token (no leading slash).
pub fn normalize_command_token(command: &str) -> String {
    command.trim().replace('_', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_strips_special_chars() {
        assert_eq!(slugify("GitHub PR Workflow"), "github-pr-workflow");
        assert_eq!(slugify("foo_bar"), "foo-bar");
    }
}
