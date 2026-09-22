use std::{
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{
    breathing::{
        COLOR_COUNT, MAX_COLOR_STEP, color_step_from_degrees, default_color_step,
        default_interval_ms, frame_interval, rounded_interval_ms, validate_color_step,
    },
    protocol::DeviceProfile,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceConfig {
    pub name: String,
    pub identifier: String,
    pub profile: DeviceProfile,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LightMode {
    Color,
    White,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub name: String,
    pub mode: LightMode,
    pub value: String,
    pub brightness: f32,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FloodlightAction {
    #[default]
    Unchanged,
    On,
    Off,
    #[serde(alias = "on_wifi")]
    OnHomeNetwork,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub name: String,
    /// Local wall-clock time in 24-hour HH:MM form.
    pub time: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// Temporary override of `enabled`, as Unix epoch milliseconds. Expiry needs no write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled_until: Option<i64>,
    #[serde(default)]
    pub lights: Vec<String>,
    pub preset: String,
    #[serde(default)]
    pub floodlights: FloodlightAction,
    #[serde(default)]
    pub floodlight_network: Option<String>,
    #[serde(default)]
    pub shell_command: String,
    #[serde(default)]
    pub run_as_administrator: bool,
    #[serde(default)]
    pub privileged_approved_command: String,
    #[serde(default)]
    pub privileged_approved_at: String,
}

impl Schedule {
    pub fn is_enabled_at(&self, now_millis: i64) -> bool {
        self.enabled && self.disabled_until.is_none_or(|until| now_millis >= until)
    }
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub devices: Vec<DeviceConfig>,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_white")]
    pub white: String,
    #[serde(default = "default_brightness")]
    pub brightness: f32,
    #[serde(default = "default_connection_hold_seconds")]
    pub connection_hold_seconds: u64,
    #[serde(default = "default_interval_ms")]
    pub breathing_interval_ms: u32,
    #[serde(default = "default_color_step")]
    pub breathing_color_step: u16,
    #[serde(default)]
    pub breathing_defaults_version: u8,
    #[serde(default)]
    pub presets: Vec<Preset>,
    #[serde(default)]
    pub schedules: Vec<Schedule>,
}

fn default_color() -> String {
    "#ff4f22".into()
}

fn default_white() -> String {
    "#ffd5ad".into()
}

fn default_brightness() -> f32 {
    0.4
}

fn default_connection_hold_seconds() -> u64 {
    6
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            color: default_color(),
            white: default_white(),
            brightness: default_brightness(),
            connection_hold_seconds: default_connection_hold_seconds(),
            breathing_interval_ms: default_interval_ms(),
            breathing_color_step: default_color_step(),
            breathing_defaults_version: 3,
            presets: vec![
                Preset {
                    name: "daytime".into(),
                    mode: LightMode::White,
                    value: "#d6e1ff".into(),
                    brightness: 1.0,
                },
                Preset {
                    name: "eveningtime".into(),
                    mode: LightMode::White,
                    value: "#ff8912".into(),
                    brightness: 0.35,
                },
                Preset {
                    name: "nighttime".into(),
                    mode: LightMode::Color,
                    value: "#ff4500".into(),
                    brightness: 0.35,
                },
                Preset {
                    name: "nighttimedark".into(),
                    mode: LightMode::Color,
                    value: "#ff4500".into(),
                    brightness: 0.03,
                },
                Preset {
                    name: "crashtime".into(),
                    mode: LightMode::Color,
                    value: "#ff4500".into(),
                    brightness: 0.0,
                },
            ],
            schedules: Vec::new(),
        }
    }
}

impl Settings {
    pub fn canonicalize_identifiers(&mut self) {
        let mut canonical_devices = Vec::new();
        let mut configured_names = HashMap::<String, String>::new();
        for mut device in std::mem::take(&mut self.devices) {
            device.identifier = canonical_identifier(&device.identifier);
            if let Some(configured_name) = configured_names.get(&device.identifier) {
                for schedule in &mut self.schedules {
                    for light in &mut schedule.lights {
                        if light == &device.name {
                            *light = configured_name.clone();
                        }
                    }
                    let mut seen = HashSet::new();
                    schedule.lights.retain(|light| seen.insert(light.clone()));
                }
            } else {
                configured_names.insert(device.identifier.clone(), device.name.clone());
                canonical_devices.push(device);
            }
        }
        self.devices = canonical_devices;
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.brightness) {
            return Err("brightness must be between 0 and 1".into());
        }
        if !(1..=60).contains(&self.connection_hold_seconds) {
            return Err("connection hold time must be between 1 and 60 seconds".into());
        }
        frame_interval(self.breathing_interval_ms)?;
        validate_color_step(self.breathing_color_step)?;
        crate::protocol::parse_hex_color(&self.color)?;
        crate::protocol::parse_hex_color(&self.white)?;
        let mut device_identifiers = HashSet::new();
        for device in &self.devices {
            if device.name.trim().is_empty() || device.identifier.trim().is_empty() {
                return Err("every device needs a name and Bluetooth identifier".into());
            }
            if !device_identifiers.insert(canonical_identifier(&device.identifier)) {
                return Err(format!(
                    "Bluetooth identifier for {:?} is already configured",
                    device.name
                ));
            }
        }
        for preset in &self.presets {
            if preset.name.trim().is_empty() {
                return Err("every preset needs a name".into());
            }
            crate::protocol::parse_hex_color(&preset.value)?;
            if !(0.0..=1.0).contains(&preset.brightness) {
                return Err(format!("preset {:?} has invalid brightness", preset.name));
            }
        }
        for schedule in &self.schedules {
            if schedule.id.trim().is_empty() || schedule.name.trim().is_empty() {
                return Err("every schedule needs an ID and name".into());
            }
            if schedule.id.len() > 64
                || !schedule
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err(format!("schedule {:?} has an invalid ID", schedule.name));
            }
            let valid_time = schedule.time.len() == 5
                && schedule.time.as_bytes()[2] == b':'
                && schedule.time[..2].parse::<u8>().is_ok_and(|hour| hour < 24)
                && schedule.time[3..]
                    .parse::<u8>()
                    .is_ok_and(|minute| minute < 60);
            if !valid_time {
                return Err(format!("schedule {:?} needs an HH:MM time", schedule.name));
            }
            if !self
                .presets
                .iter()
                .any(|preset| preset.name == schedule.preset)
            {
                return Err(format!(
                    "schedule {:?} references missing preset {:?}",
                    schedule.name, schedule.preset
                ));
            }
        }
        Ok(())
    }
}

