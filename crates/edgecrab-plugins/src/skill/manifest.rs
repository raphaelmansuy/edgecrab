use std::path::Path;

use crate::error::PluginError;
use regex::Regex;
use serde::Deserialize;

#[derive(Debug, Clone, Default)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub platforms: Vec<String>,
    pub required_environment_variables: Vec<RequiredEnvVar>,
    pub collect_secrets: Vec<CollectSecret>,
    pub setup_help: Option<String>,
    pub tags: Vec<String>,
    pub related_skills: Vec<String>,
    pub category: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, Default)]
pub struct RequiredEnvVar {
    pub name: String,
    pub prompt: String,
    pub help: String,
    pub required_for: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CollectSecret {
    pub env_var: String,
    pub prompt: String,
    pub help: String,
    pub secret: bool,
}

#[derive(Debug, Deserialize, Default)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    author: Option<String>,
    license: Option<String>,
    compatibility: Option<String>,
    #[serde(default)]
    platforms: PlatformList,
    #[serde(default)]
    prerequisites: Option<LegacyPrerequisites>,
    #[serde(default)]
    required_environment_variables: Vec<RequiredEnvVarRaw>,
    #[serde(default)]
    setup: Option<SetupBlock>,
    #[serde(default)]
    metadata: Option<MetadataBlock>,
}

#[derive(Debug, Deserialize, Default)]
struct LegacyPrerequisites {
    #[serde(default)]
    env_vars: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
enum PlatformList {
    #[default]
    Missing,
    One(String),
    Many(Vec<String>),
}

impl PlatformList {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::Missing => Vec::new(),
            Self::One(value) => vec![value],
            Self::Many(values) => values,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct RequiredEnvVarRaw {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    env_var: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    help: Option<String>,
    #[serde(default)]
    required_for: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct SetupBlock {
    #[serde(default)]
    help: Option<String>,
    #[serde(default)]
    collect_secrets: Vec<CollectSecretRaw>,
}

#[derive(Debug, Deserialize, Default)]
struct CollectSecretRaw {
    env_var: String,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    help: Option<String>,
    #[serde(default)]
    provider_url: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default = "default_secret_true")]
    secret: bool,
}

#[derive(Debug, Deserialize, Default)]
struct MetadataBlock {
    #[serde(default)]
    hermes: Option<HermesMetadataBlock>,
}

#[derive(Debug, Deserialize, Default)]
struct HermesMetadataBlock {
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    related_skills: Vec<String>,
    #[serde(default)]
    category: Option<String>,
}

fn default_secret_true() -> bool {
    true
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let stripped = content.strip_prefix("---\n")?;
    let end = stripped.find("\n---\n")?;
    Some((&stripped[..end], &stripped[end + 5..]))
}

fn parse_simple_frontmatter(frontmatter: &str) -> SkillFrontmatter {
    let mut raw = serde_json::Map::new();
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        raw.insert(
            key.trim().to_string(),
            serde_json::Value::String(value.trim().trim_matches('"').to_string()),
        );
    }
    serde_json::from_value(serde_json::Value::Object(raw)).unwrap_or_default()
}

fn parse_frontmatter(content: &str) -> SkillFrontmatter {
    let Some((frontmatter, _body)) = split_frontmatter(content) else {
        return SkillFrontmatter::default();
    };

    serde_yml::from_str(frontmatter).unwrap_or_else(|_| parse_simple_frontmatter(frontmatter))
}

fn skill_body(content: &str) -> &str {
    split_frontmatter(content)
        .map(|(_, body)| body)
        .unwrap_or(content)
}

pub fn parse_skill_manifest(path: &Path) -> Result<SkillManifest, PluginError> {
    let content = std::fs::read_to_string(path)?;
    parse_skill_manifest_str(path, &content)
}

