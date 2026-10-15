// Configuration: the household's default time zone, whose check-ins may raise
// Greenhouse notices, and the defaults for a new check-in plan. A root-owned
// file.

use crate::time;
use greenhouse_kernel::clock::{self, Zone};
use serde::Deserialize;

pub const CONFIG_VERSION: &str = "greenhouse-body-double-config-v1";
pub const DEFAULT_CONFIG_FILE: &str = "/etc/greenhouse/apps/body-double/config.json";
pub const DEFAULT_STATE_DIR: &str = "/var/lib/greenhouse/apps/body-double";
pub const DEFAULT_PORT: &str = "4107";

/// Check-in intervals a plan may use, in minutes.
pub const EVERY_MINUTES: std::ops::RangeInclusive<i64> = 10..=240;

/// What a new person's check-in plan starts as.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanDefaults {
    pub on: bool,
    pub every_minutes: i64,
    /// Quiet hours as minutes after local midnight: (from, until).
    pub quiet: Option<(u32, u32)>,
}

impl Default for PlanDefaults {
    fn default() -> Self {
        PlanDefaults {
            on: true,
            every_minutes: 45,
            quiet: Some((22 * 60, 8 * 60)),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCheckins {
    #[serde(default)]
    on: Option<bool>,
    #[serde(default)]
    every_minutes: Option<i64>,
    #[serde(default)]
    quiet_from: Option<String>,
    #[serde(default)]
    quiet_until: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    version: String,
    timezone: String,
    #[serde(default)]
    notices_for: Vec<String>,
    #[serde(default)]
    checkins: Option<FileCheckins>,
}

/// What the configuration file says.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub timezone: String,
    pub notices_for: Vec<String>,
    pub defaults: PlanDefaults,
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub kernel_url: String,
    /// Where the journal lives.
    pub state_dir: std::path::PathBuf,
    pub port: u16,
    pub service_token: String,
    /// The zone of anyone who has not chosen their own.
    pub timezone: String,
    /// The people whose check-ins also raise a Greenhouse notice. Notices are
    /// the household's alerts, read by the owner, so this is meant for the
    /// owner.
    pub notices_for: Vec<String>,
    pub defaults: PlanDefaults,
}

pub fn parse_file_config(text: &str) -> Result<Settings, String> {
    let file: FileConfig =
        serde_json::from_str(text).map_err(|e| format!("configuration is not valid: {e}"))?;
    if file.version != CONFIG_VERSION {
        return Err(format!("configuration version must be {CONFIG_VERSION}"));
    }
    if !clock::valid_zone_name(&file.timezone) {
        return Err("timezone must be an IANA time zone name".into());
    }
    for user in &file.notices_for {
        if user.trim().is_empty() || user.len() > 200 {
            return Err("notices_for holds user names".into());
        }
    }
    let mut defaults = PlanDefaults::default();
    if let Some(c) = file.checkins {
        if let Some(on) = c.on {
            defaults.on = on;
        }
        if let Some(every) = c.every_minutes {
            if !EVERY_MINUTES.contains(&every) {
                return Err(format!(
                    "checkins.every_minutes must be between {} and {}",
                    EVERY_MINUTES.start(),
                    EVERY_MINUTES.end()
                ));
            }
            defaults.every_minutes = every;
        }
        match (c.quiet_from, c.quiet_until) {
            (None, None) => {}
            (Some(from), Some(until)) => {
                let from = time::parse_hhmm(&from)
                    .ok_or("checkins.quiet_from must be a time like \"22:00\"")?;
                let until = time::parse_hhmm(&until)
                    .ok_or("checkins.quiet_until must be a time like \"08:00\"")?;
                defaults.quiet = (from != until).then_some((from, until));
            }
            _ => return Err("checkins.quiet_from and quiet_until go together".into()),
        }
    }
    Ok(Settings {
        timezone: file.timezone,
        notices_for: file.notices_for,
        defaults,
    })
}

fn ev(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.into())
}

fn registration_credential() -> String {
    std::env::var("GREENHOUSE_REGISTRATION_CREDENTIAL_FILE")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("CREDENTIALS_DIRECTORY")
                .ok()
                .map(|directory| format!("{directory}/registration.credential"))
        })
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

impl AppConfig {
    pub fn from_env() -> Result<Self, String> {
        let path = ev("GREENHOUSE_BODY_DOUBLE_CONFIG", DEFAULT_CONFIG_FILE);
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        let settings = parse_file_config(&text).map_err(|e| format!("{path}: {e}"))?;
        Zone::load(&settings.timezone).map_err(|e| format!("{path}: timezone: {e}"))?;
        let port = ev("GREENHOUSE_APP_PORT", DEFAULT_PORT)
            .parse()
            .map_err(|_| "GREENHOUSE_APP_PORT must be a port number".to_string())?;
        Ok(Self {
            kernel_url: ev("GREENHOUSE_KERNEL_URL", "http://127.0.0.1:4000"),
            state_dir: ev("GREENHOUSE_BODY_DOUBLE_STATE_DIR", DEFAULT_STATE_DIR).into(),
            port,
            service_token: registration_credential(),
            timezone: settings.timezone,
            notices_for: settings.notices_for,
            defaults: settings.defaults,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"{
      "version": "greenhouse-body-double-config-v1",
      "timezone": "America/New_York",
      "notices_for": ["owner"],
      "checkins": {"on": true, "every_minutes": 45, "quiet_from": "22:00", "quiet_until": "08:00"}
    }"#;

    #[test]
    fn the_example_configuration_is_read() {
        let s = parse_file_config(EXAMPLE).unwrap();
        assert_eq!(s.timezone, "America/New_York");
        assert_eq!(s.notices_for, vec!["owner"]);
        assert_eq!(s.defaults, PlanDefaults::default());
    }

    #[test]
    fn only_the_zone_is_required() {
        let s =
            parse_file_config(r#"{"version":"greenhouse-body-double-config-v1","timezone":"UTC"}"#)
                .unwrap();
        assert!(s.notices_for.is_empty());
        assert_eq!(s.defaults, PlanDefaults::default());
    }

    #[test]
    fn mistakes_are_refused() {
        for (bad, why) in [
            (EXAMPLE.replace("-v1", "-v2"), "version"),
            (EXAMPLE.replace("America/New_York", "../etc/passwd"), "zone"),
            (
                EXAMPLE.replace("\"every_minutes\": 45", "\"every_minutes\": 5"),
                "interval",
            ),
            (EXAMPLE.replace("\"22:00\"", "\"10pm\""), "clock time"),
            (
                EXAMPLE.replace(", \"quiet_until\": \"08:00\"", ""),
                "half a window",
            ),
            (
                EXAMPLE.replace("\"notices_for\"", "\"extra\": 1, \"notices_for\""),
                "unknown field",
            ),
            (EXAMPLE.replace("[\"owner\"]", "[\" \"]"), "blank user"),
        ] {
            assert!(parse_file_config(&bad).is_err(), "{why}");
        }
    }

    #[test]
    fn equal_quiet_bounds_mean_no_quiet_hours() {
        let s = parse_file_config(&EXAMPLE.replace("\"08:00\"", "\"22:00\"")).unwrap();
        assert_eq!(s.defaults.quiet, None);
    }
}
