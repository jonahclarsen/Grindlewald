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
        if let ControlCommand::Party { device } = &command {
            return self.start_party(device.clone()).await;
        }
        if matches!(command, ControlCommand::StopParty) {
            return self.stop_party().await;
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
        let settings = self.load_settings()?;
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

    async fn stop_party(&self) -> Result<String, String> {
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
        result.map(|_| "Party mode stopped".into())
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
        let shell = async move {
            if shell_command.is_empty() {
                return Ok::<String, String>("No shell command".into());
            }
            let output = tokio::process::Command::new("/bin/zsh")
                .args(["-lc", &shell_command])
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
