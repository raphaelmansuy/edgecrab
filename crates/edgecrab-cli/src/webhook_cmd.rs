use std::fmt::Write as _;

use anyhow::{Context, anyhow};
use edgecrab_core::AppConfig;
use edgecrab_gateway::webhook_subscriptions::{
    CreateSubscriptionParams, base_url, create_subscription, hmac_sha256_header,
    load_subscriptions, normalize_subscription_name, save_subscriptions, subscriptions_path,
};
use rand::distr::{Alphanumeric, SampleString};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};

use crate::cli_args::WebhookCommand;

pub async fn run(command: WebhookCommand) -> anyhow::Result<()> {
    let report = run_capture(command).await?;
    if !report.trim().is_empty() {
        println!("{report}");
    }
    Ok(())
}

pub async fn run_capture(command: WebhookCommand) -> anyhow::Result<String> {
    match command {
        WebhookCommand::Subscribe {
            name,
            description,
            events,
            prompt,
            skills,
            secret,
            deliver,
            deliver_extra,
            rate_limit,
            max_body_bytes,
        } => {
            subscribe(SubscribeRequest {
                name: &name,
                description: description.as_deref(),
                events: &events,
                prompt: prompt.as_deref(),
                skills: &skills,
                secret,
                deliver: deliver.as_deref(),
                deliver_extra: parse_deliver_extra_pairs(&deliver_extra)?,
                rate_limit,
                max_body_bytes,
            })
            .await
        }
        WebhookCommand::List => list().await,
        WebhookCommand::Remove { name } => remove(&name).await,
        WebhookCommand::Test { name, payload } => test(&name, payload.as_deref()).await,
        WebhookCommand::Path => Ok(subscriptions_path().display().to_string()),
    }
}

struct SubscribeRequest<'a> {
    name: &'a str,
    description: Option<&'a str>,
    events: &'a [String],
    prompt: Option<&'a str>,
    skills: &'a [String],
    secret: Option<String>,
    deliver: Option<&'a str>,
    deliver_extra: std::collections::BTreeMap<String, String>,
    rate_limit: Option<u32>,
    max_body_bytes: Option<usize>,
}

pub fn command_from_slash_args(args: &str) -> Result<WebhookCommand, String> {
    let parts = crate::mcp_support::parse_inline_command_tokens(args.trim())?;
    match parts.first().map(String::as_str) {
        None | Some("list") | Some("ls") => Ok(WebhookCommand::List),
        Some("path") => Ok(WebhookCommand::Path),
        Some("remove") | Some("rm") | Some("delete") => {
            let Some(name) = parts.get(1).cloned() else {
                return Err(webhook_usage().into());
            };
            Ok(WebhookCommand::Remove { name })
        }
        Some("test") => {
            let Some(name) = parts.get(1).cloned() else {
                return Err(webhook_usage().into());
            };
            let payload = parse_named_option(&parts[2..], "payload")?;
            Ok(WebhookCommand::Test { name, payload })
        }
        Some("subscribe") | Some("add") => {
            let Some(name) = parts.get(1).cloned() else {
                return Err(webhook_usage().into());
            };
            let options = parse_subscribe_options(&parts[2..])?;
            Ok(WebhookCommand::Subscribe {
                name,
                description: options.description,
                events: options.events,
                prompt: options.prompt,
                skills: options.skills,
                secret: options.secret,
                deliver: options.deliver,
                deliver_extra: options.deliver_extra,
                rate_limit: options.rate_limit,
                max_body_bytes: options.max_body_bytes,
            })
        }
        Some(_) => Err(webhook_usage().into()),
    }
}

