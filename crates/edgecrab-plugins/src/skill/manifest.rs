use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use serde::Deserialize;

use crate::error::PluginError;

#[derive(Debug, Clone, Default)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub platforms: Vec<String>,
    pub required_environment_variables: Vec<RequiredEnvVar>,
    pub collect_secrets: Vec<CollectSecret>,
    pub setup_help: Option<String>,
    pub tags: Vec<String>,
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
    #[serde(default)]
    platforms: Vec<String>,
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
    hermes: Option<HashMap<String, serde_json::Value>>,
}

fn default_secret_true() -> bool {
    true
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let stripped = content.strip_prefix("---\n")?;
    let (frontmatter, body) = stripped.split_once("\n---\n")?;
    Some((frontmatter, body))
}

pub fn parse_skill_manifest(path: &Path) -> Result<SkillManifest, PluginError> {
    let content = std::fs::read_to_string(path)?;
    parse_skill_manifest_str(path, &content)
}

pub fn parse_skill_manifest_str(path: &Path, content: &str) -> Result<SkillManifest, PluginError> {
    let (frontmatter, body) =
        split_frontmatter(content).ok_or_else(|| PluginError::InvalidSkill {
            path: path.to_path_buf(),
            message: "missing YAML frontmatter".into(),
        })?;
    let frontmatter: SkillFrontmatter =
        serde_yml::from_str(frontmatter).map_err(|error| PluginError::InvalidSkill {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

    let name = frontmatter.name.unwrap_or_else(|| {
        path.parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_string()
    });
    let description = frontmatter.description.unwrap_or_default();
    if body.trim().is_empty() {
        return Err(PluginError::InvalidSkill {
            path: path.to_path_buf(),
            message: "empty body".into(),
        });
    }

    let env_name_re = Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").expect("valid regex");
    let mut required_environment_variables = Vec::new();
    if frontmatter.required_environment_variables.is_empty() {
        if let Some(prerequisites) = frontmatter.prerequisites {
            for env_var in prerequisites.env_vars {
                required_environment_variables.push(RequiredEnvVar {
                    prompt: format!("Enter value for {env_var}"),
                    name: env_var,
                    help: String::new(),
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
                help: entry.help.unwrap_or_default(),
                required_for: entry.required_for,
                name,
            });
        }
    }

    let setup_help = frontmatter
        .setup
        .as_ref()
        .and_then(|setup| setup.help.clone());
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

    let tags = frontmatter
        .metadata
        .and_then(|metadata| metadata.hermes)
        .and_then(|mut hermes| hermes.remove("tags"))
        .and_then(|value| value.as_array().cloned())
        .map(|values| {
            values
                .into_iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect()
        })
        .unwrap_or_default();

    Ok(SkillManifest {
        name,
        description,
        version: frontmatter.version,
        author: frontmatter.author,
        license: frontmatter.license,
        platforms: frontmatter.platforms,
        required_environment_variables,
        collect_secrets,
        setup_help,
        tags,
        body: body.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(error.to_string().contains("empty body"));
    }
}
