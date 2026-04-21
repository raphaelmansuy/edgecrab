//! # homeassistant — Home Assistant smart home control tools
//!
//! Provides four tools that mirror the hermes-agent HA integration:
//!
//! - `ha_list_entities` — list available entities, optionally filtered by domain
//! - `ha_get_state`     — get the current state of a specific entity
//! - `ha_list_services` — list available services, optionally filtered by domain
//! - `ha_call_service`  — call a HA service on a target entity
//!
//! All tools use the Home Assistant REST API via reqwest.
//!
//! ## Environment variables
//!
//! | Variable     | Required | Description                              |
//! |-------------|----------|------------------------------------------|
//! | `HA_URL`    | Yes      | Home Assistant URL (e.g. http://ha:8123) |
//! | `HA_TOKEN`  | Yes      | Long-lived access token                  |

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::env;
use std::time::Duration;

use edgecrab_types::{ToolError, ToolSchema};

use crate::registry::{ToolContext, ToolHandler};

fn ha_url() -> Option<String> {
    env::var("HA_URL")
        .ok()
        .map(|u| u.trim_end_matches('/').to_string())
}

fn ha_token() -> Option<String> {
    env::var("HA_TOKEN").ok()
}

fn ha_available() -> bool {
    ha_url().is_some() && ha_token().is_some()
}

async fn ha_get(path: &str) -> Result<serde_json::Value, ToolError> {
    let base = ha_url().ok_or_else(|| ToolError::Unavailable {
        tool: "ha".into(),
        reason: "HA_URL not set".into(),
    })?;
    let token = ha_token().ok_or_else(|| ToolError::Unavailable {
        tool: "ha".into(),
        reason: "HA_TOKEN not set".into(),
    })?;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{base}/api/{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "ha".into(),
            message: format!("HA request failed: {e}"),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            tool: "ha".into(),
            message: format!("HA API error {status}: {text}"),
        });
    }

    resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        tool: "ha".into(),
        message: format!("HA JSON parse error: {e}"),
    })
}

async fn ha_post(path: &str, body: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
    let base = ha_url().ok_or_else(|| ToolError::Unavailable {
        tool: "ha".into(),
        reason: "HA_URL not set".into(),
    })?;
    let token = ha_token().ok_or_else(|| ToolError::Unavailable {
        tool: "ha".into(),
        reason: "HA_TOKEN not set".into(),
    })?;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/api/{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .json(body)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| ToolError::ExecutionFailed {
            tool: "ha".into(),
            message: format!("HA request failed: {e}"),
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(ToolError::ExecutionFailed {
            tool: "ha".into(),
            message: format!("HA API error {status}: {text}"),
        });
    }

    resp.json().await.map_err(|e| ToolError::ExecutionFailed {
        tool: "ha".into(),
        message: format!("HA JSON parse error: {e}"),
    })
}

// ─── ha_list_entities ─────────────────────────────────────────────────

pub struct HaListEntitiesTool;

#[derive(Deserialize)]
struct ListEntitiesArgs {
    #[serde(default)]
    domain: Option<String>,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    50
}

#[async_trait]
impl ToolHandler for HaListEntitiesTool {
    fn name(&self) -> &'static str {
        "ha_list_entities"
    }
    fn toolset(&self) -> &'static str {
        "homeassistant"
    }
    fn emoji(&self) -> &'static str {
        "🏠"
    }
    fn is_available(&self) -> bool {
        ha_available()
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "ha_list_entities".into(),
            description: "List Home Assistant entities. Optionally filter by domain (light, switch, sensor, etc).".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "Filter by entity domain (e.g. 'light', 'switch', 'sensor', 'climate')"
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 200,
                        "description": "Maximum entities to return (default 50)"
                    }
                }
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: ListEntitiesArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "ha_list_entities".into(),
                message: format!("Invalid args: {e}"),
            })?;

        let states = ha_get("states").await?;
        let entities = states
            .as_array()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "ha_list_entities".into(),
                message: "Unexpected response format".into(),
            })?;

        let limit = args.limit.clamp(1, 200);
        let mut results: Vec<serde_json::Value> = Vec::new();

        for entity in entities {
            let eid = entity["entity_id"].as_str().unwrap_or("");
            if let Some(ref domain) = args.domain
                && !eid.starts_with(&format!("{domain}."))
            {
                continue;
            }
            results.push(json!({
                "entity_id": eid,
                "state": entity["state"],
                "friendly_name": entity["attributes"]["friendly_name"],
            }));
            if results.len() >= limit {
                break;
            }
        }

        Ok(serde_json::to_string_pretty(&json!({
            "count": results.len(),
            "entities": results,
        }))
        .unwrap_or_default())
    }
}

static HA_LIST_ENTITIES: HaListEntitiesTool = HaListEntitiesTool;
inventory::submit!(&HA_LIST_ENTITIES as &dyn ToolHandler);

// ─── ha_get_state ─────────────────────────────────────────────────────

pub struct HaGetStateTool;

#[derive(Deserialize)]
struct GetStateArgs {
    entity_id: String,
}