async fn subscribe(request: SubscribeRequest<'_>) -> anyhow::Result<String> {
    let mut subscriptions = load_subscriptions()?;
    let secret = request.secret.unwrap_or_else(generate_secret);
    let subscription = create_subscription(CreateSubscriptionParams {
        name: request.name,
        description: request.description,
        events: request.events,
        prompt: request.prompt,
        skills: request.skills,
        secret: secret.clone(),
        deliver: request.deliver,
        deliver_extra: request.deliver_extra,
        rate_limit_per_minute: request.rate_limit,
        max_body_bytes: request.max_body_bytes,
    })?;
    let existed = subscriptions
        .insert(subscription.name.clone(), subscription.clone())
        .is_some();
    save_subscriptions(&subscriptions)?;

    let config = AppConfig::load()?;
    let action = if existed { "Updated" } else { "Created" };
    let mut out = String::new();
    writeln!(out, "{action} webhook subscription: {}", subscription.name)?;
    writeln!(
        out,
        "URL:    {}/webhooks/{}",
        base_url(&config.gateway),
        subscription.name
    )?;
    writeln!(out, "Secret: {secret}")?;
    writeln!(out, "Events: {}", render_events(&subscription.events))?;
    writeln!(
        out,
        "Deliver: {}",
        render_deliver(&subscription.deliver, &subscription.deliver_extra)
    )?;
    writeln!(out, "Rate:   {}/min", subscription.rate_limit_per_minute)?;
    writeln!(out, "Body:   {} bytes", subscription.max_body_bytes)?;
    if !subscription.skills.is_empty() {
        writeln!(out, "Skills: {}", subscription.skills.join(", "))?;
    }
    if !subscription.prompt.is_empty() {
        writeln!(out, "Prompt: {}", subscription.prompt)?;
    }
    Ok(out.trim_end().to_string())
}

async fn list() -> anyhow::Result<String> {
    let subscriptions = load_subscriptions()?;
    if subscriptions.is_empty() {
        return Ok("No dynamic webhook subscriptions.".into());
    }

    let config = AppConfig::load()?;
    let base = base_url(&config.gateway);
    let mut out = String::new();
    for subscription in subscriptions.values() {
        writeln!(out, "{}", subscription.name)?;
        writeln!(out, "  URL:     {}/webhooks/{}", base, subscription.name)?;
        writeln!(out, "  Events:  {}", render_events(&subscription.events))?;
        writeln!(
            out,
            "  Deliver: {}",
            render_deliver(&subscription.deliver, &subscription.deliver_extra)
        )?;
        writeln!(out, "  Rate:    {}/min", subscription.rate_limit_per_minute)?;
        writeln!(out, "  Body:    {} bytes", subscription.max_body_bytes)?;
        writeln!(out, "  Created: {}", subscription.created_at)?;
        if !subscription.description.is_empty() {
            writeln!(out, "  About:   {}", subscription.description)?;
        }
        if !subscription.prompt.is_empty() {
            writeln!(out, "  Prompt:  {}", subscription.prompt)?;
        }
        if !subscription.skills.is_empty() {
            writeln!(out, "  Skills:  {}", subscription.skills.join(", "))?;
        }
    }
    Ok(out.trim_end().to_string())
}

async fn remove(name: &str) -> anyhow::Result<String> {
    let mut subscriptions = load_subscriptions()?;
    let normalized = normalize_subscription_name(name)?;
    if subscriptions.remove(&normalized).is_none() {
        return Err(anyhow!("no subscription named '{normalized}'"));
    }
    save_subscriptions(&subscriptions)?;
    Ok(format!("Removed webhook subscription: {normalized}"))
}

async fn test(name: &str, payload: Option<&str>) -> anyhow::Result<String> {
    let subscriptions = load_subscriptions()?;
    let normalized = normalize_subscription_name(name)?;
    let subscription = subscriptions
        .get(&normalized)
        .ok_or_else(|| anyhow!("no subscription named '{normalized}'"))?;

    let config = AppConfig::load()?;
    let url = format!("{}/webhooks/{}", base_url(&config.gateway), normalized);
    let payload = payload.unwrap_or(
        "{\"test\":true,\"event_type\":\"test\",\"message\":\"Hello from edgecrab webhook test\"}",
    );

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "X-Hub-Signature-256",
        HeaderValue::from_str(&hmac_sha256_header(&subscription.secret, payload))
            .context("failed to build signature header")?,
    );
    headers.insert("X-Event-Type", HeaderValue::from_static("test"));

    let response = reqwest::Client::new()
        .post(&url)
        .headers(headers)
        .body(payload.to_string())
        .send()
        .await
        .with_context(|| format!("failed to POST {url}"))?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Ok(format!("{status} {body}"))
}

