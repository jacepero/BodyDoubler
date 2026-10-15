// src/validate.rs — manifest validation: where the D1–D9 schema
// (greenhouse/manifest-schema.md) is enforced at registration. 400 on any
// violation, with the D-reference in the message.

use crate::error::{err, ApiError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const EFFECTS: [&str; 3] = ["read", "write", "command"];
const CHANNELS: [&str; 3] = ["push", "in-app", "email"];
const SENSITIVITIES: [&str; 3] = ["public", "private", "secret"];
// `background`: inference outside any person's turn (D18, background.rs).
const MODEL_COMPUTE_CAPABILITIES: [&str; 3] = ["embeddings", "inference", "background"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub effect: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ui {
    pub route: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Events {
    pub emits: Vec<String>,
    pub subscribes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NotifyCategory {
    pub category: String,
    pub sensitivity: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Permissions {
    pub secrets: Vec<String>,
    pub egress: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_compute: Vec<String>,
    pub notify: Vec<String>,
    pub notify_categories: Vec<NotifyCategory>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub name: String,
    pub cron: String,
    pub action: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub health: String,
    pub tools: Vec<Tool>,
    pub ui: Option<Ui>,
    pub events: Events,
    pub permissions: Permissions,
    pub schedules: Vec<Schedule>,
    // D6: wildcards surfaced for owner approval, not silently accepted.
    #[serde(rename = "egressWildcards", default)]
    pub egress_wildcards: Vec<String>,
}

fn bad(m: impl Into<String>) -> ApiError {
    err(400, m)
}

pub fn is_app_id(s: &str) -> bool {
    let mut ch = s.chars();
    matches!(ch.next(), Some(c) if c.is_ascii_lowercase())
        && ch.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
fn is_tool_name(s: &str) -> bool {
    let mut ch = s.chars();
    matches!(ch.next(), Some(c) if c.is_ascii_lowercase())
        && ch.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}
fn is_topic(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
        })
}
fn is_secret_name(s: &str) -> bool {
    let mut ch = s.chars();
    matches!(ch.next(), Some(c) if c.is_ascii_lowercase())
        && ch.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}
/// The egress request for the public web: any public host, on the web's
/// ports. Private, loopback, link-local and tailnet addresses are never part
/// of it. Like every wildcard it is surfaced for the owner's explicit approval.
pub const PUBLIC_WEB_EGRESS: &str = "*";

fn is_hostname(s: &str) -> bool {
    let body = s.strip_prefix("*.").unwrap_or(s);
    !body.is_empty()
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !body.starts_with(['-', '.'])
        && !body.ends_with(['-', '.'])
}
fn is_semver(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}
fn arr_field<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], ApiError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        Some(_) => Err(bad(format!("{key} must be an array"))),
    }
}

pub fn validate_manifest(m: &Value) -> Result<Manifest, ApiError> {
    if !m.is_object() {
        return Err(bad("manifest must be an object"));
    }
    let id = str_field(m, "id")
        .filter(|s| is_app_id(s))
        .ok_or_else(|| bad("id must match ^[a-z][a-z0-9-]*$ (D1)"))?;
    let version = str_field(m, "version")
        .filter(|s| is_semver(s))
        .ok_or_else(|| bad("version must be semver x.y.z (D1)"))?;
    let health = str_field(m, "health")
        .filter(|s| s.starts_with('/'))
        .ok_or_else(|| bad("health must be a path starting with / (D1)"))?;
    if m.get("base_url").is_some() {
        return Err(bad(
            "base_url is kernel-assigned (D1) — remove it from the manifest",
        ));
    }

    let mut tools = Vec::new();
    let mut names = std::collections::HashSet::new();
    for t in arr_field(m, "tools")? {
        let name = str_field(t, "name")
            .filter(|s| is_tool_name(s))
            .ok_or_else(|| {
                bad(format!(
                    "tool name '{}' must match ^[a-z][a-z0-9_]*$ (D2)",
                    t.get("name").and_then(|v| v.as_str()).unwrap_or("?")
                ))
            })?;
        if !names.insert(name.to_string()) {
            return Err(bad(format!("duplicate tool '{name}'")));
        }
        let description = str_field(t, "description")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| bad(format!("tool '{name}' needs a description (D2)")))?;
        let parameters = t.get("parameters")
            .filter(|p| p.is_object() && p.get("type").and_then(|v| v.as_str()) == Some("object"))
            .ok_or_else(|| bad(format!("tool '{name}' parameters must be a JSON Schema object ({{type:\"object\",...}}) (D2)")))?;
        let effect = str_field(t, "effect")
            .filter(|e| EFFECTS.contains(e))
            .ok_or_else(|| {
                bad(format!(
                    "tool '{name}' effect must be one of {} (D2)",
                    EFFECTS.join("|")
                ))
            })?;
        tools.push(Tool {
            name: name.into(),
            description: description.into(),
            parameters: parameters.clone(),
            effect: effect.into(),
        });
    }

    let ui = match m.get("ui") {
        None | Some(Value::Null) => None,
        Some(u) => {
            let route = str_field(u, "route")
                .filter(|s| s.starts_with('/'))
                .ok_or_else(|| bad("ui.route must start with / (D3)"))?;
            let label = str_field(u, "label")
                .filter(|s| !s.is_empty())
                .ok_or_else(|| bad("ui.label required (D3)"))?;
            Some(Ui {
                route: route.into(),
                label: label.into(),
            })
        }
    };

    let mut events = Events::default();
    let ev = m.get("events").cloned().unwrap_or(Value::Null);
    for (key, out) in [
        ("emits", &mut events.emits),
        ("subscribes", &mut events.subscribes),
    ] {
        for t in arr_field(&ev, key)? {
            let topic = t.as_str().filter(|s| is_topic(s)).ok_or_else(|| {
                bad(format!(
                    "topic '{}' must be domain.topic (D4)",
                    t.as_str().unwrap_or("?")
                ))
            })?;
            if !out.contains(&topic.to_string()) {
                out.push(topic.into());
            }
        }
    }

    let mut permissions = Permissions::default();
    let p = m.get("permissions").cloned().unwrap_or(Value::Null);
    for s in arr_field(&p, "secrets")? {
        let name = s.as_str().filter(|x| is_secret_name(x)).ok_or_else(|| {
            bad(format!(
                "secret name '{}' must match ^[a-z][a-z0-9_-]*$ (D5)",
                s.as_str().unwrap_or("?")
            ))
        })?;
        permissions.secrets.push(name.into());
    }
    for h in arr_field(&p, "egress")? {
        let host = h.as_str().filter(|x| *x == PUBLIC_WEB_EGRESS || is_hostname(x)).ok_or_else(|| {
            bad(format!(
                "egress '{}' must be a hostname, optionally *.-prefixed, or * for the public web (D6)",
                h.as_str().unwrap_or("?")
            ))
        })?;
        permissions.egress.push(host.to_lowercase());
    }
    let mut compute = std::collections::HashSet::new();
    for capability in arr_field(&p, "model_compute")? {
        let capability = capability
            .as_str()
            .filter(|value| MODEL_COMPUTE_CAPABILITIES.contains(value))
            .ok_or_else(|| {
                bad(format!(
                    "model_compute capability '{}' must be one of {}",
                    capability.as_str().unwrap_or("?"),
                    MODEL_COMPUTE_CAPABILITIES.join("|")
                ))
            })?;
        if !compute.insert(capability.to_string()) {
            return Err(bad(format!(
                "duplicate model_compute capability '{capability}'"
            )));
        }
        permissions.model_compute.push(capability.into());
    }
    for c in arr_field(&p, "notify")? {
        let ch = c.as_str().filter(|x| CHANNELS.contains(x)).ok_or_else(|| {
            bad(format!(
                "notify channel '{}' must be one of {} (D7)",
                c.as_str().unwrap_or("?"),
                CHANNELS.join("|")
            ))
        })?;
        permissions.notify.push(ch.into());
    }
    let mut cats = std::collections::HashSet::new();
    for nc in arr_field(&p, "notify_categories")? {
        let category = str_field(nc, "category")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| bad("notify_categories entries need a category (D7)"))?;
        if !cats.insert(category.to_string()) {
            return Err(bad(format!("duplicate notify category '{category}'")));
        }
        let sensitivity = str_field(nc, "sensitivity")
            .filter(|s| SENSITIVITIES.contains(s))
            .ok_or_else(|| {
                bad(format!(
                    "category '{category}' sensitivity must be {} (D7)",
                    SENSITIVITIES.join("|")
                ))
            })?;
        permissions.notify_categories.push(NotifyCategory {
            category: category.into(),
            sensitivity: sensitivity.into(),
        });
    }

    let mut schedules = Vec::new();
    for s in arr_field(m, "schedules")? {
        let name = str_field(s, "name")
            .filter(|x| !x.is_empty())
            .ok_or_else(|| bad("schedule needs a name (D8)"))?;
        let action = str_field(s, "action")
            .filter(|a| names.contains(*a))
            .ok_or_else(|| {
                bad(format!(
                    "schedule '{name}' action '{}' must name a declared tool (D8)",
                    str_field(s, "action").unwrap_or("?")
                ))
            })?;
        if tools
            .iter()
            .find(|tool| tool.name == action)
            .is_none_or(|tool| tool.effect != "command")
        {
            return Err(bad(format!(
                "schedule '{name}' action '{action}' must name a command tool (D8)"
            )));
        }
        let cron = str_field(s, "cron").unwrap_or("");
        crate::cron::parse_cron(cron).map_err(|e| bad(format!("schedule '{name}': {e}")))?;
        schedules.push(Schedule {
            name: name.into(),
            cron: cron.into(),
            action: action.into(),
        });
    }

    let egress_wildcards = permissions
        .egress
        .iter()
        .filter(|h| h.starts_with("*.") || *h == PUBLIC_WEB_EGRESS)
        .cloned()
        .collect();

    Ok(Manifest {
        id: id.into(),
        version: version.into(),
        health: health.into(),
        tools,
        ui,
        events,
        permissions,
        schedules,
        egress_wildcards,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn good() -> Value {
        json!({
            "id": "node-monitor",
            "version": "0.1.0",
            "health": "/health",
            "tools": [
                { "name": "get_status", "description": "Current state", "parameters": { "type": "object", "properties": { "node": { "type": "string" } } }, "effect": "read" },
                { "name": "rollup_hourly", "description": "Roll metrics", "parameters": { "type": "object" }, "effect": "command" }
            ],
            "ui": { "route": "/nodes", "label": "Nodes" },
            "events": { "emits": ["node.down", "node.recovered"], "subscribes": [] },
            "permissions": {
                "secrets": [], "egress": [], "model_compute": ["embeddings"],
                "notify": ["push", "in-app"],
                "notify_categories": [{ "category": "node-down", "sensitivity": "public" }]
            },
            "schedules": [{ "name": "rollup.hourly", "cron": "0 * * * *", "action": "rollup_hourly" }]
        })
    }

    fn rejects(mutate: impl FnOnce(&mut Value), needle: &str) {
        let mut m = good();
        mutate(&mut m);
        let e = validate_manifest(&m).expect_err("should reject");
        assert!(
            e.message.contains(needle),
            "'{}' should contain '{needle}'",
            e.message
        );
    }

    #[test]
    fn conforming_manifest_normalizes() {
        let m = validate_manifest(&good()).unwrap();
        assert_eq!(m.id, "node-monitor");
        assert_eq!(m.tools.len(), 2);
        assert_eq!(m.permissions.model_compute, vec!["embeddings"]);
        assert!(m.egress_wildcards.is_empty());
    }

    #[test]
    fn background_inference_is_a_requestable_capability() {
        let mut manifest = good();
        manifest["permissions"]["model_compute"] = json!(["embeddings", "background"]);
        let m = validate_manifest(&manifest).unwrap();
        assert_eq!(
            m.permissions.model_compute,
            vec!["embeddings", "background"]
        );
    }

    #[test]
    fn minimal_gets_defaults() {
        let m = validate_manifest(
            &json!({ "id": "web-search", "version": "0.1.0", "health": "/health" }),
        )
        .unwrap();
        assert!(m.tools.is_empty());
        assert!(m.ui.is_none());
        assert!(m.events.emits.is_empty());
        assert!(m.schedules.is_empty());
    }

    #[test]
    fn d1_d8_rejections() {
        rejects(|m| m["id"] = json!("Node-Monitor"), "id must match");
        rejects(|m| m["version"] = json!("1.0"), "semver");
        rejects(
            |m| m["base_url"] = json!("http://127.0.0.1:9"),
            "kernel-assigned",
        );
        rejects(
            |m| m["tools"][0]["effect"] = json!("delete"),
            "effect must be one of",
        );
        rejects(
            |m| m["tools"][0]["parameters"] = json!("any"),
            "JSON Schema",
        );
        rejects(
            |m| {
                let t = m["tools"][0].clone();
                m["tools"].as_array_mut().unwrap().push(t);
            },
            "duplicate tool",
        );
        rejects(|m| m["ui"]["route"] = json!("nodes"), "ui.route");
        rejects(
            |m| {
                m["events"]["emits"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("nodedown"))
            },
            "domain.topic",
        );
        rejects(
            |m| m["permissions"]["egress"] = json!(["http://x.com"]),
            "hostname",
        );
        rejects(|m| m["permissions"]["notify"] = json!(["sms"]), "channel");
        rejects(
            |m| m["permissions"]["model_compute"] = json!(["raw-provider"]),
            "model_compute capability",
        );
        rejects(
            |m| m["permissions"]["model_compute"] = json!(["embeddings", "embeddings"]),
            "duplicate model_compute",
        );
        rejects(
            |m| m["permissions"]["notify_categories"][0]["sensitivity"] = json!("top-secret"),
            "sensitivity",
        );
        rejects(
            |m| m["schedules"][0]["action"] = json!("not_a_tool"),
            "declared tool",
        );
        rejects(
            |m| m["schedules"][0]["action"] = json!("get_status"),
            "command tool",
        );
        rejects(
            |m| m["schedules"][0]["cron"] = json!("99 * * * *"),
            "out of range",
        );
    }

    #[test]
    fn d6_wildcards_surfaced() {
        let mut m = good();
        m["permissions"]["egress"] = json!(["api.example.com", "*.example.org"]);
        assert_eq!(
            validate_manifest(&m).unwrap().egress_wildcards,
            vec!["*.example.org"]
        );
    }

    #[test]
    fn public_web_egress_is_a_wildcard() {
        let mut m = good();
        m["permissions"]["egress"] = json!(["api.example.com", "*"]);
        assert_eq!(validate_manifest(&m).unwrap().egress_wildcards, vec!["*"]);
        m["permissions"]["egress"] = json!(["*.*"]);
        assert!(validate_manifest(&m).is_err());
        m["permissions"]["egress"] = json!(["**"]);
        assert!(validate_manifest(&m).is_err());
    }
}
