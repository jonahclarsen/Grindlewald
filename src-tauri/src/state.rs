use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use chrono::Local;
use tokio::sync::Mutex;

use crate::{
    ble::{BleController, DiscoveredDevice},
    command::ControlCommand,
    settings::{self, LightMode, Schedule, Settings},
};

#[derive(Clone)]
pub struct SharedState {
    controller: Arc<Mutex<BleController>>,
    settings_path: Arc<PathBuf>,
    activity_generation: Arc<AtomicU64>,
    party_generation: Arc<AtomicU64>,
    party_active: Arc<AtomicBool>,
}

impl SharedState {
    pub fn new(settings_path: PathBuf) -> Self {
        Self {
            controller: Arc::new(Mutex::new(BleController::new())),
            settings_path: Arc::new(settings_path),
            activity_generation: Arc::new(AtomicU64::new(0)),
            party_generation: Arc::new(AtomicU64::new(0)),
            party_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn settings_path(&self) -> &Path {
        &self.settings_path
    }

    pub fn load_settings(&self) -> Result<Settings, String> {
        settings::load(&self.settings_path)
    }

    pub fn save_settings(&self, value: &Settings) -> Result<(), String> {
        settings::save(&self.settings_path, value)
    }

    pub async fn discover(&self) -> Result<Vec<DiscoveredDevice>, String> {
        self.controller.lock().await.discover().await
    }

    pub async fn execute(&self, command: ControlCommand) -> Result<String, String> {
        if matches!(&command, ControlCommand::Experiment { device: None, .. }) {
            return Err("experimental commands must target one named light".into());
        }
        if let ControlCommand::Party { device } = &command {
            return self.start_party(device.clone()).await;
        }
        if let ControlCommand::Breathe {
            pace_seconds,
            hue_step_degrees,
            device,
        } = &command
        {
            return self
                .start_breathing(*pace_seconds, *hue_step_degrees, device.clone())
                .await;
        }
        if matches!(
            command,
            ControlCommand::StopParty | ControlCommand::StopEffect
        ) {
            return self.stop_effect().await;
        }
        self.party_active.store(false, Ordering::SeqCst);
        self.party_generation.fetch_add(1, Ordering::SeqCst);
        let settings = self.load_settings()?;
        let command = resolve_preset(&settings, command)?;
        let result = self
            .controller
            .lock()
            .await
            .apply(&settings, &command)
            .await;
        self.arm_idle_disconnect(settings.connection_hold_seconds);
        result
    }

    async fn start_party(&self, device: Option<String>) -> Result<String, String> {
        if self.party_active.swap(true, Ordering::SeqCst) {
            return Ok("Party mode is already running".into());
        }
        let generation = self.party_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.activity_generation.fetch_add(1, Ordering::SeqCst);
        let settings = match self.load_settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.party_active.store(false, Ordering::SeqCst);
                return Err(error);
            }
        };
        if let Err(error) = self
            .controller
            .lock()
            .await
            .apply(
                &settings,
                &ControlCommand::PartyFrame {
                    value: "#ff3040".into(),
                    enter: true,
                    device: device.clone(),
                },
            )
            .await
        {
            self.party_active.store(false, Ordering::SeqCst);
            return Err(error);
        }

        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            const COLORS: [&str; 12] = [
                "#ff3040", "#ff7a21", "#ffd52b", "#7cff31", "#24e6a8", "#23d7ff", "#2670ff",
                "#673cff", "#b72cff", "#ff2bba", "#ff315e", "#ff3040",
            ];
            let mut color_index = 1;
            loop {
                tokio::time::sleep(Duration::from_millis(180)).await;
                if state.party_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                let command = ControlCommand::PartyFrame {
                    value: COLORS[color_index].into(),
                    enter: false,
                    device: device.clone(),
                };
                if state
                    .controller
                    .lock()
                    .await
                    .apply(&settings, &command)
                    .await
                    .is_err()
                {
                    state.party_generation.fetch_add(1, Ordering::SeqCst);
                    state.party_active.store(false, Ordering::SeqCst);
                    break;
                }
                color_index = (color_index + 1) % COLORS.len();
            }
        });
        Ok("Party mode started".into())
    }

    async fn start_breathing(
        &self,
        pace_seconds: f32,
        hue_step_degrees: f32,
        device: Option<String>,
    ) -> Result<String, String> {
        if !pace_seconds.is_finite() || !(0.1..=15.0).contains(&pace_seconds) {
            return Err("breathing pace must be between 0.1 and 15 seconds".into());
        }
        if !hue_step_degrees.is_finite() || !(0.1..=120.0).contains(&hue_step_degrees) {
            return Err("breathing hue step must be between 0.1 and 120 degrees".into());
        }
        if self.party_active.swap(true, Ordering::SeqCst) {
            return Ok("An effect is already running".into());
        }
        let generation = self.party_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.activity_generation.fetch_add(1, Ordering::SeqCst);
        let settings = match self.load_settings() {
            Ok(settings) => settings,
            Err(error) => {
                self.party_active.store(false, Ordering::SeqCst);
                return Err(error);
            }
        };
        let mut hue = hue_from_rgb(crate::protocol::parse_hex_color(&settings.color)?);
        if let Err(error) = self
            .controller
            .lock()
            .await
            .apply(
                &settings,
                &ControlCommand::BreathingFrame {
                    value: color_at_hue(hue),
                    device: device.clone(),
                },
            )
            .await
        {
            self.party_active.store(false, Ordering::SeqCst);
            return Err(error);
        }

        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs_f32(pace_seconds)).await;
                if state.party_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                hue = (hue + hue_step_degrees) % 360.0;
                let command = ControlCommand::BreathingFrame {
                    value: color_at_hue(hue),
                    device: device.clone(),
                };
                if state
                    .controller
                    .lock()
                    .await
                    .apply(&settings, &command)
                    .await
                    .is_err()
                {
                    state.party_generation.fetch_add(1, Ordering::SeqCst);
                    state.party_active.store(false, Ordering::SeqCst);
                    break;
                }
            }
        });
        Ok(format!(
            "Breathing every {pace_seconds:.2} seconds with {hue_step_degrees:.1}° hue steps"
        ))
    }

    async fn stop_effect(&self) -> Result<String, String> {
        self.party_active.store(false, Ordering::SeqCst);
        self.party_generation.fetch_add(1, Ordering::SeqCst);
        let settings = self.load_settings()?;
        let command = ControlCommand::Color {
            value: settings.color.clone(),
            brightness: Some(settings.brightness),
            device: None,
        };
        let result = self
            .controller
            .lock()
            .await
            .apply(&settings, &command)
            .await;
        self.arm_idle_disconnect(settings.connection_hold_seconds);
        result.map(|_| "Effect stopped".into())
    }

    pub async fn run_schedule_by_id(&self, id: &str) -> Result<String, String> {
        let settings = self.load_settings()?;
        let schedule = settings
            .schedules
            .iter()
            .find(|schedule| schedule.id == id)
            .cloned()
            .ok_or_else(|| format!("schedule {id:?} was not found"))?;
        self.run_schedule(schedule).await
    }

    async fn run_schedule(&self, schedule: Schedule) -> Result<String, String> {
        let targets = if schedule.lights.is_empty() {
            vec![None]
        } else {
            schedule.lights.iter().cloned().map(Some).collect()
        };
        let preset_name = schedule.preset.clone();
        let state = self.clone();
        let lights = async move {
            let mut messages = Vec::new();
            for target in targets {
                messages.push(
                    state
                        .execute(ControlCommand::Preset {
                            name: preset_name.clone(),
                            device: target,
                        })
                        .await?,
                );
            }
            Ok::<_, String>(messages.join(", "))
        };

        let shell_command = schedule.shell_command.trim().to_owned();
        let run_as_administrator = schedule.run_as_administrator;
        let shell = async move {
            if shell_command.is_empty() {
                return Ok::<String, String>("No shell command".into());
            }
            let (mut process, success_message) = if run_as_administrator {
                let mut process = tokio::process::Command::new("/usr/bin/osascript");
                process.args([
                    "-e",
                    "on run argv",
                    "-e",
                    "do shell script (\"/bin/zsh -lc \" & quoted form of (item 1 of argv)) with administrator privileges",
                    "-e",
                    "end run",
                    "--",
                    &shell_command,
                ]);
                (process, "Administrator command completed")
            } else {
                let mut process = tokio::process::Command::new("/bin/zsh");
                process.args(["-lc", &shell_command]);
                (process, "Shell command completed")
            };
            let output = process
                .output()
                .await
                .map_err(|error| format!("could not start shell command: {error}"))?;
            if output.status.success() {
                Ok(success_message.into())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                Err(format!("shell command failed: {stderr}"))
            }
        };

        let (lights_result, shell_result) = tokio::join!(lights, shell);
        match (lights_result, shell_result) {
            (Ok(lights), Ok(shell)) => Ok(format!("{lights}. {shell}.")),
            (Err(lights), Ok(_)) => Err(lights),
            (Ok(_), Err(shell)) => Err(shell),
            (Err(lights), Err(shell)) => Err(format!("{lights}; {shell}")),
        }
    }

    fn arm_idle_disconnect(&self, hold_seconds: u64) {
        let generation = self.activity_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(hold_seconds);
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                tokio::time::sleep(remaining.min(Duration::from_secs(2))).await;
                if state.activity_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    state.controller.lock().await.disconnect_all().await;
                    break;
                }
                state.controller.lock().await.keep_alive().await;
            }
        });
    }

    pub fn start_scheduler(&self) {
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut triggered = HashSet::<String>::new();
            loop {
                let now = Local::now();
                let today = now.format("%Y-%m-%d").to_string();
                let minute = now.format("%H:%M").to_string();
                triggered.retain(|key| key.starts_with(&today));

                if let Ok(settings) = state.load_settings() {
                    for schedule in settings
                        .schedules
                        .into_iter()
                        .filter(|schedule| schedule.enabled && schedule.time == minute)
                    {
                        let key = format!("{today}:{}", schedule.id);
                        if triggered.insert(key) {
                            let state = state.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(error) = state.run_schedule(schedule).await {
                                    eprintln!("Grindlewald schedule failed: {error}");
                                }
                            });
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });
    }
}