#[derive(Default)]
struct SubscribeOptions {
    description: Option<String>,
    events: Vec<String>,
    prompt: Option<String>,
    skills: Vec<String>,
    secret: Option<String>,
    deliver: Option<String>,
    deliver_extra: Vec<String>,
    rate_limit: Option<u32>,
    max_body_bytes: Option<usize>,
}

fn parse_subscribe_options(parts: &[String]) -> Result<SubscribeOptions, String> {
    let mut idx = 0usize;
    let mut options = SubscribeOptions::default();
    while idx < parts.len() {
        let current = &parts[idx];
        match current.as_str() {
            "--description" => {
                let Some(value) = parts.get(idx + 1).cloned() else {
                    return Err("Missing value for --description".into());
                };
                options.description = Some(value);
                idx += 2;
            }
            "--events" => {
                let Some(value) = parts.get(idx + 1) else {
                    return Err("Missing value for --events".into());
                };
                options.events = parse_csv(value);
                idx += 2;
            }
            "--prompt" => {
                let Some(value) = parts.get(idx + 1).cloned() else {
                    return Err("Missing value for --prompt".into());
                };
                options.prompt = Some(value);
                idx += 2;
            }
            "--skill" => {
                let Some(value) = parts.get(idx + 1).cloned() else {
                    return Err("Missing value for --skill".into());
                };
                options.skills.extend(parse_csv(&value));
                idx += 2;
            }
            "--secret" => {
                let Some(value) = parts.get(idx + 1).cloned() else {
                    return Err("Missing value for --secret".into());
                };
                options.secret = Some(value);
                idx += 2;
            }
            "--deliver" => {
                let Some(value) = parts.get(idx + 1).cloned() else {
                    return Err("Missing value for --deliver".into());
                };
                options.deliver = Some(value);
                idx += 2;
            }
            "--deliver-extra" => {
                let Some(value) = parts.get(idx + 1).cloned() else {
                    return Err("Missing value for --deliver-extra".into());
                };
                options.deliver_extra.push(value);
                idx += 2;
            }
            "--rate-limit" => {
                let Some(value) = parts.get(idx + 1) else {
                    return Err("Missing value for --rate-limit".into());
                };
                options.rate_limit = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| "Invalid value for --rate-limit".to_string())?,
                );
                idx += 2;
            }
            "--max-body-bytes" => {
                let Some(value) = parts.get(idx + 1) else {
                    return Err("Missing value for --max-body-bytes".into());
                };
                options.max_body_bytes = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| "Invalid value for --max-body-bytes".to_string())?,
                );
                idx += 2;
            }
            _ if current.starts_with("--description=") => {
                options.description = Some(current["--description=".len()..].to_string());
                idx += 1;
            }
            _ if current.starts_with("--events=") => {
                options.events = parse_csv(&current["--events=".len()..]);
                idx += 1;
            }
            _ if current.starts_with("--prompt=") => {
                options.prompt = Some(current["--prompt=".len()..].to_string());
                idx += 1;
            }
            _ if current.starts_with("--secret=") => {
                options.secret = Some(current["--secret=".len()..].to_string());
                idx += 1;
            }
            _ if current.starts_with("--skill=") => {
                options
                    .skills
                    .extend(parse_csv(&current["--skill=".len()..]));
                idx += 1;
            }
            _ if current.starts_with("--deliver=") => {
                options.deliver = Some(current["--deliver=".len()..].to_string());
                idx += 1;
            }
            _ if current.starts_with("--deliver-extra=") => {
                options
                    .deliver_extra
                    .push(current["--deliver-extra=".len()..].to_string());
                idx += 1;
            }
            _ if current.starts_with("--rate-limit=") => {
                options.rate_limit = Some(
                    current["--rate-limit=".len()..]
                        .parse::<u32>()
                        .map_err(|_| "Invalid value for --rate-limit".to_string())?,
                );
                idx += 1;
            }
            _ if current.starts_with("--max-body-bytes=") => {
                options.max_body_bytes = Some(
                    current["--max-body-bytes=".len()..]
                        .parse::<usize>()
                        .map_err(|_| "Invalid value for --max-body-bytes".to_string())?,
                );
                idx += 1;
            }
            _ => return Err(format!("Unexpected argument: {current}")),
        }
    }
    Ok(options)
}

