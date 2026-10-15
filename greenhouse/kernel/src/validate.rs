// src/validate.rs — the manifest an app registers with (manifest-schema.md
// D1-D9), checked and normalized before it becomes a contract proposal.
//
// Stand-in: kernel/src/validate.rs was never captured whole. The types, the
// name predicates, and the parsing of events, secrets, egress, model_compute,
// notify_categories and schedules are verbatim fragments (main, 2026-09-12 to
// 2026-10-14). The header, the helpers arr_field and str_field, the id,
// version, health, tools, ui and notify channel parsing, and the tests are
// reconstructed from the captured rules and error texts. Lines marked
// "Stand-in" below are those reconstructions.

use crate::error::{err, ApiError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const EFFECTS: [&str; 3] = ["read", "write", "command"];
// Stand-in: the channel list was not captured. node-monitor uses push and
// in-app, the schema names push, email and in-app, and sms is refused.
const CHANNELS: [&str; 3] = ["push", "email", "in-app"];
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

// Stand-in: an absent or null list is empty; anything else must be a list.
fn arr_field<'a>(v: &'a Value, key: &str) -> Result<&'a [Value], ApiError> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(&[]),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(bad(format!("{key} must be a list"))),
    }
}
// Stand-in.
fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

pub fn validate_manifest(m: &Value) -> Result<Manifest, ApiError> {
    // Stand-in: id, version and health, from the documented rules.
    let id = str_field(m, "id")
        .filter(|x| is_app_id(x))
        .ok_or_else(|| bad("id must match ^[a-z][a-z0-9-]*$ (D1)"))?;
    let version = str_field(m, "version")
        .filter(|x| is_semver(x))
        .ok_or_else(|| bad("version must be MAJOR.MINOR.PATCH (D1)"))?;
    let health = str_field(m, "health")
        .filter(|x| x.starts_with('/'))
        .ok_or_else(|| bad("health must be an app path starting with / (D1)"))?;

    // Stand-in: tools. The description and parameters checks are verbatim
    // (wt-expansion grep, 2026-09-12); names, uniqueness and effect are not.
    let mut tools: Vec<Tool> = Vec::new();
    for t in arr_field(m, "tools")? {
        let name = str_field(t, "name")
            .filter(|x| is_tool_name(x))
            .ok_or_else(|| {
                bad(format!(
                    "tool name '{}' must match ^[a-z][a-z0-9_]*$ (D2)",
                    str_field(t, "name").unwrap_or("?")
                ))
            })?;
        if tools.iter().any(|tool| tool.name == name) {
            return Err(bad(format!("tool '{name}' is declared twice (D2)")));
        }
        let description = str_field(t, "description")
            .filter(|x| !x.trim().is_empty())
            .ok_or_else(|| bad(format!("tool '{name}' needs a description (D2)")))?;
        let parameters = t.get("parameters")
            .filter(|p| p.is_object() && p.get("type").and_then(|v| v.as_str()) == Some("object"))
            .ok_or_else(|| bad(format!("tool '{name}' parameters must be a JSON Schema object ({{type:\"object\",...}}) (D2)")))?;
        let effect = str_field(t, "effect")
            .filter(|x| EFFECTS.contains(x))
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
    let names: std::collections::HashSet<&str> = tools.iter().map(|t| t.name.as_str()).collect();

    // Stand-in: ui. The two error texts are verbatim.
    let ui = match m.get("ui") {
        None | Some(Value::Null) => None,
        Some(u) => {
            let route = str_field(u, "route")
                .filter(|r| r.starts_with('/'))
                .ok_or_else(|| bad("ui.route must start with / (D3)"))?;
            let label = str_field(u, "label")
                .filter(|l| !l.is_empty())
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
        // Stand-in: the channel check (lines 263-271) was not captured.
        let channel = c.as_str().filter(|x| CHANNELS.contains(x)).ok_or_else(|| {
            bad(format!(
                "notify channel '{}' must be one of {} (D7)",
                c.as_str().unwrap_or("?"),
                CHANNELS.join("|")
            ))
        })?;
        permissions.notify.push(channel.into());
    }
    for nc in arr_field(&p, "notify_categories")? {
        let category = str_field(nc, "category")
            .filter(|x| !x.is_empty())
            .ok_or_else(|| bad("notify_categories entries need a category (D7)"))?;
        // Stand-in: lines 277-279 and 281-285 were not captured.
        let sensitivity = str_field(nc, "sensitivity")
            .filter(|x| SENSITIVITIES.contains(x))
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

    // Stand-in: the kernel's own `good()` is node-monitor's manifest; its
    // captured lines (351-366) are kept and the rest is filled in.
    fn good() -> Value {
        json!({
            "id": "node-monitor",
            "version": "0.2.0",
            "health": "/health",
            "tools": [
                { "name": "get_status", "description": "Current state", "parameters": { "type": "object", "properties": { "node": { "type": "string" } } }, "effect": "read" },
                { "name": "rollup_hourly", "description": "Roll metrics", "parameters": { "type": "object" }, "effect": "command" }
            ],
            "ui": { "route": "/nodes", "label": "Nodes" },
            "events": { "emits": ["node.down"], "subscribes": [] },
            "permissions": {
                "secrets": [], "egress": [], "model_compute": ["embeddings"],
                "notify": ["push", "in-app"],
                "notify_categories": [{ "category": "node-down", "sensitivity": "public" }]
            },
            "schedules": [{ "name": "rollup.hourly", "cron": "0 * * * *", "action": "rollup_hourly" }]
        })
    }

    fn rejects(change: impl Fn(&mut Value), needle: &str) {
        let mut m = good();
        change(&mut m);
        match validate_manifest(&m) {
            Ok(_) => panic!("accepted a manifest that should fail with '{needle}'"),
            Err(e) => {
                assert_eq!(e.status.as_u16(), 400);
                assert!(
                    e.message.contains(needle),
                    "'{}' does not mention '{needle}'",
                    e.message
                );
            }
        }
    }

    /// weather-app's manifest() (main, Write 2026-09-26T11:46Z) with the
    /// example configuration's home.
    fn weather() -> Value {
        json!({
            "id": "weather",
            "version": "0.1.0",
            "health": "/health",
            "tools": [{
                "name": "get_weather",
                "description": "Weather for a place: conditions now, the next hours and the coming days with the chance and amount of precipitation, official severe-weather warnings (US and Canada), and a comparison of what each weather service says. Leave place empty for home (Troy, New York, US). For anywhere else give place (the city or town), and region (state or province) and country (e.g. \"US\", \"CA\") when known. Values are in the place's usual units (°F, mph and inches in the US). Mention any warnings first, and say so when the services disagree noticeably.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "place": {"type": "string", "maxLength": 100},
                        "region": {"type": "string", "maxLength": 60},
                        "country": {"type": "string", "maxLength": 60},
                        "hours": {"type": "integer", "minimum": 0, "maximum": 48},
                        "days": {"type": "integer", "minimum": 1, "maximum": 7}
                    }
                },
                "effect": "read"
            }, {
                "name": "check_warnings",
                "description": "Scheduled, not for chat: looks for official warnings for home that have not been announced yet and raises an in-app notification for each.",
                "parameters": {"type": "object", "properties": {}},
                "effect": "command"
            }],
            "events": { "emits": [], "subscribes": [] },
            "schedules": [{ "name": "warnings.home", "cron": "*/10 * * * *", "action": "check_warnings" }],
            "permissions": {
                "secrets": [],
                "egress": [
                    "api.weather.gov",
                    "api.weather.gc.ca",
                    "api.open-meteo.com",
                    "geocoding-api.open-meteo.com",
                    "api.met.no"
                ],
                "notify": ["in-app"],
                "notify_categories": [{ "category": "weather-warning", "sensitivity": "public" }]
            }
        })
    }

    /// home-assistant-app's manifest() (main, 2026-10 reconstruction) with the
    /// example configuration's Home Assistant host.
    fn home_assistant() -> Value {
        json!({
            "id": "home-assistant",
            "version": "0.1.0",
            "health": "/health",
            "tools": [
                {
                    "name": "list_lights",
                    "description": "List the household lights: each light's name, whether it is on, off or unavailable, its brightness and colour when on, and what it can do (can: on_off, dim, white_temperature, colour). Rooms and zones appear too, with kind 'room' or 'zone' and their member lights; naming a room in set_lights controls the whole room. readiness 'fresh' is current; 'stale' means Home Assistant could not be reached and the state is the last one known.",
                    "parameters": { "type": "object", "properties": {} },
                    "effect": "read"
                },
                {
                    "name": "set_lights",
                    "description": "Turn lights or whole rooms on or off, toggle them, or set brightness or colour, all in one call. changes is a list; each change names lights or rooms the way the user says them (for example [\"bedroom\"], [\"front window\", \"office\"] or [\"all\"]; there is no need to call list_lights first) and gives them one setting: action turn_on, turn_off or toggle, and with turn_on optionally brightness_pct 1-100 and either color_temp_kelvin (2700 warm, 4000 neutral, 6500 cool) or rgb_color [r, g, b]. Use one change per different setting, e.g. bedroom off and living room at 30%: [{\"lights\": [\"bedroom\"], \"action\": \"turn_off\"}, {\"lights\": [\"living room\"], \"action\": \"turn_on\", \"brightness_pct\": 30}]. Name each light or room in only one change. Each light is only asked for what it can do. The result gives every light an outcome: 'done' (Home Assistant now shows it), 'not_done' with the reason, 'not_confirmed' (asked, but Home Assistant does not show the change) or 'unknown'. Tell the user exactly those outcomes: a light has changed only if its outcome is 'done'.",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "changes": {
                                "type": "array",
                                "minItems": 1,
                                "maxItems": 10,
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "lights": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 25 },
                                        "action": { "type": "string", "enum": ["turn_on", "turn_off", "toggle"] },
                                        "brightness_pct": { "type": "integer", "minimum": 1, "maximum": 100 },
                                        "color_temp_kelvin": { "type": "integer", "minimum": 1000, "maximum": 10000 },
                                        "rgb_color": { "type": "array", "items": { "type": "integer", "minimum": 0, "maximum": 255 }, "minItems": 3, "maxItems": 3 }
                                    },
                                    "required": ["lights", "action"]
                                }
                            }
                        },
                        "required": ["changes"]
                    },
                    "effect": "write"
                },
                {
                    "name": "house_status",
                    "description": "The house right now, from Home Assistant: temperatures and humidity, light level, motion, doors and windows open or closed, leaks and smoke, thermostats (read only) and sunrise and sunset. Only what the owner allowlisted; 'unavailable' lists sensors Home Assistant cannot read now, and a 'note' says when Home Assistant could not be reached at all. Times are UTC in ISO form.",
                    "parameters": { "type": "object", "properties": {} },
                    "effect": "read"
                },
                {
                    "name": "activate_scene",
                    "description": "Activate one of the household's lighting scenes by name, e.g. \"Bedroom Relax\" (list_lights lists them under scenes). The outcome says whether Home Assistant accepted it: 'done', 'not_done' with the reason, or 'unknown'.",
                    "parameters": {
                        "type": "object",
                        "properties": { "scene": { "type": "string", "minLength": 1, "maxLength": 120 } },
                        "required": ["scene"]
                    },
                    "effect": "write"
                },
                {
                    "name": "check_house",
                    "description": "Scheduled, not for chat: announces a low battery or a leak, smoke, gas or carbon monoxide sensor going off among the allowlisted sensors, once per episode, as an in-app notification.",
                    "parameters": { "type": "object", "properties": {} },
                    "effect": "command"
                }
            ],
            "events": { "emits": [], "subscribes": [] },
            "schedules": [{ "name": "house.check", "cron": "*/5 * * * *", "action": "check_house" }],
            "permissions": {
                "secrets": ["ha_token"],
                "egress": ["192.168.1.118"],
                "notify": ["in-app"],
                "notify_categories": [{ "category": "home-alert", "sensitivity": "private" }]
            }
        })
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
    fn the_weather_manifest_is_accepted() {
        let m = validate_manifest(&weather()).unwrap();
        assert_eq!((m.id.as_str(), m.version.as_str()), ("weather", "0.1.0"));
        let tools: Vec<(&str, &str)> = m
            .tools
            .iter()
            .map(|t| (t.name.as_str(), t.effect.as_str()))
            .collect();
        assert_eq!(
            tools,
            vec![("get_weather", "read"), ("check_warnings", "command")]
        );
        assert_eq!(m.schedules[0].cron, "*/10 * * * *");
        assert_eq!(m.permissions.egress.len(), 5);
        assert!(m.egress_wildcards.is_empty());
        assert!(m.ui.is_none());
        assert_eq!(m.permissions.notify, vec!["in-app"]);
        // Serialized, an empty model_compute is left out.
        let wire = serde_json::to_value(&m).unwrap();
        assert!(wire["permissions"].get("model_compute").is_none());
        assert_eq!(wire["egressWildcards"], json!([]));
    }

    #[test]
    fn the_home_assistant_manifest_is_accepted() {
        let m = validate_manifest(&home_assistant()).unwrap();
        assert_eq!(m.id, "home-assistant");
        assert_eq!(m.tools.len(), 5);
        assert_eq!(m.permissions.secrets, vec!["ha_token"]);
        assert_eq!(m.permissions.egress, vec!["192.168.1.118"]);
        assert_eq!(
            m.permissions.notify_categories,
            vec![NotifyCategory {
                category: "home-alert".into(),
                sensitivity: "private".into()
            }]
        );
        assert_eq!(
            m.schedules,
            vec![Schedule {
                name: "house.check".into(),
                cron: "*/5 * * * *".into(),
                action: "check_house".into()
            }]
        );
    }

    #[test]
    fn the_body_double_manifest_shape_is_accepted() {
        // The shape in apps/body-double.md, with placeholder descriptions.
        let tool = |name: &str, effect: &str| {
            json!({
                "name": name,
                "description": format!("{name} (placeholder)"),
                "parameters": {"type": "object", "properties": {}, "additionalProperties": false},
                "effect": effect
            })
        };
        let manifest = json!({
            "id": "body-double",
            "version": "0.1.0",
            "health": "/health",
            "tools": [
                tool("now", "read"), tool("day", "read"), tool("export", "read"),
                tool("log", "write"), tool("set_task", "write"), tool("update_task", "write"),
                tool("open_thread", "write"), tool("close_thread", "write"),
                tool("checkins", "write"), tool("checkin_tick", "command")
            ],
            "events": { "emits": [], "subscribes": [] },
            "schedules": [{ "name": "checkins.tick", "cron": "* * * * *", "action": "checkin_tick" }],
            "permissions": {
                "secrets": [], "egress": [],
                "notify": ["in-app"],
                "notify_categories": [{ "category": "body-double-checkin", "sensitivity": "private" }]
            }
        });
        let m = validate_manifest(&manifest).unwrap();
        assert_eq!(m.tools.len(), 10);
        assert_eq!(m.schedules[0].action, "checkin_tick");
        assert!(m.permissions.model_compute.is_empty());
    }

    #[test]
    fn absent_lists_are_empty() {
        let m = validate_manifest(&json!({"id": "stub", "version": "0.1.0", "health": "/health"}))
            .unwrap();
        assert!(m.tools.is_empty());
        assert!(m.ui.is_none());
        assert!(m.schedules.is_empty());
        assert!(m.permissions.notify.is_empty());
    }

    #[test]
    fn identity_and_tools_are_checked() {
        rejects(|m| m["id"] = json!("Node_Monitor"), "id must match");
        rejects(|m| m["id"] = json!("9lives"), "id must match");
        rejects(
            |m| {
                m.as_object_mut().unwrap().remove("id");
            },
            "id must match",
        );
        rejects(|m| m["version"] = json!("1.0"), "version");
        rejects(|m| m["health"] = json!("health"), "health");
        rejects(|m| m["tools"][0]["name"] = json!("Get-Status"), "tool name");
        rejects(
            |m| m["tools"][1]["name"] = json!("get_status"),
            "declared twice",
        );
        rejects(
            |m| {
                m["tools"][0].as_object_mut().unwrap().remove("description");
            },
            "needs a description",
        );
        rejects(
            |m| m["tools"][0]["description"] = json!(""),
            "needs a description",
        );
        rejects(
            |m| m["tools"][0]["parameters"] = json!("any"),
            "parameters must be a JSON Schema object",
        );
        rejects(
            |m| m["tools"][0]["parameters"] = json!({"type": "array"}),
            "parameters must be a JSON Schema object",
        );
        rejects(|m| m["tools"][0]["effect"] = json!("admin"), "effect");
        rejects(
            |m| {
                m["tools"][0].as_object_mut().unwrap().remove("effect");
            },
            "effect",
        );
        rejects(|m| m["tools"] = json!({"get_status": {}}), "must be a list");
        rejects(|m| m["ui"]["route"] = json!("nodes"), "ui.route");
        rejects(|m| m["ui"]["label"] = json!(""), "ui.label");
    }

    #[test]
    fn permissions_are_checked() {
        rejects(
            |m| m["events"]["emits"] = json!(["nodedown"]),
            "domain.topic",
        );
        rejects(
            |m| m["permissions"]["secrets"] = json!(["HA token"]),
            "secret name",
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
            |m| m["permissions"]["notify_categories"][0]["category"] = json!(""),
            "need a category",
        );
        let mut m = good();
        m["permissions"]["notify"] = json!(["email", "push", "in-app"]);
        assert!(validate_manifest(&m).is_ok());
    }

    #[test]
    fn schedules_are_checked() {
        rejects(
            |m| m["schedules"][0]["name"] = json!(""),
            "schedule needs a name",
        );
        rejects(
            |m| m["schedules"][0]["action"] = json!("not_a_tool"),
            "must name a declared tool",
        );
        rejects(
            |m| m["schedules"][0]["action"] = json!("get_status"),
            "must name a command tool",
        );
        rejects(
            |m| {
                m["schedules"][0].as_object_mut().unwrap().remove("cron");
            },
            "5 fields",
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