pub fn parse_skill_manifest_str(path: &Path, content: &str) -> Result<SkillManifest, PluginError> {
    let frontmatter = parse_frontmatter(content);
    let body = skill_body(content);

    let name = frontmatter.name.unwrap_or_else(|| {
        path.parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_string()
    });
    let description = frontmatter.description.unwrap_or_default();
    let name_re = Regex::new(r"^[a-z0-9][a-z0-9._-]*$").expect("valid regex");
    if body.trim().is_empty() {
        return Err(PluginError::InvalidSkill {
            path: path.to_path_buf(),
            message: "SKILL.md must have content after the frontmatter.".into(),
        });
    }
    if name.len() > 64 || !name_re.is_match(&name) {
        return Err(PluginError::InvalidSkill {
            path: path.to_path_buf(),
            message: format!("name '{name}' does not match pattern ^[a-z0-9][a-z0-9._-]*$"),
        });
    }
    if description.len() > 1024 {
        return Err(PluginError::InvalidSkill {
            path: path.to_path_buf(),
            message: "description exceeds 1024 characters".into(),
        });
    }

    let env_name_re = Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").expect("valid regex");
    let mut required_environment_variables = Vec::new();
    let setup_help = frontmatter
        .setup
        .as_ref()
        .and_then(|setup| setup.help.clone());
    if frontmatter.required_environment_variables.is_empty() {
        if let Some(prerequisites) = frontmatter.prerequisites {
            for env_var in prerequisites.env_vars {
                required_environment_variables.push(RequiredEnvVar {
                    prompt: format!("Enter value for {env_var}"),
                    name: env_var,
                    help: setup_help.clone().unwrap_or_default(),
                    required_for: None,
                });
            }
        }
    } else {
        for entry in frontmatter.required_environment_variables {
            let name = entry
                .name
                .or(entry.env_var)
                .unwrap_or_default()
                .trim()
                .to_string();
            if !env_name_re.is_match(&name) {
                return Err(PluginError::InvalidSkill {
                    path: path.to_path_buf(),
                    message: format!("invalid environment variable name '{name}'"),
                });
            }
            required_environment_variables.push(RequiredEnvVar {
                prompt: entry
                    .prompt
                    .unwrap_or_else(|| format!("Enter value for {name}")),
                help: entry
                    .help
                    .or_else(|| setup_help.clone())
                    .unwrap_or_default(),
                required_for: entry.required_for,
                name,
            });
        }
    }

    let collect_secrets = frontmatter
        .setup
        .map(|setup| {
            setup
                .collect_secrets
                .into_iter()
                .map(|entry| CollectSecret {
                    prompt: entry
                        .prompt
                        .unwrap_or_else(|| format!("Enter value for {}", entry.env_var)),
                    help: entry
                        .help
                        .or(entry.provider_url)
                        .or(entry.url)
                        .or_else(|| setup.help.clone())
                        .unwrap_or_default(),
                    secret: entry.secret,
                    env_var: entry.env_var,
                })
                .collect()
        })
        .unwrap_or_default();

    let hermes_metadata = frontmatter
        .metadata
        .and_then(|metadata| metadata.hermes)
        .unwrap_or_default();

    Ok(SkillManifest {
        name,
        description,
        version: frontmatter.version,
        author: frontmatter.author,
        license: frontmatter.license,
        compatibility: frontmatter.compatibility,
        platforms: frontmatter.platforms.into_vec(),
        required_environment_variables,
        collect_secrets,
        setup_help,
        tags: hermes_metadata.tags,
        related_skills: hermes_metadata.related_skills,
        category: hermes_metadata.category,
        body: body.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::OnceLock;

    use super::*;

    const REAL_HERMES_REPO_URL: &str = "https://github.com/NousResearch/hermes-agent";
    const REAL_HERMES_REF: &str = "268ee6bdce013c74c9a8dfbb13fd850423189322";

    fn real_hermes_repo() -> &'static PathBuf {
        static REPO: OnceLock<PathBuf> = OnceLock::new();
        REPO.get_or_init(|| {
            let path = std::env::temp_dir()
                .join(format!("edgecrab-hermes-agent-skills-{REAL_HERMES_REF}"));
            if path.exists() {
                return path;
            }
            let status = Command::new("git")
                .args([
                    "clone",
                    "--depth",
                    "1",
                    REAL_HERMES_REPO_URL,
                    path.to_str().expect("utf8 path"),
                ])
                .status()
                .expect("git clone hermes-agent");
            if !status.success() {
                assert!(
                    path.join("skills").is_dir() || path.join("optional-skills").is_dir(),
                    "failed to clone hermes-agent fixtures"
                );
                return path;
            }

            let status = Command::new("git")
                .args([
                    "-C",
                    path.to_str().expect("utf8 path"),
                    "checkout",
                    REAL_HERMES_REF,
                ])
                .status()
                .expect("git checkout hermes-agent ref");
            assert!(status.success(), "failed to checkout hermes-agent ref");
            path
        })
    }

    #[test]
    fn parses_env_var_alias_and_setup_help_fallback() {
        let path = Path::new("/tmp/demo/SKILL.md");
        let skill = parse_skill_manifest_str(
            path,
            r#"---
name: demo
description: Demo
required_environment_variables:
  - env_var: DEMO_TOKEN
setup:
  help: https://example.com/help
  collect_secrets:
    - env_var: SECOND_TOKEN
---

# Demo

Use the demo workflow.
"#,
        )
        .expect("skill parses");

        assert_eq!(skill.required_environment_variables[0].name, "DEMO_TOKEN");
        assert_eq!(skill.collect_secrets[0].help, "https://example.com/help");
    }

    #[test]
    fn rejects_empty_body() {
        let path = Path::new("/tmp/demo/SKILL.md");
        let error = parse_skill_manifest_str(path, "---\nname: demo\ndescription: Demo\n---\n\n")
            .expect_err("empty body rejected");
        assert!(
            error
                .to_string()
                .contains("SKILL.md must have content after the frontmatter.")
        );
    }

    #[test]
    fn missing_frontmatter_is_treated_as_body() {
        let path = Path::new("/tmp/demo/SKILL.md");
        let skill = parse_skill_manifest_str(path, "# Demo\n\nBody.\n").expect("skill parses");

        assert_eq!(skill.name, "demo");
        assert_eq!(skill.description, "");
        assert_eq!(skill.body, "# Demo\n\nBody.");
    }

    #[test]
    fn malformed_frontmatter_falls_back_to_simple_key_value_parsing() {
        let path = Path::new("/tmp/demo/SKILL.md");
        let skill = parse_skill_manifest_str(
            path,
            "---\nname: demo\ndescription: broken: yaml: here\nplatforms: macos\n---\n\nBody.\n",
        )
        .expect("skill parses");

        assert_eq!(skill.name, "demo");
        assert_eq!(skill.description, "broken: yaml: here");
        assert_eq!(skill.platforms, vec!["macos"]);
    }

    #[test]
    fn parses_hermes_metadata_fields() {
        let path = Path::new("/tmp/demo/SKILL.md");
        let skill = parse_skill_manifest_str(
            path,
            r#"---
name: demo
description: Demo
compatibility: Requires macOS 13+
metadata:
  hermes:
    tags: [GitHub, Issues]
    related_skills: [github-auth, github-pr-workflow]
    category: version-control
---

Body.
"#,
        )
        .expect("skill parses");

        assert_eq!(skill.compatibility.as_deref(), Some("Requires macOS 13+"));
        assert_eq!(skill.tags, vec!["GitHub", "Issues"]);
        assert_eq!(
            skill.related_skills,
            vec!["github-auth", "github-pr-workflow"]
        );
        assert_eq!(skill.category.as_deref(), Some("version-control"));
    }

    #[test]
    fn setup_help_falls_back_for_required_environment_variables() {
        let path = Path::new("/tmp/demo/SKILL.md");
        let skill = parse_skill_manifest_str(
            path,
            r#"---
name: demo
description: Demo
required_environment_variables:
  - name: DEMO_TOKEN
setup:
  help: https://example.com/setup
---

Body.
"#,
        )
        .expect("skill parses");

        assert_eq!(
            skill.required_environment_variables[0].help,
            "https://example.com/setup"
        );
    }

    #[test]
    fn parses_real_github_issues_skill_without_mutating_it() {
        let skill_path = real_hermes_repo().join("skills/github/github-issues/SKILL.md");
        let before = std::fs::read_to_string(&skill_path).expect("read skill before parse");
        let skill = parse_skill_manifest(&skill_path).expect("parse real github-issues skill");
        let after = std::fs::read_to_string(&skill_path).expect("read skill after parse");

        assert_eq!(skill.name, "github-issues");
        assert!(skill.tags.iter().any(|tag| tag == "GitHub"));
        assert_eq!(
            skill.related_skills,
            vec!["github-auth", "github-pr-workflow"]
        );
        assert!(skill.body.contains("~/.hermes/.env"));
        let injected = crate::skill::inject::build_prompt_fragment(&skill.name, &skill.body);
        assert!(injected.contains("~/.edgecrab/.env"));
        assert_eq!(before, after);
    }

    #[test]
    fn parses_real_1password_setup_block() {
        let skill_path = real_hermes_repo().join("optional-skills/security/1password/SKILL.md");
        let skill = parse_skill_manifest(&skill_path).expect("parse real 1password skill");

        assert_eq!(skill.name, "1password");
        assert_eq!(skill.category.as_deref(), Some("security"));
        assert_eq!(
            skill.setup_help.as_deref(),
            Some(
                "Create a service account at https://my.1password.com → Settings → Service Accounts"
            )
        );
        assert_eq!(skill.collect_secrets.len(), 1);
        assert_eq!(skill.collect_secrets[0].env_var, "OP_SERVICE_ACCOUNT_TOKEN");
        assert_eq!(
            skill.collect_secrets[0].help,
            "https://developer.1password.com/docs/service-accounts/"
        );
        assert!(skill.collect_secrets[0].secret);
    }
}
