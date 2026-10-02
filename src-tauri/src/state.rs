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
    ble::{BLUETOOTH_OFF_MESSAGE, BleController, DiscoveredDevice},
    breathing::{
        COLOR_COUNT, ColorCycle, color_at_position, color_step_from_degrees, frame_interval,
        next_frame_deadline, resolve_interval_ms, validate_white_sweep_seconds, white_at_kelvin,
        white_kelvin_at_phase, white_kelvin_from_hex, white_phase,
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
    pub mode: LightMode,
    pub position: u16,
    pub generation: u64,
    pub request_id: u64,
    pub frame_id: u64,
}

// The picker state a preset leaves behind, mirrored into the UI sliders.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LightSelection {
    pub mode: LightMode,
    pub color: String,
    pub white: String,
    pub white_kelvin: Option<u16>,
    pub brightness: f32,
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
    manual_light_signal: watch::Sender<()>,
    disconnecting: Arc<AtomicBool>,
    breathing_frames: watch::Sender<Option<BreathingPlayback>>,
    breathing_seek: watch::Sender<Option<BreathingSeek>>,
    light_selection: watch::Sender<Option<LightSelection>>,
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
            manual_light_signal: watch::channel(()).0,
            disconnecting: Arc::new(AtomicBool::new(false)),
            breathing_frames: watch::channel(None).0,
            breathing_seek: watch::channel(None).0,
            light_selection: watch::channel(None).0,
        }
    }

    pub fn subscribe_breathing(&self) -> watch::Receiver<Option<BreathingPlayback>> {
        self.breathing_frames.subscribe()
    }

    pub fn subscribe_light_selection(&self) -> watch::Receiver<Option<LightSelection>> {
        self.light_selection.subscribe()
    }

    // Reload before saving so a slow Bluetooth write never clobbers newer edits.
    fn remember_preset(&self, command: &ControlCommand) -> Result<(), String> {
        let mut settings = self.load_settings()?;
        if let Some(selection) = select_preset(&mut settings, command) {
            self.save_settings(&settings)?;
            self.light_selection.send_replace(Some(selection));
        }
        Ok(())
    }

    fn current_breathing(&self) -> Option<BreathingPlayback> {
        self.breathing_frames.borrow().clone().filter(|frame| {
            self.party_active.load(Ordering::SeqCst)
                && frame.generation == self.party_generation.load(Ordering::SeqCst)
        })
    }

    #[cfg(test)]
    fn publish_breathing(&self, position: u16, generation: u64, request_id: u64) {
        self.publish_breathing_mode(position, generation, request_id, LightMode::Color);
    }

    fn publish_breathing_mode(
        &self,
        position: u16,
        generation: u64,
        request_id: u64,
        mode: LightMode,
    ) {
        if self.party_active.load(Ordering::SeqCst)
            && self.party_generation.load(Ordering::SeqCst) == generation
        {
            let frame_id = self
                .breathing_frames
                .borrow()
                .as_ref()
                .map_or(1, |frame| frame.frame_id + 1);
            crate::timing::record("publish", std::time::Instant::now(), None, true);
            self.breathing_frames.send_replace(Some(BreathingPlayback {
                mode,
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
        self.manual_light_signal.send_replace(());
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
        let manual = !matches!(command, ControlCommand::TraceBreathing { .. });
        if manual {
            self.manual_light_signal.send_replace(());
        }
        self.execute_command(command, manual).await
    }

    // Automation attempts do not cancel each other's pending retries.
    async fn execute_automatic(&self, command: ControlCommand) -> Result<String, String> {
        self.execute_command(command, false).await
    }

    async fn execute_command(
        &self,
        command: ControlCommand,
        manual: bool,
    ) -> Result<String, String> {
        let cancelled = self.disconnect_signal.subscribe();
        if self.disconnecting.load(Ordering::SeqCst) {
            return Err("Disconnect in progress".into());
        }
        // Subscribe before waiting for the lock, including cancelled-scan cleanup.
        until_disconnect(cancelled, async {
            if manual {
                self.controller.lock().await.cancel_pending_scan().await;
            }
            self.execute_inner(command).await
        })
        .await
    }

    async fn execute_inner(&self, command: ControlCommand) -> Result<String, String> {
        // Check before changing effects, reading settings, or remembering a preset.
        if self.controller.lock().await.bluetooth_powered_off().await? {
            return Ok(BLUETOOTH_OFF_MESSAGE.into());
        }
        if let ControlCommand::TraceBreathing {
            seconds,
            cache_characteristic,
        } = &command
        {
            if self.current_breathing().is_none() {
                return Err("Start breathing before capturing timing".into());
            }
            return crate::timing::capture(
                self.settings_path
                    .parent()
                    .ok_or("missing settings directory")?,
                *seconds,
                *cache_characteristic,
            )
            .await;
        }
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
        if let ControlCommand::BreatheWhite {
            sweep_seconds,
            device,
        } = &command
        {
            validate_white_sweep_seconds(*sweep_seconds)?;
            return self
                .start_breathing_mode(350, 1, device.clone(), LightMode::White, *sweep_seconds)
                .await;
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
        let preset = matches!(command, ControlCommand::Preset { .. });
        let command = resolve_preset(&settings, command)?;
        let result = self
            .controller
            .lock()
            .await
            .apply(&settings, &command)
            .await;
        if result.as_deref() == Ok(BLUETOOTH_OFF_MESSAGE) {
            return result;
        }
        self.arm_idle_disconnect(settings.connection_hold_seconds);
        if preset
            && result.is_ok()
            && let Err(error) = self.remember_preset(&command)
        {
            eprintln!("Grindlewald could not save the preset selection: {error}");
        }
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
        let result = self
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
            .await;
        if result.as_deref() == Ok(BLUETOOTH_OFF_MESSAGE) {
            self.party_active.store(false, Ordering::SeqCst);
            self.breathing_frames.send_replace(None);
            return result;
        }
        if let Err(error) = result {
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
                if result.is_err() || result.as_deref() == Ok(BLUETOOTH_OFF_MESSAGE) {
                    state.party_generation.fetch_add(1, Ordering::SeqCst);
                    state.party_active.store(false, Ordering::SeqCst);
                    state.breathing_frames.send_replace(None);
                    if result.is_err() {
                        state.arm_idle_disconnect(settings.connection_hold_seconds);
                    }
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
        self.start_breathing_mode(interval_ms, color_step, device, LightMode::Color, 30)
            .await
    }

    async fn start_breathing_mode(
        &self,
        interval_ms: u32,
        color_step: u16,
        device: Option<String>,
        mode: LightMode,
        sweep_seconds: u32,
    ) -> Result<String, String> {
        let settings = self.load_settings()?;
        let origin = if mode == LightMode::White {
            let kelvin = match settings.white_kelvin {
                Some(kelvin) => kelvin.clamp(2000, 9000),
                None => white_kelvin_from_hex(&settings.white)?,
            };
            ((f64::from(kelvin - 2000) / 7000.0) * f64::from(COLOR_COUNT / 2)).round() as u16
        } else {
            fastrand::u16(0..COLOR_COUNT)
        };
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
                &breathing_command(mode, cycle.position, device.clone()),
            )
            .await;
        if result.as_deref() == Ok(BLUETOOTH_OFF_MESSAGE) {
            self.party_active.store(false, Ordering::SeqCst);
            self.breathing_frames.send_replace(None);
            return result;
        }
        if let Err(error) = result {
            self.party_active.store(false, Ordering::SeqCst);
            self.breathing_frames.send_replace(None);
            self.arm_idle_disconnect(settings.connection_hold_seconds);
            return Err(error);
        }
        self.publish_breathing_mode(cycle.position, generation, request_id, mode);
        // Connection setup is not part of the first color interval.
        let mut deadline = tokio::time::Instant::now() + interval;
        let mut white_started = tokio::time::Instant::now();
        let mut white_origin = origin;
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
                    crate::timing::record("timer_lateness", deadline.into_std(), None, true);
                    if mode == LightMode::White {
                        cycle.position =
                            white_phase(white_origin, white_started.elapsed(), sweep_seconds);
                    } else {
                        cycle.advance();
                    }
                }
                let lock_started = std::time::Instant::now();
                let mut controller = state.controller.lock().await;
                crate::timing::record("controller_wait", lock_started, None, true);
                if state.party_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                // Coalesce pointer moves arriving during a BLE write or while waiting
                // for the controller. Seeking never stops the effect or restores a preset.
                if moved || seek.has_changed().unwrap_or(false) {
                    let latest = *seek.borrow_and_update();
                    if let Some(latest) = latest.filter(|seek| seek.generation == generation) {
                        cycle.position = latest.position;
                        white_origin = latest.position;
                        white_started = tokio::time::Instant::now();
                        request_id = latest.request_id;
                    }
                }
                let command = breathing_command(mode, cycle.position, device.clone());
                let started = tokio::time::Instant::now();
                let result = tokio::select! {
                    biased;
                    _ = cancelled.changed() => break,
                    result = controller.apply(&settings, &command) => result,
                };
                crate::timing::record("apply", started.into_std(), None, result.is_ok());
                deadline = next_frame_deadline(started, tokio::time::Instant::now(), interval);
                if state.party_generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                if result.is_err() || result.as_deref() == Ok(BLUETOOTH_OFF_MESSAGE) {
                    state.party_generation.fetch_add(1, Ordering::SeqCst);
                    state.party_active.store(false, Ordering::SeqCst);
                    state.breathing_frames.send_replace(None);
                    if result.is_err() {
                        state.arm_idle_disconnect(settings.connection_hold_seconds);
                    }
                    break;
                }
                state.publish_breathing_mode(cycle.position, generation, request_id, mode);
            }
        });
        Ok(if mode == LightMode::White {
            "White breathing"
        } else {
            "Breathing"
        }
        .into())
    }

    async fn stop_effect(&self) -> Result<String, String> {
        let effect_mode = self.current_breathing().map(|frame| frame.mode);
        self.party_active.store(false, Ordering::SeqCst);
        self.breathing_frames.send_replace(None);
        self.party_generation.fetch_add(1, Ordering::SeqCst);
        self.activity_generation.fetch_add(1, Ordering::SeqCst);
        let settings = self.load_settings()?;
        let command = if effect_mode.unwrap_or(settings.mode) == LightMode::White {
            ControlCommand::White {
                value: settings.white.clone(),
                kelvin: settings.white_kelvin,
                brightness: Some(settings.brightness),
                device: None,
            }
        } else {
            ControlCommand::Color {
                value: settings.color.clone(),
                brightness: Some(settings.brightness),
                device: None,
            }
        };
        let result = self
            .controller
            .lock()
            .await
            .apply(&settings, &command)
            .await;
        if result.as_deref() != Ok(BLUETOOTH_OFF_MESSAGE) {
            self.arm_idle_disconnect(settings.connection_hold_seconds);
        }
        result.map(|_| "Effect stopped".into())
    }

    pub async fn run_schedule_by_id(&self, id: &str) -> Result<String, String> {
        self.manual_light_signal.send_replace(());
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
        let state = self.clone();
        let light_schedule = schedule.clone();
        let mut manual_change = self.manual_light_signal.subscribe();
        let lights = async move {
            let started = tokio::time::Instant::now();
            let attempt = tokio::select! {
                biased;
                _ = manual_change.changed() => return Ok("Automation lights cancelled by manual control".into()),
                result = state.run_schedule_lights(&light_schedule) => result,
            };
            let (result, any_succeeded) = attempt;
            if result.is_err() && !any_succeeded {
                eprintln!(
                    "Grindlewald automation lights will retry every 15 minutes for six hours"
                );
                tauri::async_runtime::spawn(async move {
                    retry_automation_lights(started, manual_change, || async {
                        let (result, any_succeeded) =
                            state.run_schedule_lights(&light_schedule).await;
                        if let Err(error) = result {
                            eprintln!("Grindlewald automation light retry failed: {error}");
                        }
                        any_succeeded
                    })
                    .await;
                });
            }
            result
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

    // Try every target even when the first one is unavailable. Shell and floodlight
    // actions stay outside this method so background retries cannot repeat them.
    async fn run_schedule_lights(&self, schedule: &Schedule) -> (Result<String, String>, bool) {
        let targets = schedule.light_targets();
        if targets.is_empty() {
            return (Ok("Lights unchanged".into()), false);
        }
        let mut messages = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            match self
                .execute_automatic(ControlCommand::Preset {
                    name: schedule.preset.clone(),
                    device: target,
                })
                .await
            {
                Ok(message) => messages.push(message),
                Err(error) => errors.push(error),
            }
        }
        let any_succeeded = !messages.is_empty();
        let result = if errors.is_empty() {
            Ok(messages.join(", "))
        } else {
            Err(errors.join("; "))
        };
        (result, any_succeeded)
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

fn breathing_command(mode: LightMode, position: u16, device: Option<String>) -> ControlCommand {
    if mode == LightMode::White {
        let kelvin = white_kelvin_at_phase(position);
        ControlCommand::White {
            value: white_at_kelvin(kelvin),
            kelvin: Some(kelvin),
            brightness: None,
            device,
        }
    } else {
        ControlCommand::BreathingFrame {
            value: color_at_position(position),
            device,
        }
    }
}

const AUTOMATION_LIGHT_RETRY_INTERVAL: Duration = Duration::from_secs(15 * 60);
const AUTOMATION_LIGHT_RETRY_WINDOW: Duration = Duration::from_secs(6 * 60 * 60);

async fn retry_automation_lights<F, Fut>(
    started: tokio::time::Instant,
    mut cancelled: watch::Receiver<()>,
    mut attempt: F,
) where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = started + AUTOMATION_LIGHT_RETRY_WINDOW;
    let mut next = started + AUTOMATION_LIGHT_RETRY_INTERVAL;
    while next <= deadline {
        tokio::select! {
            biased;
            _ = cancelled.changed() => break,
            _ = tokio::time::sleep_until(next) => {},
        }
        // Do not replay expired automations after a long sleep or a slow attempt.
        if tokio::time::Instant::now() > deadline {
            break;
        }
        let succeeded = tokio::select! {
            biased;
            _ = cancelled.changed() => break,
            result = attempt() => result,
        };
        if succeeded {
            break;
        }
        next += AUTOMATION_LIGHT_RETRY_INTERVAL;
        while next < tokio::time::Instant::now() {
            next += AUTOMATION_LIGHT_RETRY_INTERVAL;
        }
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

fn select_preset(settings: &mut Settings, command: &ControlCommand) -> Option<LightSelection> {
    match command {
        ControlCommand::Color {
            value, brightness, ..
        } => {
            settings.mode = LightMode::Color;
            settings.color = value.clone();
            settings.brightness = brightness.unwrap_or(settings.brightness);
        }
        ControlCommand::White {
            value,
            kelvin,
            brightness,
            ..
        } => {
            settings.mode = LightMode::White;
            settings.white = value.clone();
            settings.white_kelvin = *kelvin;
            settings.brightness = brightness.unwrap_or(settings.brightness);
        }
        _ => return None,
    }
    Some(LightSelection {
        mode: settings.mode,
        color: settings.color.clone(),
        white: settings.white.clone(),
        white_kelvin: settings.white_kelvin,
        brightness: settings.brightness,
    })
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
    use super::{SharedState, resolve_preset, select_preset, until_disconnect};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::sync::{Mutex, oneshot, watch};

    use crate::{
        command::ControlCommand,
        settings::{LightMode, Settings},
    };

    #[tokio::test]
    async fn automations_can_run_shell_actions_without_touching_lights() {
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("missing-settings.json"));
        for preset in ["", "missing-preset"] {
            let schedule = serde_json::from_value(serde_json::json!({
                "id": "no-lights", "name": "No lights", "time": "06:00",
                "preset": preset, "allLights": false, "shellCommand": "exit 0"
            }))
            .unwrap();
            let result = state.run_schedule(schedule).await.unwrap();
            assert!(result.contains("Lights unchanged"));
            assert!(result.contains("Shell command completed"));
        }
        assert!(!state.settings_path().exists());
    }

    #[tokio::test]
    async fn disconnect_cancels_manual_controls_waiting_to_clean_up_a_retry_scan() {
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("settings.json"));
        let controller = state.controller.lock().await;
        let queued_state = state.clone();
        let queued = tokio::spawn(async move {
            queued_state
                .execute(ControlCommand::Power {
                    on: true,
                    device: None,
                })
                .await
        });
        tokio::task::yield_now().await;
        let disconnect_state = state.clone();
        let disconnect = tokio::spawn(async move { disconnect_state.disconnect().await });
        tokio::task::yield_now().await;
        drop(controller);
        assert!(queued.await.unwrap().unwrap_err().contains("cancelled"));
        disconnect.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn manual_light_control_cancels_all_pending_retries_even_with_bluetooth_off() {
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("settings.json"));
        state.controller.lock().await.test_power_state = btleplug::api::CentralState::PoweredOff;
        let count = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let mut jobs = Vec::new();
        for _ in 0..2 {
            let counter = count.clone();
            let cancelled = state.manual_light_signal.subscribe();
            jobs.push(tokio::spawn(super::retry_automation_lights(
                tokio::time::Instant::now(),
                cancelled,
                move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                    std::future::ready(false)
                },
            )));
        }
        state
            .execute(ControlCommand::Power {
                on: true,
                device: None,
            })
            .await
            .unwrap();
        for job in jobs {
            job.await.unwrap();
        }
        tokio::time::advance(super::AUTOMATION_LIGHT_RETRY_WINDOW).await;
        assert_eq!(count.load(Ordering::SeqCst), 0);
        // A later automation gets a fresh subscription and can retry normally.
        super::retry_automation_lights(
            tokio::time::Instant::now(),
            state.manual_light_signal.subscribe(),
            || {
                count.fetch_add(1, Ordering::SeqCst);
                std::future::ready(true)
            },
        )
        .await;
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn manual_control_cancels_an_in_flight_retry_and_releases_its_lock() {
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("settings.json"));
        state.controller.lock().await.test_power_state = btleplug::api::CentralState::PoweredOff;
        let lock = Arc::new(Mutex::new(()));
        let operation_lock = lock.clone();
        let (started, ready) = oneshot::channel();
        let mut started = Some(started);
        let cancelled = state.manual_light_signal.subscribe();
        let job = tokio::spawn(super::retry_automation_lights(
            tokio::time::Instant::now(),
            cancelled,
            move || {
                let lock = operation_lock.clone();
                let started = started.take().unwrap();
                async move {
                    let _guard = lock.lock().await;
                    started.send(()).unwrap();
                    std::future::pending::<bool>().await
                }
            },
        ));
        ready.await.unwrap();
        assert!(lock.try_lock().is_err());
        state
            .execute(ControlCommand::Preset {
                name: "daytime".into(),
                device: None,
            })
            .await
            .unwrap();
        job.await.unwrap();
        assert!(lock.try_lock().is_ok());
    }

    #[test]
    fn white_breathing_frames_use_dedicated_white_without_resetting_brightness() {
        for (position, kelvin) in [(0, 2000), (765, 9000), (1530, 2000)] {
            let command = super::breathing_command(LightMode::White, position, None);
            let ControlCommand::White {
                kelvin: actual,
                brightness,
                ..
            } = command
            else {
                panic!("expected dedicated white frame");
            };
            assert_eq!(actual, Some(kelvin));
            assert_eq!(brightness, None);
        }
    }

    #[tokio::test]
    async fn bluetooth_off_commands_do_not_change_settings_or_start_effects() {
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("missing-settings.json"));
        state.controller.lock().await.test_power_state = btleplug::api::CentralState::PoweredOff;
        for command in [
            ControlCommand::Preset {
                name: "daytime".into(),
                device: None,
            },
            ControlCommand::Power {
                on: true,
                device: None,
            },
            ControlCommand::Party { device: None },
            ControlCommand::BreatheWhite {
                sweep_seconds: 30,
                device: None,
            },
            ControlCommand::Breathe {
                interval_ms: Some(500),
                cycle_seconds: None,
                pace_seconds: None,
                color_step: 1,
                hue_step_degrees: None,
                device: None,
            },
        ] {
            assert_eq!(
                state.execute(command).await.unwrap(),
                super::BLUETOOTH_OFF_MESSAGE
            );
        }
        assert!(state.discover().await.unwrap().is_empty());
        assert!(!state.settings_path.exists());
        assert!(!state.party_active.load(Ordering::SeqCst));
        assert_eq!(state.party_generation.load(Ordering::SeqCst), 0);
        assert_eq!(state.activity_generation.load(Ordering::SeqCst), 0);
        assert!(state.current_breathing().is_none());
    }

    #[tokio::test]
    async fn effects_do_not_start_if_bluetooth_turns_off_during_setup() {
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("missing-settings.json"));
        state.controller.lock().await.test_power_state = btleplug::api::CentralState::PoweredOff;
        // Call setup directly to cover Bluetooth turning off after execute's check.
        assert_eq!(
            state.start_party(None).await.unwrap(),
            super::BLUETOOTH_OFF_MESSAGE
        );
        assert!(!state.party_active.load(Ordering::SeqCst));
        assert_eq!(
            state.start_breathing(500, 1, None).await.unwrap(),
            super::BLUETOOTH_OFF_MESSAGE
        );
        assert!(!state.party_active.load(Ordering::SeqCst));
        assert!(state.current_breathing().is_none());
        assert!(!state.settings_path.exists());
    }

    #[tokio::test]
    async fn bluetooth_off_automations_still_run_shell_without_light_retries() {
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("missing-settings.json"));
        state.controller.lock().await.test_power_state = btleplug::api::CentralState::PoweredOff;
        let marker = directory.path().join("shell-runs");
        let schedule = serde_json::from_value(serde_json::json!({
            "id": "bluetooth-off", "name": "Bluetooth off", "time": "06:00",
            "preset": "daytime", "allLights": true,
            "shellCommand": format!("printf x >> '{}'", marker.display())
        }))
        .unwrap();
        let result = state.run_schedule(schedule).await.unwrap();
        assert!(result.contains(super::BLUETOOTH_OFF_MESSAGE));
        assert!(result.contains("Shell command completed"));
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "x");
        assert_eq!(state.activity_generation.load(Ordering::SeqCst), 0);
        assert!(!state.settings_path.exists());
    }

    #[tokio::test]
    async fn shell_commands_run_once_even_when_light_actions_fail() {
        let directory = tempfile::tempdir().unwrap();
        let state = SharedState::new(directory.path().join("settings.json"));
        let marker = directory.path().join("shell-runs");
        let schedule = serde_json::from_value(serde_json::json!({
            "id": "unavailable", "name": "Unavailable lights", "time": "06:00",
            "preset": "daytime", "allLights": true,
            "shellCommand": format!("printf x >> '{}'", marker.display())
        }))
        .unwrap();
        assert!(
            state
                .run_schedule(schedule)
                .await
                .unwrap_err()
                .contains("No lights connected")
        );
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "x");
    }

    #[tokio::test(start_paused = true)]
    async fn light_retries_run_every_fifteen_minutes_for_six_hours() {
        let started = tokio::time::Instant::now();
        let mut attempts = 0;
        let (_cancel, cancelled) = watch::channel(());
        super::retry_automation_lights(started, cancelled, || {
            attempts += 1;
            assert_eq!(
                tokio::time::Instant::now() - started,
                super::AUTOMATION_LIGHT_RETRY_INTERVAL * attempts
            );
            std::future::ready(false)
        })
        .await;
        assert_eq!(attempts, 24);
        assert_eq!(
            tokio::time::Instant::now() - started,
            super::AUTOMATION_LIGHT_RETRY_WINDOW
        );
    }

    #[tokio::test(start_paused = true)]
    async fn slow_light_attempts_do_not_shift_the_retry_schedule() {
        let started = tokio::time::Instant::now();
        let mut attempts = 0;
        let (_cancel, cancelled) = watch::channel(());
        super::retry_automation_lights(started, cancelled, || {
            attempts += 1;
            assert_eq!(
                tokio::time::Instant::now() - started,
                super::AUTOMATION_LIGHT_RETRY_INTERVAL * attempts
            );
            async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                false
            }
        })
        .await;
        assert_eq!(attempts, 24);
    }

    #[tokio::test(start_paused = true)]
    async fn light_retries_stop_as_soon_as_a_target_succeeds() {
        let started = tokio::time::Instant::now();
        let mut attempts = 0;
        let (_cancel, cancelled) = watch::channel(());
        super::retry_automation_lights(started, cancelled, || {
            attempts += 1;
            std::future::ready(attempts == 2)
        })
        .await;
        assert_eq!(attempts, 2);
        assert_eq!(
            tokio::time::Instant::now() - started,
            super::AUTOMATION_LIGHT_RETRY_INTERVAL * 2
        );
    }

    #[tokio::test(start_paused = true)]
    async fn expired_light_retries_do_not_run_after_sleep() {
        let started = tokio::time::Instant::now();
        tokio::time::advance(
            super::AUTOMATION_LIGHT_RETRY_WINDOW + std::time::Duration::from_secs(1),
        )
        .await;
        let (_cancel, cancelled) = watch::channel(());
        super::retry_automation_lights(started, cancelled, || async {
            panic!("expired automation must not run");
        })
        .await;
    }

    #[test]
    fn presets_move_the_ui_selection_to_match_the_light() {
        let mut settings = Settings {
            mode: LightMode::Color,
            white_kelvin: Some(6000),
            ..Settings::default()
        };
        let command = resolve_preset(
            &settings,
            ControlCommand::Preset {
                name: "eveningtime".into(),
                device: None,
            },
        )
        .unwrap();
        let selection = select_preset(&mut settings, &command).unwrap();
        assert_eq!(settings.mode, LightMode::White);
        assert_eq!(settings.white, "#ff8912");
        // Presets carry no kelvin, so the light infers it from the swatch.
        assert_eq!(settings.white_kelvin, None);
        assert_eq!(settings.brightness, 0.35);
        assert_eq!(selection.white, settings.white);

        let command = resolve_preset(
            &settings,
            ControlCommand::Preset {
                name: "nighttime".into(),
                device: None,
            },
        )
        .unwrap();
        select_preset(&mut settings, &command).unwrap();
        assert_eq!(settings.mode, LightMode::Color);
        assert_eq!(settings.color, "#ff4500");
        assert_eq!(settings.white, "#ff8912");

        let power = ControlCommand::Power {
            on: false,
            device: None,
        };
        assert!(select_preset(&mut settings, &power).is_none());
    }

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