#[async_trait]
impl ToolHandler for HaGetStateTool {
    fn name(&self) -> &'static str {
        "ha_get_state"
    }
    fn toolset(&self) -> &'static str {
        "homeassistant"
    }
    fn emoji(&self) -> &'static str {
        "🏠"
    }
    fn is_available(&self) -> bool {
        ha_available()
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "ha_get_state".into(),
            description: "Get the current state and attributes of a Home Assistant entity.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "entity_id": {
                        "type": "string",
                        "description": "Entity ID (e.g. 'light.living_room', 'sensor.temperature')"
                    }
                },
                "required": ["entity_id"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: GetStateArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "ha_get_state".into(),
                message: format!("Invalid args: {e}"),
            })?;

        let state = ha_get(&format!("states/{}", args.entity_id)).await?;
        Ok(serde_json::to_string_pretty(&state).unwrap_or_default())
    }
}

static HA_GET_STATE: HaGetStateTool = HaGetStateTool;
inventory::submit!(&HA_GET_STATE as &dyn ToolHandler);

// ─── ha_list_services ─────────────────────────────────────────────────

pub struct HaListServicesTool;

#[derive(Deserialize)]
struct ListServicesArgs {
    #[serde(default)]
    domain: Option<String>,
}

#[async_trait]
impl ToolHandler for HaListServicesTool {
    fn name(&self) -> &'static str {
        "ha_list_services"
    }
    fn toolset(&self) -> &'static str {
        "homeassistant"
    }
    fn emoji(&self) -> &'static str {
        "🏠"
    }
    fn is_available(&self) -> bool {
        ha_available()
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "ha_list_services".into(),
            description: "List available Home Assistant services. Optionally filter by domain."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "Filter by service domain (e.g. 'light', 'switch', 'climate')"
                    }
                }
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: ListServicesArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "ha_list_services".into(),
                message: format!("Invalid args: {e}"),
            })?;

        let services = ha_get("services").await?;
        let services_arr = services
            .as_array()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool: "ha_list_services".into(),
                message: "Unexpected response format".into(),
            })?;

        let mut results: Vec<serde_json::Value> = Vec::new();
        for svc_domain in services_arr {
            let domain = svc_domain["domain"].as_str().unwrap_or("");
            if let Some(ref filter) = args.domain
                && domain != filter.as_str()
            {
                continue;
            }
            if let Some(svcs) = svc_domain["services"].as_object() {
                for (name, details) in svcs {
                    results.push(json!({
                        "domain": domain,
                        "service": name,
                        "description": details["description"],
                    }));
                }
            }
        }

        Ok(serde_json::to_string_pretty(&json!({
            "count": results.len(),
            "services": results,
        }))
        .unwrap_or_default())
    }
}

static HA_LIST_SERVICES: HaListServicesTool = HaListServicesTool;
inventory::submit!(&HA_LIST_SERVICES as &dyn ToolHandler);

// ─── ha_call_service ──────────────────────────────────────────────────

pub struct HaCallServiceTool;

#[derive(Deserialize)]
struct CallServiceArgs {
    domain: String,
    service: String,
    entity_id: Option<String>,
    #[serde(default)]
    data: serde_json::Value,
}

#[async_trait]
impl ToolHandler for HaCallServiceTool {
    fn name(&self) -> &'static str {
        "ha_call_service"
    }
    fn toolset(&self) -> &'static str {
        "homeassistant"
    }
    fn emoji(&self) -> &'static str {
        "🏠"
    }
    fn is_available(&self) -> bool {
        ha_available()
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "ha_call_service".into(),
            description: "Call a Home Assistant service on a target entity.\n\
                Examples: turn on a light, set thermostat temperature, lock a door."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "Service domain (e.g. 'light', 'switch', 'climate')"
                    },
                    "service": {
                        "type": "string",
                        "description": "Service name (e.g. 'turn_on', 'turn_off', 'set_temperature')"
                    },
                    "entity_id": {
                        "type": "string",
                        "description": "Target entity ID (e.g. 'light.living_room')"
                    },
                    "data": {
                        "type": "object",
                        "description": "Additional service data (e.g. brightness, temperature)"
                    }
                },
                "required": ["domain", "service"]
            }),
            strict: None,
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<String, ToolError> {
        let args: CallServiceArgs =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs {
                tool: "ha_call_service".into(),
                message: format!("Invalid args: {e}"),
            })?;

        let mut body = if args.data.is_object() {
            args.data.clone()
        } else {
            json!({})
        };

        if let Some(ref eid) = args.entity_id {
            body["entity_id"] = json!(eid);
        }

        let result = ha_post(&format!("services/{}/{}", args.domain, args.service), &body).await?;

        Ok(serde_json::to_string_pretty(&json!({
            "status": "called",
            "domain": args.domain,
            "service": args.service,
            "entity_id": args.entity_id,
            "result": result,
        }))
        .unwrap_or_default())
    }
}

static HA_CALL_SERVICE: HaCallServiceTool = HaCallServiceTool;
inventory::submit!(&HA_CALL_SERVICE as &dyn ToolHandler);

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ha_tools_unavailable_without_env() {
        // Without HA_URL and HA_TOKEN, tools should report unavailable
        // (unless CI sets them)
        if env::var("HA_URL").is_err() {
            assert!(!ha_available());
        }
    }

    #[test]
    fn ha_schemas_are_valid() {
        assert_eq!(HaListEntitiesTool.name(), "ha_list_entities");
        assert_eq!(HaGetStateTool.name(), "ha_get_state");
        assert_eq!(HaListServicesTool.name(), "ha_list_services");
        assert_eq!(HaCallServiceTool.name(), "ha_call_service");
    }
}
