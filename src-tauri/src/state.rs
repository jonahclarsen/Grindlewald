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
use tokio::sync::{Mutex, watch};

use crate::{
    ble::{BleController, DiscoveredDevice},
    breathing::{
        COLOR_COUNT, ColorCycle, color_at_position, color_step_from_degrees, frame_interval,
        next_frame_deadline, resolve_interval_ms,
    },
    command::ControlCommand,
    privileged,
    settings::{self, FloodlightAction, LightMode, Schedule, Settings},
};

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    connected_count: Option<usize>,
    effect_active: bool,
    breathing: Option<BreathingPlayback>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BreathingPlayback {
    pub position: u16,
    pub generation: u64,
    pub request_id: u64,
    pub frame_id: u64,
}

#[derive(Debug, Clone, Copy)]
struct BreathingSeek {
    position: u16,
    generation: u64,
    request_id: u64,
}

#[derive(Clone)]
pub struct SharedState {
    controller: Arc<Mutex<BleController>>,
    settings_path: Arc<PathBuf>,
    activity_generation: Arc<AtomicU64>,
    party_generation: Arc<AtomicU64>,
    party_active: Arc<AtomicBool>,
    disconnect_signal: watch::Sender<()>,
    disconnecting: Arc<AtomicBool>,
    breathing_frames: watch::Sender<Option<BreathingPlayback>>,
    breathing_seek: watch::Sender<Option<BreathingSeek>>,
}

impl SharedState {
    pub fn new(settings_path: PathBuf) -> Self {
        Self {
            controller: Arc::new(Mutex::new(BleController::new())),
            settings_path: Arc::new(settings_path),
            activity_generation: Arc::new(AtomicU64::new(0)),
            party_generation: Arc::new(AtomicU64::new(0)),
            party_active: Arc::new(AtomicBool::new(false)),
            disconnect_signal: watch::channel(()).0,
            disconnecting: Arc::new(AtomicBool::new(false)),
            breathing_frames: watch::channel(None).0,
            breathing_seek: watch::channel(None).0,
        }
    }

    pub fn subscribe_breathing(&self) -> watch::Receiver<Option<BreathingPlayback>> {
        self.breathing_frames.subscribe()
    }

    fn current_breathing(&self) -> Option<BreathingPlayback> {
        self.breathing_frames.borrow().clone().filter(|frame| {
            self.party_active.load(Ordering::SeqCst)
                && frame.generation == self.party_generation.load(Ordering::SeqCst)
        })
    }