fn resolve_preset(settings: &Settings, command: ControlCommand) -> Result<ControlCommand, String> {
    let ControlCommand::Preset { name, device } = command else {
        return Ok(command);
    };
    let preset = settings
        .presets
        .iter()
        .find(|preset| preset.name.eq_ignore_ascii_case(&name))
        .ok_or_else(|| format!("preset {name:?} was not found"))?;
    Ok(match preset.mode {
        LightMode::Color => ControlCommand::Color {
            value: preset.value.clone(),
            brightness: Some(preset.brightness),
            device,
        },
        LightMode::White => ControlCommand::White {
            value: preset.value.clone(),
            kelvin: None,
            brightness: Some(preset.brightness),
            device,
        },
    })
}

fn hue_from_rgb([red, green, blue]: [u8; 3]) -> f32 {
    let red = f32::from(red) / 255.0;
    let green = f32::from(green) / 255.0;
    let blue = f32::from(blue) / 255.0;
    let maximum = red.max(green).max(blue);
    let minimum = red.min(green).min(blue);
    let difference = maximum - minimum;
    if difference == 0.0 {
        return 0.0;
    }
    let hue = if maximum == red {
        ((green - blue) / difference) % 6.0
    } else if maximum == green {
        (blue - red) / difference + 2.0
    } else {
        (red - green) / difference + 4.0
    };
    (hue * 60.0 + 360.0) % 360.0
}

fn color_at_hue(hue: f32) -> String {
    let sector = hue.rem_euclid(360.0) / 60.0;
    let intermediate = (255.0 * (1.0 - ((sector % 2.0) - 1.0).abs())).round() as u8;
    let [red, green, blue] = match sector.floor() as u8 {
        0 => [255, intermediate, 0],
        1 => [intermediate, 255, 0],
        2 => [0, 255, intermediate],
        3 => [0, intermediate, 255],
        4 => [intermediate, 0, 255],
        _ => [255, 0, intermediate],
    };
    format!("#{red:02x}{green:02x}{blue:02x}")
}

#[cfg(test)]
mod tests {
    use super::{color_at_hue, hue_from_rgb};

    #[test]
    fn breathing_colors_move_continuously_around_the_hue_wheel() {
        assert_eq!(color_at_hue(0.0), "#ff0000");
        assert_eq!(color_at_hue(60.0), "#ffff00");
        assert_eq!(color_at_hue(120.0), "#00ff00");
        assert_eq!(color_at_hue(360.0), "#ff0000");
        assert!((hue_from_rgb([255, 79, 34]) - 12.2).abs() < 0.1);
    }
}