fn parse_named_option(parts: &[String], key: &str) -> Result<Option<String>, String> {
    let mut idx = 0usize;
    let mut value = None;
    while idx < parts.len() {
        let current = &parts[idx];
        if current == &format!("--{key}") {
            let Some(next) = parts.get(idx + 1).cloned() else {
                return Err(format!("Missing value for --{key}"));
            };
            value = Some(next);
            idx += 2;
            continue;
        }
        if let Some(inline) = current.strip_prefix(&format!("--{key}=")) {
            value = Some(inline.to_string());
            idx += 1;
            continue;
        }
        return Err(format!("Unexpected argument: {current}"));
    }
    Ok(value)
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn render_events(events: &[String]) -> String {
    if events.is_empty() {
        "(all)".into()
    } else {
        events.join(", ")
    }
}

fn generate_secret() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 32)
}

fn webhook_usage() -> &'static str {
    "Usage: /webhook [list|subscribe <name> [--events push,pull_request] [--description text] [--prompt text] [--skill review] [--secret token|--secret _INSECURE_NO_AUTH] [--deliver telegram] [--deliver-extra chat_id=12345] [--rate-limit 30] [--max-body-bytes 1048576]|remove <name>|test <name> [--payload json]|path]"
}

fn parse_deliver_extra_pairs(
    pairs: &[String],
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let mut out = std::collections::BTreeMap::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(anyhow!(
                "invalid --deliver-extra '{pair}' (expected key=value)"
            ));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(anyhow!("invalid --deliver-extra '{pair}' (empty key)"));
        }
        out.insert(key.to_string(), value.trim().to_string());
    }
    Ok(out)
}

fn render_deliver(deliver: &str, extra: &std::collections::BTreeMap<String, String>) -> String {
    if extra.is_empty() {
        return deliver.to_string();
    }
    let pairs = extra
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{deliver} ({pairs})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_subscribe_from_slash_args() {
        let command = command_from_slash_args(
            "subscribe github --events push,pull_request --prompt 'Summarize it'",
        )
        .unwrap();
        match command {
            WebhookCommand::Subscribe {
                name,
                events,
                prompt,
                ..
            } => {
                assert_eq!(name, "github");
                assert_eq!(events, vec!["push", "pull_request"]);
                assert_eq!(prompt.as_deref(), Some("Summarize it"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_subscribe_delivery_and_skills_from_slash_args() {
        let command = command_from_slash_args(
            "subscribe github --skill code-review,triage --deliver github_comment --deliver-extra repo=org/repo --deliver-extra pr_number=42",
        )
        .unwrap();
        match command {
            WebhookCommand::Subscribe {
                name,
                skills,
                deliver,
                deliver_extra,
                ..
            } => {
                assert_eq!(name, "github");
                assert_eq!(skills, vec!["code-review", "triage"]);
                assert_eq!(deliver.as_deref(), Some("github_comment"));
                assert_eq!(
                    deliver_extra,
                    vec!["repo=org/repo".to_string(), "pr_number=42".to_string()]
                );
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_list_as_default() {
        let command = command_from_slash_args("").unwrap();
        assert!(matches!(command, WebhookCommand::List));
    }
}