    fn publish_breathing(&self, position: u16, generation: u64, request_id: u64) {
        if self.party_active.load(Ordering::SeqCst)
            && self.party_generation.load(Ordering::SeqCst) == generation
        {
            let frame_id = self
                .breathing_frames
                .borrow()
                .as_ref()
                .map_or(1, |frame| frame.frame_id + 1);
            self.breathing_frames.send_replace(Some(BreathingPlayback {
                position,
                generation,
                request_id,
                frame_id,
            }));
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
        let cancelled = self.disconnect_signal.subscribe();
        if self.disconnecting.load(Ordering::SeqCst) {
            return Err("Disconnect in progress".into());
        }
        until_disconnect(cancelled, async {
            self.controller.lock().await.discover().await
        })
        .await
    }

    pub async fn connection_status(&self) -> Result<ConnectionStatus, String> {
        let connected_count = match self.controller.try_lock() {
            Ok(controller) => Some(controller.connected_count().await?),
            Err(_) => None,
        };
        Ok(ConnectionStatus {
            connected_count,
            effect_active: self.party_active.load(Ordering::SeqCst),
            breathing: self.current_breathing(),
        })
    }

    pub async fn disconnect(&self) -> Result<String, String> {
        if self.disconnecting.swap(true, Ordering::SeqCst) {
            return Err("Disconnect already in progress".into());
        }
        self.disconnect_signal.send_replace(());
        self.party_active.store(false, Ordering::SeqCst);
        self.breathing_frames.send_replace(None);
        self.party_generation.fetch_add(1, Ordering::SeqCst);
        self.activity_generation.fetch_add(1, Ordering::SeqCst);
        let result = self.controller.lock().await.disconnect_all().await;
        self.disconnecting.store(false, Ordering::SeqCst);
        result.map(|_| "Disconnected from lights".into())
    }

    pub async fn execute(&self, command: ControlCommand) -> Result<String, String> {
        let cancelled = self.disconnect_signal.subscribe();
        if self.disconnecting.load(Ordering::SeqCst) {
            return Err("Disconnect in progress".into());
        }
        until_disconnect(cancelled, self.execute_inner(command)).await
    }

    async fn execute_inner(&self, command: ControlCommand) -> Result<String, String> {
        if matches!(&command, ControlCommand::Experiment { device: None, .. }) {
            return Err("experimental commands must target one named light".into());
        }
        if let ControlCommand::SeekBreathing {
            position,
            request_id,
        } = &command
        {
            if *position >= COLOR_COUNT {
                return Err("breathing position must be between 0 and 1529".into());
            }
            let playback = self.current_breathing().ok_or("Breathing is not running")?;
            self.breathing_seek.send_replace(Some(BreathingSeek {
                position: *position,
                generation: playback.generation,
                request_id: *request_id,
            }));
            return Ok("Breathing".into());
        }
        if let ControlCommand::Party { device } = &command {
            return self.start_party(device.clone()).await;
        }
        if let ControlCommand::Breathe {
            interval_ms,
            cycle_seconds,
            pace_seconds,
            color_step,
            hue_step_degrees,
            device,
        } = &command
        {
            let step = hue_step_degrees
                .map(color_step_from_degrees)
                .transpose()?
                .unwrap_or(*color_step);
            let interval = resolve_interval_ms(*interval_ms, *pace_seconds, *cycle_seconds, step)?;
            return self.start_breathing(interval, step, device.clone()).await;
        }
        if matches!(
            command,
            ControlCommand::StopParty | ControlCommand::StopEffect
        ) {
            return self.stop_effect().await;
        }
        self.party_active.store(false, Ordering::SeqCst);
        self.breathing_frames.send_replace(None);
        self.party_generation.fetch_add(1, Ordering::SeqCst);
        self.activity_generation.fetch_add(1, Ordering::SeqCst);
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
                self.breathing_frames.send_replace(None);
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
            self.breathing_frames.send_replace(None);
            self.arm_idle_disconnect(settings.connection_hold_seconds);
            return Err(error);
        }

        let state = self.clone();
        let mut cancelled = self.disconnect_signal.subscribe();
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
                let mut controller = state.controller.lock().await;
                if state.party_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                let result = tokio::select! {
                    biased;
                    _ = cancelled.changed() => break,
                    result = controller.apply(&settings, &command) => result,
                };
                if result.is_err() {
                    state.party_generation.fetch_add(1, Ordering::SeqCst);
                    state.party_active.store(false, Ordering::SeqCst);
                    state.breathing_frames.send_replace(None);
                    state.arm_idle_disconnect(settings.connection_hold_seconds);
                    break;
                }
                color_index = (color_index + 1) % COLORS.len();
            }
        });
        Ok("Party mode started".into())
    }

    async fn start_breathing(
        &self,
        interval_ms: u32,
        color_step: u16,
        device: Option<String>,
    ) -> Result<String, String> {
        let settings = self.load_settings()?;
        let origin = fastrand::u16(0..COLOR_COUNT);
        let mut cycle = ColorCycle::new(origin, color_step)?;
        let interval = frame_interval(interval_ms)?;
        if self.party_active.swap(true, Ordering::SeqCst) {
            return Ok("An effect is already running".into());
        }
        let generation = self.party_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.activity_generation.fetch_add(1, Ordering::SeqCst);
        let mut seek = self.breathing_seek.subscribe();
        let mut request_id = 0;
        let result = self
            .controller
            .lock()
            .await
            .apply(
                &settings,
                &ControlCommand::BreathingFrame {
                    value: color_at_position(cycle.position),
                    device: device.clone(),
                },
            )
            .await;
        if let Err(error) = result {
            self.party_active.store(false, Ordering::SeqCst);
            self.breathing_frames.send_replace(None);
            self.arm_idle_disconnect(settings.connection_hold_seconds);
            return Err(error);
        }
        self.publish_breathing(cycle.position, generation, request_id);
        // Connection setup is not part of the first color interval.
        let mut deadline = tokio::time::Instant::now() + interval;
        let state = self.clone();
        let mut cancelled = self.disconnect_signal.subscribe();
        tauri::async_runtime::spawn(async move {
            loop {
                let moved = tokio::select! {
                    biased;
                    _ = cancelled.changed() => break,
                    _ = seek.changed() => true,
                    _ = tokio::time::sleep_until(deadline) => false,
                };
                if state.party_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                if !moved {
                    cycle.advance();
                }
                let mut controller = state.controller.lock().await;
                if state.party_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                // Coalesce pointer moves arriving during a BLE write or while waiting
                // for the controller. Seeking never stops the effect or restores a preset.
                if moved || seek.has_changed().unwrap_or(false) {
                    let latest = *seek.borrow_and_update();
                    if let Some(latest) = latest.filter(|seek| seek.generation == generation) {
                        cycle.position = latest.position;
                        request_id = latest.request_id;
                    }
                }
                let command = ControlCommand::BreathingFrame {
                    value: color_at_position(cycle.position),
                    device: device.clone(),
                };
                let started = tokio::time::Instant::now();
                let result = tokio::select! {
                    biased;
                    _ = cancelled.changed() => break,
                    result = controller.apply(&settings, &command) => result,
                };
                deadline = next_frame_deadline(started, tokio::time::Instant::now(), interval);
                if state.party_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                if result.is_err() {
                    state.party_generation.fetch_add(1, Ordering::SeqCst);
                    state.party_active.store(false, Ordering::SeqCst);
                    state.breathing_frames.send_replace(None);
                    state.arm_idle_disconnect(settings.connection_hold_seconds);
                    break;
                }
                state.publish_breathing(cycle.position, generation, request_id);
            }
        });
        Ok("Breathing".into())
    }

    async fn stop_effect(&self) -> Result<String, String> {
        self.party_active.store(false, Ordering::SeqCst);
        self.breathing_frames.send_replace(None);
        self.party_generation.fetch_add(1, Ordering::SeqCst);
        self.activity_generation.fetch_add(1, Ordering::SeqCst);
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
        eprintln!(
            "{} Grindlewald automation started: scheduled time {}, floodlight action {:?}",
            Local::now().to_rfc3339(),
            schedule.time,
            schedule.floodlights
        );
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
                FloodlightAction::OnHomeNetwork => {
                    let current = match crate::network::home_network_status().await {
                        Ok(status) => status.fingerprint,
                        Err(error) => {
                            eprintln!(
                                "{} Grindlewald floodlights skipped: home network lookup failed",
                                Local::now().to_rfc3339()
                            );
                            return Err(error);
                        }
                    };
                    if crate::network::matches_saved_network(
                        schedule.floodlight_network.as_deref(),
                        current.as_deref(),
                    ) {
                        eprintln!(
                            "{} Grindlewald floodlights allowed: saved home router matched",
                            Local::now().to_rfc3339()
                        );
                        crate::run_floodlights(true).await.map(Some)
                    } else {
                        eprintln!(
                            "{} Grindlewald floodlights skipped: saved home router not matched",
                            Local::now().to_rfc3339()
                        );
                        Ok(Some(
                            "Floodlights skipped: saved home network is not connected".into(),
                        ))
                    }
                }
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
        let mut cancelled = self.disconnect_signal.subscribe();
        let state = self.clone();
        tauri::async_runtime::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(hold_seconds);
            loop {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                tokio::time::sleep(remaining.min(Duration::from_secs(2))).await;
                let mut controller = state.controller.lock().await;
                if state.activity_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                if tokio::time::Instant::now() >= deadline {
                    let _ = controller.disconnect_all().await;
                    break;
                }
                tokio::select! {
                    biased;
                    _ = cancelled.changed() => break,
                    _ = controller.keep_alive() => {}
                }
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
                    for schedule in settings.schedules.into_iter().filter(|schedule| {
                        schedule.is_enabled_at(now.timestamp_millis()) && schedule.time == minute
                    }) {
                        let key = format!("{today}:{}", schedule.id);
                        if triggered.insert(key) {
                            let state = state.clone();
                            tauri::async_runtime::spawn(async move {
                                if let Err(error) = state.run_schedule(schedule).await {
                                    eprintln!(
                                        "{} Grindlewald schedule failed: {error}",
                                        Local::now().to_rfc3339()
                                    );
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

async fn until_disconnect<T>(
    mut cancelled: watch::Receiver<()>,
    operation: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::select! {
        biased;
        _ = cancelled.changed() => Err("Light command cancelled by disconnect".into()),
        result = operation => result,
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

#[cfg(test)]
mod tests {
    use super::{SharedState, until_disconnect};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::sync::{Mutex, oneshot, watch};

    #[tokio::test]
    async fn seeking_updates_the_running_effect_without_stopping_or_loading_settings() {
        use crate::command::ControlCommand;
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("missing-settings.json"));
        assert!(
            state
                .execute(ControlCommand::SeekBreathing {
                    position: 100,
                    request_id: 1
                })
                .await
                .is_err()
        );
        state.party_active.store(true, Ordering::SeqCst);
        state.party_generation.store(7, Ordering::SeqCst);
        state.publish_breathing(0, 7, 0);
        let mut seek = state.breathing_seek.subscribe();
        for (position, request_id) in [(100, 1), (765, 2), (1529, 3)] {
            state
                .execute(ControlCommand::SeekBreathing {
                    position,
                    request_id,
                })
                .await
                .unwrap();
        }
        seek.changed().await.unwrap();
        let latest = (*seek.borrow_and_update()).unwrap();
        assert_eq!(latest.position, 1529);
        assert_eq!(latest.request_id, 3);
        assert_eq!(latest.generation, 7);
        assert!(state.party_active.load(Ordering::SeqCst));
        assert_eq!(state.party_generation.load(Ordering::SeqCst), 7);
        assert!(!state.settings_path.exists());
        assert!(
            state
                .execute(ControlCommand::SeekBreathing {
                    position: 1530,
                    request_id: 4
                })
                .await
                .is_err()
        );
        state.party_generation.fetch_add(1, Ordering::SeqCst);
        assert!(state.current_breathing().is_none());
    }

    #[tokio::test]
    async fn disconnect_cancels_an_in_flight_operation_and_releases_its_lock() {
        let (signal, cancelled) = watch::channel(());
        let lock = Arc::new(Mutex::new(()));
        let operation_lock = lock.clone();
        let (started, ready) = oneshot::channel();
        let task = tokio::spawn(until_disconnect(cancelled, async move {
            let _guard = operation_lock.lock().await;
            started.send(()).unwrap();
            std::future::pending::<Result<(), String>>().await
        }));
        ready.await.unwrap();
        assert!(lock.try_lock().is_err());
        signal.send_replace(());
        assert!(task.await.unwrap().unwrap_err().contains("cancelled"));
        assert!(lock.try_lock().is_ok());
        // A later, explicitly requested command can connect again.
        assert_eq!(
            until_disconnect(signal.subscribe(), async { Ok(42) }).await,
            Ok(42)
        );
    }

    #[tokio::test]
    async fn disconnect_wins_over_a_queued_command() {
        let (signal, cancelled) = watch::channel(());
        let ran = AtomicBool::new(false);
        signal.send_replace(());
        let result = until_disconnect(cancelled, async {
            ran.store(true, Ordering::SeqCst);
            Ok(())
        })
        .await;
        assert!(result.is_err());
        assert!(!ran.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn disconnect_clears_effects_and_invalidates_hold_timers_without_settings() {
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("missing-settings.json"));
        state.party_active.store(true, Ordering::SeqCst);
        let mut cancelled = state.disconnect_signal.subscribe();
        assert!(state.disconnect().await.is_ok());
        cancelled.changed().await.unwrap();
        assert!(!state.party_active.load(Ordering::SeqCst));
        assert_eq!(state.party_generation.load(Ordering::SeqCst), 1);
        assert_eq!(state.activity_generation.load(Ordering::SeqCst), 1);
        assert_eq!(
            state.connection_status().await.unwrap().connected_count,
            Some(0)
        );
        assert!(!state.disconnecting.load(Ordering::SeqCst));
        assert!(!state.settings_path().exists());
    }
}
