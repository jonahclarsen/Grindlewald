use std::{collections::HashMap, time::Duration};

use btleplug::{
    api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter, WriteType},
    platform::{Adapter, Manager, Peripheral},
};
use futures::{Stream, StreamExt, future::join_all};
use uuid::Uuid;

use crate::{
    command::ControlCommand,
    protocol::{
        CONTROL_CHARACTERISTIC, brightness_frame, color_frame, experimental_mode_frame,
        keep_alive_frame, parse_hex_color, party_frames, power_frame, white_frame,
    },
    settings::{DeviceConfig, Settings},
};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredDevice {
    pub name: String,
    pub identifier: String,
}

pub struct BleController {
    adapter: Option<Adapter>,
    connections: HashMap<String, Peripheral>,
    light_states: HashMap<String, LightState>,
    last_breathing_frame_finished: Option<tokio::time::Instant>,
}

impl Default for BleController {
    fn default() -> Self {
        Self::new()
    }
}

impl BleController {
    pub fn new() -> Self {
        Self {
            adapter: None,
            connections: HashMap::new(),
            light_states: HashMap::new(),
            last_breathing_frame_finished: None,
        }
    }

    pub async fn discover(&mut self) -> Result<Vec<DiscoveredDevice>, String> {
        let adapter = self.adapter().await?;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|error| error.to_string())?;
        tokio::time::sleep(Duration::from_secs(3)).await;
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|error| error.to_string())?;
        let _ = adapter.stop_scan().await;

        let mut discovered = Vec::new();
        for peripheral in peripherals {
            let properties = peripheral.properties().await.ok().flatten();
            let name = properties
                .and_then(|properties| properties.local_name)
                .unwrap_or_else(|| "Bluetooth light".into());
            if name.to_ascii_lowercase().contains("govee")
                || name.to_ascii_lowercase().contains("ihoment")
                || name.to_ascii_lowercase().starts_with('h')
            {
                discovered.push(DiscoveredDevice {
                    name,
                    identifier: crate::settings::canonical_identifier(&peripheral.id().to_string()),
                });
            }
        }
        discovered.sort_by(|left, right| left.name.cmp(&right.name));
        discovered.dedup_by(|left, right| left.identifier == right.identifier);
        Ok(discovered)
    }

    async fn adapter(&mut self) -> Result<Adapter, String> {
        if let Some(adapter) = &self.adapter {
            return Ok(adapter.clone());
        }

        let manager = Manager::new().await.map_err(|error| error.to_string())?;
        let adapter = manager
            .adapters()
            .await
            .map_err(|error| error.to_string())?
            .into_iter()
            .next()
            .ok_or("no Bluetooth adapter is available")?;
        self.adapter = Some(adapter.clone());
        Ok(adapter)
    }

    pub async fn apply(
        &mut self,
        settings: &Settings,
        command: &ControlCommand,
    ) -> Result<String, String> {
        let selected: Vec<DeviceConfig> = settings
            .devices
            .iter()
            .filter(|device| device.enabled)
            .filter(|device| {
                command
                    .device()
                    .is_none_or(|wanted| device.name.eq_ignore_ascii_case(wanted))
            })
            .cloned()
            .collect();

        if selected.is_empty() {
            return Err(match command.device() {
                Some(name) => format!("no enabled device named {name:?}"),
                None => "No lights connected.".into(),
            });
        }

        self.ensure_connected(&selected).await?;
        let breathing = matches!(command, ControlCommand::BreathingFrame { .. });
        if breathing {
            if let Some(last_frame) = self.last_breathing_frame_finished {
                tokio::time::sleep_until(last_frame + crate::breathing::MIN_FRAME_INTERVAL).await;
            }
        }
        let characteristic_uuid = Uuid::parse_str(CONTROL_CHARACTERISTIC)
            .map_err(|error| format!("invalid control UUID: {error}"))?;

        let writes = selected.iter().map(|device| {
            let peripheral = self.connections[&normalize(&device.identifier)].clone();
            let key = normalize(&device.identifier);
            let mut next_state = self.light_states.get(&key).cloned().unwrap_or_default();
            let frames = next_state.frames(settings, command, device.profile);
            async move {
                let characteristic = peripheral
                    .characteristics()
                    .into_iter()
                    .find(|characteristic| characteristic.uuid == characteristic_uuid)
                    .ok_or_else(|| {
                        format!(
                            "{} does not expose the Govee control characteristic",
                            device.name
                        )
                    })?;

                for frame in frames? {
                    peripheral
                        .write(&characteristic, &frame, WriteType::WithoutResponse)
                        .await
                        .map_err(|error| format!("{}: {error}", device.name))?;
                }
                Ok::<_, String>((device.name.clone(), key, next_state))
            }
        });

        let results = join_all(writes).await;
        if breathing {
            self.last_breathing_frame_finished = Some(tokio::time::Instant::now());
        }
        let mut changed = Vec::new();
        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok((name, key, state)) => {
                    self.light_states.insert(key, state);
                    changed.push(name);
                }
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(format!("Updated {} light(s)", changed.len()))
        } else {
            Err(errors.join("; "))
        }
    }

    async fn ensure_connected(&mut self, devices: &[DeviceConfig]) -> Result<(), String> {
        let mut missing = Vec::new();
        for device in devices {
            let connection_key = normalize(&device.identifier);
            let connected = if let Some(peripheral) = self.connections.get(&connection_key) {
                peripheral.is_connected().await.unwrap_or(false)
            } else {
                false
            };
            if !connected {
                self.connections.remove(&connection_key);
                self.light_states
                    .entry(connection_key.clone())
                    .or_default()
                    .needs_sync = true;
                missing.push(device.clone());
            }
        }
        if missing.is_empty() {
            return Ok(());
        }

        let adapter = self.adapter().await?;
        // Subscribe before scanning so even the first advertisement is observed.
        // btleplug's macOS backend discards its native peripheral on disconnect;
        // a cached adapter.peripherals() entry alone is not safe to reconnect.
        let events = adapter.events().await.map_err(|error| error.to_string())?;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|error| error.to_string())?;
        let observations = events.filter_map(|event| async {
            let id = match event {
                CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => id,
                _ => return None,
            };
            let peripheral = match adapter.peripheral(&id).await {
                Ok(peripheral) => peripheral,
                Err(error) => return Some(Err(error.to_string())),
            };
            let name = peripheral
                .properties()
                .await
                .ok()
                .flatten()
                .and_then(|properties| properties.local_name);
            Some(Ok(ScanObservation {
                peripheral,
                identifier: id.to_string(),
                name,
            }))
        });
        let targets = scan_targets(&missing, observations, Duration::from_millis(1400)).await;
        // Stop on success, timeout, or error. Explicit cancellation stops scanning
        // through disconnect_all, including while scan_targets is awaiting an event.
        let stop_result = adapter.stop_scan().await.map_err(|error| error.to_string());
        let targets = targets?;
        stop_result?;
        // Track attempts before awaiting connect so cancellation can release every link.
        for (device, peripheral) in &targets {
            self.connections
                .insert(normalize(&device.identifier), peripheral.clone());
        }
        let jobs = targets.into_iter().map(|(device, peripheral)| async move {
            if !peripheral
                .is_connected()
                .await
                .map_err(|error| error.to_string())?
            {
                peripheral
                    .connect()
                    .await
                    .map_err(|error| format!("{}: {error}", device.name))?;
            }
            peripheral
                .discover_services()
                .await
                .map_err(|error| format!("{}: {error}", device.name))?;
            Ok::<_, String>(())
        });

        let errors: Vec<_> = join_all(jobs)
            .await
            .into_iter()
            .filter_map(Result::err)
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub async fn connected_count(&self) -> Result<usize, String> {
        let results = join_all(
            self.connections
                .values()
                .map(|peripheral| peripheral.is_connected()),
        )
        .await;
        let mut count = 0;
        for result in results {
            if result.map_err(|_| "Could not read Bluetooth connection status")? {
                count += 1;
            }
        }
        Ok(count)
    }

    pub async fn disconnect_all(&mut self) -> Result<(), String> {
        if let Some(adapter) = &self.adapter {
            let _ = adapter.stop_scan().await;
        }
        let results = join_all(self.connections.iter().map(
            |(identifier, peripheral)| async move {
                // Also cancel connections that are still being established.
                let result =
                    tokio::time::timeout(Duration::from_secs(3), peripheral.disconnect()).await;
                let released = matches!(result, Ok(Ok(())))
                    || matches!(peripheral.is_connected().await, Ok(false));
                (identifier.clone(), released)
            },
        ))
        .await;
        for (identifier, released) in results {
            if released {
                self.connections.remove(&identifier);
                self.light_states.entry(identifier).or_default().needs_sync = true;
            }
        }
        if self.connections.is_empty() {
            Ok(())
        } else {
            Err("Could not disconnect every light. Try Disconnect again.".into())
        }
    }

    pub async fn keep_alive(&mut self) {
        let Ok(characteristic_uuid) = Uuid::parse_str(CONTROL_CHARACTERISTIC) else {
            return;
        };
        let frame = keep_alive_frame();
        for peripheral in self.connections.values() {
            if !peripheral.is_connected().await.unwrap_or(false) {
                continue;
            }
            if let Some(characteristic) = peripheral
                .characteristics()
                .into_iter()
                .find(|characteristic| characteristic.uuid == characteristic_uuid)
            {
                let _ = peripheral
                    .write(&characteristic, &frame, WriteType::WithoutResponse)
                    .await;
            }
        }
    }
}

