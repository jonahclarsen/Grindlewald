use std::{collections::HashMap, time::Duration};

use btleplug::{
    api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType},
    platform::{Adapter, Manager, Peripheral},
};
use futures::future::join_all;
use uuid::Uuid;

use crate::{
    command::ControlCommand,
    protocol::{
        CONTROL_CHARACTERISTIC, brightness_frame, color_frame, parse_hex_color, power_frame,
        white_frame,
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
}

impl BleController {
    pub fn new() -> Self {
        Self {
            adapter: None,
            connections: HashMap::new(),
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
                    identifier: peripheral.id().to_string(),
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
                None => "no enabled lights are configured; add one in Settings".into(),
            });
        }

        self.ensure_connected(&selected).await?;
        let characteristic_uuid = Uuid::parse_str(CONTROL_CHARACTERISTIC)
            .map_err(|error| format!("invalid control UUID: {error}"))?;

        let writes = selected.iter().map(|device| {
            let peripheral = self.connections[&device.identifier].clone();
            let frames = frames_for(command, device.profile);
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
                Ok::<_, String>(device.name.clone())
            }
        });

        let results = join_all(writes).await;
        let mut changed = Vec::new();
        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok(name) => changed.push(name),
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
            let connected = if let Some(peripheral) = self.connections.get(&device.identifier) {
                peripheral.is_connected().await.unwrap_or(false)
            } else {
                false
            };
            if !connected {
                self.connections.remove(&device.identifier);
                missing.push(device.clone());
            }
        }
        if missing.is_empty() {
            return Ok(());
        }

        let adapter = self.adapter().await?;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|error| error.to_string())?;
        tokio::time::sleep(Duration::from_millis(1400)).await;
        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|error| error.to_string())?;
        let _ = adapter.stop_scan().await;

        let mut jobs = Vec::new();
        for device in missing {
            let peripheral = find_peripheral(&peripherals, &device)
                .await
                .ok_or_else(|| format!("could not find {} over Bluetooth", device.name))?;
            jobs.push(async move {
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
                Ok::<_, String>((device.identifier, peripheral))
            });
        }

        for result in join_all(jobs).await {
            let (identifier, peripheral) = result?;
            self.connections.insert(identifier, peripheral);
        }
        Ok(())
    }

    pub async fn disconnect_all(&mut self) {
        let connections = std::mem::take(&mut self.connections);
        join_all(connections.into_values().map(|peripheral| async move {
            if peripheral.is_connected().await.unwrap_or(false) {
                let _ = peripheral.disconnect().await;
            }
        }))
        .await;
    }
}

async fn find_peripheral(peripherals: &[Peripheral], device: &DeviceConfig) -> Option<Peripheral> {
    let wanted = normalize(&device.identifier);
    for peripheral in peripherals {
        if normalize(&peripheral.id().to_string()) == wanted {
            return Some(peripheral.clone());
        }
        if let Ok(Some(properties)) = peripheral.properties().await
            && properties
                .local_name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(&device.identifier))
        {
            return Some(peripheral.clone());
        }
    }
    None
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
            value, brightness, ..
        } => {
            let mut frames = vec![white_frame(profile, parse_hex_color(value)?)];
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
    }
}
