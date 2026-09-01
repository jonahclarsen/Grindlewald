use std::{fs, io::Write, path::Path};

use serde::{Deserialize, Serialize};

use crate::protocol::DeviceProfile;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: String,
    pub name: String,
    /// Local wall-clock time in 24-hour HH:MM form.
    pub time: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub lights: Vec<String>,
    pub preset: String,
    #[serde(default)]
    pub shell_command: String,
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            color: default_color(),
            white: default_white(),
            brightness: default_brightness(),
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
    pub fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.brightness) {
            return Err("brightness must be between 0 and 1".into());
        }
        crate::protocol::parse_hex_color(&self.color)?;
        crate::protocol::parse_hex_color(&self.white)?;
        for device in &self.devices {
            if device.name.trim().is_empty() || device.identifier.trim().is_empty() {
                return Err("every device needs a name and Bluetooth identifier".into());
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

pub fn load(path: &Path) -> Result<Settings, String> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let settings: Settings = serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    settings.validate()?;
    Ok(settings)
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    settings.validate()?;
    let parent = path.parent().ok_or("settings path has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temp_path = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temp_path).map_err(|error| error.to_string())?;
    let json = serde_json::to_vec_pretty(settings).map_err(|error| error.to_string())?;
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
}
