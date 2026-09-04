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
    privileged,
    settings::{self, FloodlightAction, LightMode, Schedule, Settings},
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
        if !pace_seconds.is_finite() || !(0.1..=2.0).contains(&pace_seconds) {
            return Err("breathing pace must be between 0.1 and 2 seconds".into());
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
        let mut hue = random_hue();
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
        Ok("Breathing".into())
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

    pub fn approve_privileged_job(&self, id: &str) -> Result<String, String> {
        let mut settings = self.load_settings()?;
        let schedule_index = settings
            .schedules
            .iter()
            .position(|schedule| schedule.id == id)
            .ok_or_else(|| format!("schedule {id:?} was not found"))?;
        let command = settings.schedules[schedule_index]
            .shell_command
            .trim()
            .to_owned();
        let config_directory = self
            .settings_path
            .parent()
            .ok_or("settings path has no parent")?;
        privileged::approve_job(config_directory, id, &command)?;
        settings.schedules[schedule_index].run_as_administrator = true;
        settings.schedules[schedule_index].privileged_approved_command = command;
        settings.schedules[schedule_index].privileged_approved_at = Local::now().to_rfc3339();
        self.save_settings(&settings)?;
        Ok("Administrator command approved for unattended use".into())
    }

    pub fn revoke_privileged_job(&self, id: &str) -> Result<String, String> {
        let mut settings = self.load_settings()?;
        let schedule = settings
            .schedules
            .iter_mut()
            .find(|schedule| schedule.id == id)
            .ok_or_else(|| format!("schedule {id:?} was not found"))?;
        let message = privileged::revoke_job(id)?;
        schedule.run_as_administrator = false;
        schedule.privileged_approved_command.clear();
        schedule.privileged_approved_at.clear();
        self.save_settings(&settings)?;
        Ok(message)
    }

    pub fn clear_privileged_approvals(&self) -> Result<(), String> {
        let mut settings = self.load_settings()?;
        for schedule in &mut settings.schedules {
            schedule.run_as_administrator = false;
            schedule.privileged_approved_command.clear();
            schedule.privileged_approved_at.clear();
        }
        self.save_settings(&settings)
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

        let floodlights = async move {
            match schedule.floodlights {
                FloodlightAction::Unchanged => Ok(None),
                FloodlightAction::On => crate::run_floodlights(true).await.map(Some),
                FloodlightAction::Off => crate::run_floodlights(false).await.map(Some),
            }
        };

        let shell_command = schedule.shell_command.trim().to_owned();
        let run_as_administrator = schedule.run_as_administrator;
        let privileged_approved_command = schedule.privileged_approved_command.clone();
        let schedule_id = schedule.id.clone();
        let shell = async move {
            if shell_command.is_empty() {
                return Ok::<String, String>("No shell command".into());
            }
            if run_as_administrator {
                if privileged_approved_command != shell_command {
                    return Err("administrator command changed and must be approved again".into());
                }
                return privileged::run_job(&schedule_id).await;
            }
            let mut process = tokio::process::Command::new("/bin/zsh");
            process.args(["-lc", &shell_command]);
            let output = process
                .output()
                .await
                .map_err(|error| format!("could not start shell command: {error}"))?;
            if output.status.success() {
                Ok("Shell command completed".into())
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                Err(format!("shell command failed: {stderr}"))
            }
        };

        let (lights_result, floodlights_result, shell_result) =
            tokio::join!(lights, floodlights, shell);
        let mut messages = Vec::new();
        let mut errors = Vec::new();
        for result in [
            lights_result.map(Some),
            floodlights_result,
            shell_result.map(Some),
        ] {
            match result {
                Ok(Some(message)) => messages.push(message),
                Ok(None) => {}
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(format!("{}.", messages.join(". ")))
        } else {
            Err(errors.join("; "))
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

fn random_hue() -> f32 {
    fastrand::f32() * 360.0
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
    use super::{color_at_hue, random_hue};

    #[test]
    fn breathing_colors_move_continuously_around_the_hue_wheel() {
        assert_eq!(color_at_hue(0.0), "#ff0000");
        assert_eq!(color_at_hue(60.0), "#ffff00");
        assert_eq!(color_at_hue(120.0), "#00ff00");
        assert_eq!(color_at_hue(360.0), "#ff0000");
    }

    #[test]
    fn random_breathing_hues_stay_on_the_color_wheel() {
        for _ in 0..100 {
            assert!((0.0..360.0).contains(&random_hue()));
        }
    }
}