// Desired values survive disconnects; synchronization is scoped to each BLE link.
#[derive(Clone)]
struct LightState {
    color: Option<[u8; 20]>,
    brightness: Option<f32>,
    needs_sync: bool,
}

impl Default for LightState {
    fn default() -> Self {
        Self {
            color: None,
            brightness: None,
            needs_sync: true,
        }
    }
}

impl LightState {
    fn frames(
        &mut self,
        settings: &Settings,
        command: &ControlCommand,
        profile: crate::protocol::DeviceProfile,
    ) -> Result<Vec<[u8; 20]>, String> {
        let mut frames = frames_for(command, profile)?;
        match command {
            ControlCommand::Color { brightness, .. } | ControlCommand::White { brightness, .. } => {
                let desired = brightness
                    .or(self.brightness)
                    .unwrap_or(settings.brightness);
                frames.truncate(1);
                if self.needs_sync || brightness.is_some_and(|value| Some(value) != self.brightness)
                {
                    frames.push(brightness_frame(desired)?);
                }
                self.color = Some(frames[0]);
                self.brightness = Some(desired);
                self.needs_sync = false;
            }
            ControlCommand::Brightness { value, .. } => {
                if self.needs_sync {
                    let color = match self.color {
                        Some(color) => color,
                        None => color_frame(profile, parse_hex_color(&settings.color)?),
                    };
                    frames.insert(0, color);
                    self.color = Some(color);
                }
                self.brightness = Some(*value);
                self.needs_sync = false;
            }
            _ => {}
        }
        Ok(frames)
    }
}