pub fn canonical_identifier(value: &str) -> String {
    value.trim().to_ascii_uppercase()
}

pub fn load(path: &Path) -> Result<Settings, String> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut value: serde_json::Value =
        serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    if !value.is_object() {
        return Err("settings must be a JSON object".into());
    }
    if value.get("breathingColorStep").is_none() {
        if let Some(degrees) = value.get("breathingHueStepDegrees") {
            let mut degrees = degrees
                .as_f64()
                .ok_or("invalid legacy breathing hue step")? as f32;
            // Preserve the earlier migration of the original 12-degree default.
            if value
                .get("breathingDefaultsVersion")
                .and_then(|version| version.as_u64())
                .unwrap_or(0)
                == 0
                && degrees == 12.0
            {
                degrees = 2.0;
            }
            value["breathingColorStep"] = color_step_from_degrees(degrees)?.into();
        }
    }
    let version = value
        .get("breathingDefaultsVersion")
        .and_then(|version| version.as_u64())
        .unwrap_or(0);
    if version < 3 {
        let old_step = value
            .get("breathingColorStep")
            .map(|step| step.as_u64().ok_or("invalid breathing color step"))
            .transpose()?
            .unwrap_or(1);
        if !(1..=if version < 2 { 510 } else { 100 }).contains(&old_step) {
            return Err("invalid legacy breathing color step".into());
        }
        let color_step = if version < 2 && old_step == 9 {
            default_color_step()
        } else {
            old_step.min(u64::from(MAX_COLOR_STEP)) as u16
        };
        value["breathingColorStep"] = color_step.into();
        if value.get("breathingIntervalMs").is_none() {
            let milliseconds = if let Some(cycle) = value.get("breathingCycleSeconds") {
                cycle.as_f64().ok_or("invalid legacy breathing cycle")? * 1000.0
                    / f64::from(COLOR_COUNT.div_ceil(color_step))
            } else if let Some(pace) = value.get("breathingPaceSeconds") {
                let pace = pace.as_f64().ok_or("invalid legacy breathing pace")?;
                // Retain the original default migration from 2s to 750ms.
                if version == 0 && pace == 2.0 {
                    750.0
                } else {
                    pace * 1000.0
                }
            } else {
                f64::from(default_interval_ms())
            };
            value["breathingIntervalMs"] = rounded_interval_ms(milliseconds)?.into();
        }
        value["breathingDefaultsVersion"] = 3.into();
    }
    let mut settings: Settings =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    settings.canonicalize_identifiers();
    settings.validate()?;
    Ok(settings)
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    let mut settings = settings.clone();
    settings.canonicalize_identifiers();
    settings.validate()?;
    let parent = path.parent().ok_or("settings path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp_path = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temp_path).map_err(|error| error.to_string())?;
    let json = serde_json::to_vec_pretty(&settings).map_err(|error| error.to_string())?;
    file.write_all(&json).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    fs::rename(temp_path, path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_without_bundled_devices() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let settings = Settings::default();
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        assert!(!fs::read_to_string(path).unwrap().contains("identifier"));
    }

    #[test]
    fn legacy_values_migrate_to_fixed_intervals_and_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        for (legacy, step, interval) in [
            (serde_json::json!({}), 1, 400),
            (
                serde_json::json!({"breathingPaceSeconds":2.0,"breathingHueStepDegrees":12.0}),
                1,
                750,
            ),
            (
                serde_json::json!({"breathingPaceSeconds":0.75,"breathingColorStep":9,"breathingDefaultsVersion":1}),
                1,
                750,
            ),
            (
                serde_json::json!({"breathingPaceSeconds":0.1,"breathingColorStep":1,"breathingDefaultsVersion":1}),
                1,
                250,
            ),
            (
                serde_json::json!({"breathingPaceSeconds":1.25,"breathingColorStep":510,"breathingDefaultsVersion":1}),
                100,
                1000,
            ),
            (
                serde_json::json!({"breathingCycleSeconds":600,"breathingColorStep":1,"breathingDefaultsVersion":2}),
                1,
                400,
            ),
            (
                serde_json::json!({"breathingCycleSeconds":459,"breathingColorStep":1,"breathingDefaultsVersion":2}),
                1,
                300,
            ),
            (
                serde_json::json!({"breathingCycleSeconds":5,"breathingColorStep":100,"breathingDefaultsVersion":2}),
                100,
                300,
            ),
            (
                serde_json::json!({"breathingCycleSeconds":3600,"breathingColorStep":100,"breathingDefaultsVersion":2}),
                100,
                1000,
            ),
            (
                serde_json::json!({"breathingIntervalMs":250,"breathingColorStep":9,"breathingDefaultsVersion":3}),
                9,
                250,
            ),
        ] {
            fs::write(&path, legacy.to_string()).unwrap();
            let settings = load(&path).unwrap();
            assert_eq!(settings.breathing_color_step, step);
            assert_eq!(settings.breathing_interval_ms, interval);
            assert_eq!(settings.breathing_defaults_version, 3);
            save(&path, &settings).unwrap();
            assert_eq!(load(&path).unwrap(), settings);
            let saved = fs::read_to_string(&path).unwrap();
            for obsolete in [
                "breathingPaceSeconds",
                "breathingHueStepDegrees",
                "breathingCycleSeconds",
            ] {
                assert!(!saved.contains(obsolete));
            }
        }
    }

    #[test]
    fn fixed_intervals_validate_independently_of_color_step() {
        let mut settings = Settings::default();
        for step in 1..=100 {
            settings.breathing_color_step = step;
            for interval in (250..=1000).step_by(50) {
                settings.breathing_interval_ms = interval;
                assert!(settings.validate().is_ok());
            }
        }
        for interval in [0, 249, 251, 999, 1001] {
            settings.breathing_interval_ms = interval;
            assert!(settings.validate().is_err());
        }
        settings.breathing_interval_ms = 400;
        for step in [0, 101] {
            settings.breathing_color_step = step;
            assert!(settings.validate().is_err());
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        fs::write(
            &path,
            r#"{"breathingDefaultsVersion":3,"breathingIntervalMs":275}"#,
        )
        .unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn identifiers_are_stored_uppercase_and_compared_case_insensitively() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let mut settings = Settings::default();
        settings.devices.push(DeviceConfig {
            name: "Test lamp".into(),
            identifier: "aabb-ccdd".into(),
            profile: DeviceProfile::Classic,
            enabled: true,
        });
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap().devices[0].identifier, "AABB-CCDD");

        settings.devices.push(DeviceConfig {
            name: "Duplicate lamp".into(),
            identifier: "AABB-CCDD".into(),
            profile: DeviceProfile::Classic,
            enabled: true,
        });
        assert!(settings.validate().is_err());
    }

    #[test]
    fn loading_merges_case_only_duplicates_and_migrates_schedule_names() {
        let mut settings = Settings {
            devices: vec![
                DeviceConfig {
                    name: "Original lamp".into(),
                    identifier: "AABB-CCDD".into(),
                    profile: DeviceProfile::Classic,
                    enabled: true,
                },
                DeviceConfig {
                    name: "Re-added lamp".into(),
                    identifier: "aabb-ccdd".into(),
                    profile: DeviceProfile::Classic,
                    enabled: true,
                },
            ],
            schedules: vec![Schedule {
                id: "test".into(),
                name: "Test".into(),
                time: "12:00".into(),
                enabled: true,
                disabled_until: None,
                lights: vec!["Re-added lamp".into(), "Original lamp".into()],
                preset: "daytime".into(),
                floodlights: FloodlightAction::Unchanged,
                floodlight_network: None,
                shell_command: String::new(),
                run_as_administrator: false,
                privileged_approved_command: String::new(),
                privileged_approved_at: String::new(),
            }],
            ..Settings::default()
        };

        settings.canonicalize_identifiers();
        assert_eq!(settings.devices.len(), 1);
        assert_eq!(settings.devices[0].name, "Original lamp");
        assert_eq!(settings.devices[0].identifier, "AABB-CCDD");
        assert_eq!(settings.schedules[0].lights, ["Original lamp"]);
    }

    #[test]
    fn legacy_schedules_leave_floodlights_unchanged() {
        let schedule: Schedule = serde_json::from_value(serde_json::json!({
            "id": "legacy",
            "name": "Legacy automation",
            "time": "20:00",
            "preset": "daytime"
        }))
        .unwrap();

        assert_eq!(schedule.floodlights, FloodlightAction::Unchanged);
        assert_eq!(schedule.floodlight_network, None);
        assert_eq!(schedule.disabled_until, None);
        assert!(schedule.is_enabled_at(0));
    }

    #[test]
    fn old_ssid_selections_require_explicit_router_capture() {
        let schedule: Schedule = serde_json::from_value(serde_json::json!({
            "id": "legacy-wifi", "name": "Old Wi-Fi condition", "time": "20:00",
            "preset": "daytime", "floodlights": "on_wifi", "floodlightSsid": "Test Network"
        }))
        .unwrap();
        assert_eq!(schedule.floodlights, FloodlightAction::OnHomeNetwork);
        assert_eq!(schedule.floodlight_network, None);
        let saved = serde_json::to_value(schedule).unwrap();
        assert_eq!(saved["floodlights"], "on_home_network");
        assert!(saved.get("floodlightSsid").is_none());
    }

    #[test]
    fn home_network_selection_survives_settings_reload() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let schedule: Schedule = serde_json::from_value(serde_json::json!({
            "id": "wifi", "name": "Wi-Fi automation", "time": "20:00",
            "preset": "daytime", "floodlights": "on_home_network", "floodlightNetwork": "router-sha256:0000000000000000000000000000000000000000000000000000000000000000"
        }))
        .unwrap();
        let settings = Settings {
            schedules: vec![schedule],
            ..Settings::default()
        };
        save(&path, &settings).unwrap();
        let restored = load(&path).unwrap();
        assert_eq!(
            restored.schedules[0].floodlights,
            FloodlightAction::OnHomeNetwork
        );
        assert_eq!(
            restored.schedules[0].floodlight_network.as_deref(),
            Some("router-sha256:0000000000000000000000000000000000000000000000000000000000000000")
        );
    }

    #[test]
    fn schedule_pause_survives_reload_and_expires_at_its_deadline() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let deadline = 1_800_000_000_000_i64;
        let schedule: Schedule = serde_json::from_value(serde_json::json!({
            "id": "paused", "name": "Paused automation", "time": "20:00",
            "preset": "daytime", "enabled": true, "disabledUntil": deadline
        }))
        .unwrap();
        let settings = Settings {
            schedules: vec![schedule],
            ..Settings::default()
        };
        save(&path, &settings).unwrap();
        let mut restored = load(&path).unwrap().schedules.remove(0);
        assert_eq!(restored.disabled_until, Some(deadline));
        assert!(!restored.is_enabled_at(deadline - 1));
        assert!(restored.is_enabled_at(deadline));
        assert!(restored.is_enabled_at(deadline + 1));
        restored.enabled = false;
        assert!(!restored.is_enabled_at(deadline + 1));
        restored.disabled_until = None;
        assert!(!restored.is_enabled_at(deadline + 1));
    }
}