struct ScanObservation<P> {
    peripheral: P,
    identifier: String,
    name: Option<String>,
}

// Only fresh scan events feed this collector, never stale cached peripherals.
async fn scan_targets<P: Clone>(
    devices: &[DeviceConfig],
    observations: impl Stream<Item = Result<ScanObservation<P>, String>>,
    scan_window: Duration,
) -> Result<Vec<(DeviceConfig, P)>, String> {
    futures::pin_mut!(observations);
    let deadline = tokio::time::Instant::now() + scan_window;
    let mut targets = Vec::new();
    let mut missing = devices.to_vec();
    while !missing.is_empty() {
        let observation = match tokio::time::timeout_at(deadline, observations.next()).await {
            Ok(Some(observation)) => observation?,
            Ok(None) | Err(_) => break,
        };
        missing.retain(|device| {
            let matches = normalize(&observation.identifier) == normalize(&device.identifier)
                || observation
                    .name
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(&device.identifier));
            if matches {
                targets.push((device.clone(), observation.peripheral.clone()));
            }
            !matches
        });
    }
    if missing.is_empty() {
        Ok(targets)
    } else {
        Err(format!(
            "could not find {} over Bluetooth",
            missing
                .iter()
                .map(|device| device.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn frames_for(
    command: &ControlCommand,
    profile: crate::protocol::DeviceProfile,
) -> Result<Vec<[u8; 20]>, String> {
    match command {
        ControlCommand::Color {
            value, brightness, ..
        } => {
            let mut frames = vec![color_frame(profile, parse_hex_color(value)?)];
            if let Some(brightness) = brightness {
                frames.push(brightness_frame(*brightness)?);
            }
            Ok(frames)
        }
        ControlCommand::White {
            value,
            kelvin,
            brightness,
            ..
        } => {
            let mut frames = vec![white_frame(profile, parse_hex_color(value)?, *kelvin)];
            if let Some(brightness) = brightness {
                frames.push(brightness_frame(*brightness)?);
            }
            Ok(frames)
        }
        ControlCommand::Brightness { value, .. } => Ok(vec![brightness_frame(*value)?]),
        ControlCommand::Power { on, .. } => Ok(vec![power_frame(*on)]),
        ControlCommand::Preset { .. } => {
            Err("preset commands must be resolved before reaching Bluetooth".into())
        }
        ControlCommand::Party { .. }
        | ControlCommand::Breathe { .. }
        | ControlCommand::StopParty
        | ControlCommand::StopEffect => {
            Err("effect commands must be resolved before reaching Bluetooth".into())
        }
        ControlCommand::PartyFrame { value, enter, .. } => {
            Ok(party_frames(profile, parse_hex_color(value)?, *enter))
        }
        ControlCommand::BreathingFrame { value, .. } => {
            Ok(vec![color_frame(profile, parse_hex_color(value)?)])
        }
        ControlCommand::Experiment { payload, .. } => Ok(vec![experimental_mode_frame(payload)?]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::DeviceProfile;

    fn scan_device(name: &str, identifier: &str) -> DeviceConfig {
        DeviceConfig {
            name: name.into(),
            identifier: identifier.into(),
            profile: DeviceProfile::H6005,
            enabled: true,
        }
    }

    fn observation(identifier: &str, name: Option<&str>) -> Result<ScanObservation<usize>, String> {
        Ok(ScanObservation {
            peripheral: 1,
            identifier: identifier.into(),
            name: name.map(str::to_owned),
        })
    }

    #[tokio::test]
    async fn reconnect_discovery_returns_immediately_after_requested_advertisement() {
        // A stream that stays open catches regressions that wait out the scan
        // window or wait for a second event after finding the light.
        let observations = futures::stream::iter([observation("aa11-bb22", None)])
            .chain(futures::stream::pending());
        let devices = [scan_device("Desk", "AA11-BB22")];
        let targets = tokio::time::timeout(
            Duration::from_millis(100),
            scan_targets(&devices, observations, Duration::from_millis(1400)),
        )
        .await
        .expect("discovery must not wait out the 1.4-second window")
        .unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].0, devices[0]);
    }

    #[tokio::test]
    async fn reconnect_discovery_waits_for_all_targets_and_ignores_unrelated_or_repeated_events() {
        let observations = futures::stream::iter([
            observation("other", None),
            observation("aa11", None),
            observation("AA11", None),
            observation("bb22", Some("named-light")),
        ])
        .chain(futures::stream::pending());
        let devices = [
            scan_device("Desk", "AA11"),
            scan_device("Shelf", "Named-Light"),
        ];
        let targets = tokio::time::timeout(
            Duration::from_millis(100),
            scan_targets(&devices, observations, Duration::from_millis(1400)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].0, devices[0]);
        assert_eq!(targets[1].0, devices[1]);
    }

    #[tokio::test]
    async fn reconnect_discovery_times_out_and_reports_only_missing_lights() {
        let observations =
            futures::stream::iter([observation("aa11", None)]).chain(futures::stream::pending());
        let error = tokio::time::timeout(
            Duration::from_millis(200),
            scan_targets(
                &[scan_device("Desk", "AA11"), scan_device("Shelf", "BB22")],
                observations,
                Duration::from_millis(20),
            ),
        )
        .await
        .expect("missing lights must not leave discovery running indefinitely")
        .unwrap_err();
        assert_eq!(error, "could not find Shelf over Bluetooth");
    }

    #[tokio::test]
    async fn reconnect_discovery_propagates_errors_and_handles_a_closed_stream() {
        let devices = [scan_device("Desk", "AA11")];
        let observations = futures::stream::iter([Err::<ScanObservation<usize>, _>(
            "Bluetooth unavailable".to_owned(),
        )]);
        assert_eq!(
            scan_targets(&devices, observations, Duration::from_millis(1400))
                .await
                .unwrap_err(),
            "Bluetooth unavailable"
        );
        assert_eq!(
            scan_targets::<usize>(
                &devices,
                futures::stream::empty(),
                Duration::from_millis(1400)
            )
            .await
            .unwrap_err(),
            "could not find Desk over Bluetooth"
        );
    }

    #[test]
    fn reconnect_syncs_both_once_for_color_and_white_on_each_profile() {
        for profile in [DeviceProfile::Classic, DeviceProfile::H6005] {
            for command in [
                ControlCommand::Color {
                    value: "#123456".into(),
                    brightness: Some(0.0),
                    device: None,
                },
                ControlCommand::White {
                    value: "#ffccaa".into(),
                    kelvin: Some(2700),
                    brightness: Some(0.0),
                    device: None,
                },
            ] {
                let settings = Settings::default();
                let mut state = LightState::default();
                let expected = frames_for(&command, profile).unwrap();
                assert_eq!(
                    state.frames(&settings, &command, profile).unwrap(),
                    expected
                );
                assert_eq!(
                    state.frames(&settings, &command, profile).unwrap(),
                    expected[..1]
                );
                state.needs_sync = true;
                let brightness = ControlCommand::Brightness {
                    value: 0.7,
                    device: None,
                };
                assert_eq!(
                    state.frames(&settings, &brightness, profile).unwrap(),
                    vec![expected[0], brightness_frame(0.7).unwrap()]
                );
                assert_eq!(
                    state.frames(&settings, &brightness, profile).unwrap(),
                    vec![brightness_frame(0.7).unwrap()]
                );
                // An explicitly changed brightness (including presets/queued edits) must still apply.
                assert_eq!(
                    state.frames(&settings, &command, profile).unwrap(),
                    expected
                );
                state.needs_sync = true;
                assert_eq!(
                    state.frames(&settings, &command, profile).unwrap(),
                    expected
                );
            }
        }
    }

    #[test]
    fn first_brightness_uses_saved_color_and_power_does_not_consume_sync() {
        let settings = Settings::default();
        let profile = DeviceProfile::Classic;
        let mut state = LightState::default();
        state
            .frames(
                &settings,
                &ControlCommand::Power {
                    on: true,
                    device: None,
                },
                profile,
            )
            .unwrap();
        let command = ControlCommand::Brightness {
            value: 0.2,
            device: None,
        };
        assert_eq!(
            state.frames(&settings, &command, profile).unwrap(),
            vec![
                color_frame(profile, parse_hex_color(&settings.color).unwrap()),
                brightness_frame(0.2).unwrap()
            ]
        );
        let mut other_light = LightState::default();
        assert_eq!(
            other_light
                .frames(&settings, &command, profile)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(state.frames(&settings, &command, profile).unwrap().len(), 1);
    }

    #[test]
    fn color_without_brightness_restores_last_brightness_after_reconnect() {
        let settings = Settings::default();
        let profile = DeviceProfile::Classic;
        let mut state = LightState::default();
        let command = ControlCommand::Color {
            value: "#abcdef".into(),
            brightness: None,
            device: None,
        };
        assert_eq!(
            state.frames(&settings, &command, profile).unwrap()[1],
            brightness_frame(settings.brightness).unwrap()
        );
        state
            .frames(
                &settings,
                &ControlCommand::Brightness {
                    value: 0.1,
                    device: None,
                },
                profile,
            )
            .unwrap();
        assert_eq!(state.frames(&settings, &command, profile).unwrap().len(), 1);
        state.needs_sync = true;
        assert_eq!(
            state.frames(&settings, &command, profile).unwrap()[1],
            brightness_frame(0.1).unwrap()
        );
    }

    #[test]
    fn bluetooth_identifiers_are_matched_case_insensitively() {
        assert_eq!(normalize("AA11-BB22-CC33"), normalize("aa11-bb22-cc33"));
    }
}
