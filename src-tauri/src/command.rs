use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use crate::protocol::Scene;

/// The lights a command targets by name. A command without one targets every enabled light.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Lights(Vec<String>);

impl Lights {
    pub fn includes(&self, name: &str) -> bool {
        self.0
            .iter()
            .any(|wanted| wanted.eq_ignore_ascii_case(name))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for Lights {
    fn from(name: String) -> Self {
        Self(vec![name])
    }
}

impl From<Vec<String>> for Lights {
    fn from(names: Vec<String>) -> Self {
        Self(names)
    }
}

impl fmt::Display for Lights {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.join(", "))
    }
}

// Older clients send one light name; the controller sends a list.
impl<'de> Deserialize<'de> for Lights {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOrMany {
            One(String),
            Many(Vec<String>),
        }
        Ok(match OneOrMany::deserialize(deserializer)? {
            OneOrMany::One(name) => Self(vec![name]),
            OneOrMany::Many(names) => Self(names),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ControlCommand {
    Color {
        value: String,
        brightness: Option<f32>,
        device: Option<Lights>,
    },
    White {
        value: String,
        #[serde(default)]
        kelvin: Option<u16>,
        brightness: Option<f32>,
        device: Option<Lights>,
    },
    Brightness {
        value: f32,
        device: Option<Lights>,
    },
    Power {
        on: bool,
        device: Option<Lights>,
    },
    Preset {
        name: String,
        device: Option<Lights>,
    },
    Party {
        device: Option<Lights>,
    },
    Breathe {
        #[serde(default)]
        interval_ms: Option<u32>,
        // Older clients may still send a full-cycle duration.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cycle_seconds: Option<u32>,
        // Legacy seconds format for the fixed interval.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pace_seconds: Option<f32>,
        #[serde(default = "crate::breathing::default_color_step")]
        color_step: u16,
        // Accept commands from older CLI installations. New clients send color_step.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hue_step_degrees: Option<f32>,
        device: Option<Lights>,
    },
    BreatheWhite {
        sweep_seconds: u32,
        device: Option<Lights>,
    },
    SetWhiteBreathingPace {
        sweep_seconds: u32,
    },
    TraceBreathing {
        seconds: u32,
        #[serde(default = "crate::timing::default_cache_characteristic")]
        cache_characteristic: bool,
    },
    SeekBreathing {
        position: u16,
        #[serde(default)]
        request_id: u64,
    },
    StopParty,
    StopEffect,
    PartyFrame {
        value: String,
        enter: bool,
        device: Option<Lights>,
    },
    BreathingFrame {
        value: String,
        device: Option<Lights>,
    },
    Experiment {
        payload: String,
        device: Option<Lights>,
    },
    // Built-in scenes; lights whose model lacks the scene are left unchanged.
    Scene {
        scene: Scene,
        device: Option<Lights>,
    },
    // H6005 only: the light breathes through these colors by itself.
    DiyBreathe {
        colors: Vec<String>,
        speed: u8,
        device: Option<Lights>,
    },
    // Finds the closest frame spacing one light reliably keeps (src/spacing.rs).
    MeasureStepInterval {
        device: Option<Lights>,
    },
}

impl ControlCommand {
    pub fn device(&self) -> Option<&Lights> {
        match self {
            Self::Color { device, .. }
            | Self::White { device, .. }
            | Self::Brightness { device, .. }
            | Self::Power { device, .. }
            | Self::Preset { device, .. }
            | Self::Party { device }
            | Self::Breathe { device, .. }
            | Self::BreatheWhite { device, .. }
            | Self::PartyFrame { device, .. }
            | Self::BreathingFrame { device, .. }
            | Self::Experiment { device, .. }
            | Self::Scene { device, .. }
            | Self::DiyBreathe { device, .. }
            | Self::MeasureStepInterval { device } => device.as_ref(),
            Self::SetWhiteBreathingPace { .. }
            | Self::TraceBreathing { .. }
            | Self::SeekBreathing { .. }
            | Self::StopParty
            | Self::StopEffect => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResponse {
    pub ok: bool,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_accept_one_name_or_a_list() {
        let one: ControlCommand =
            serde_json::from_str(r#"{"command":"power","on":true,"device":"Desk"}"#).unwrap();
        let many: ControlCommand =
            serde_json::from_str(r#"{"command":"power","on":true,"device":["Desk","shelf"]}"#)
                .unwrap();
        let all: ControlCommand =
            serde_json::from_str(r#"{"command":"power","on":true,"device":null}"#).unwrap();
        assert!(one.device().unwrap().includes("desk"));
        assert!(!one.device().unwrap().includes("Shelf"));
        assert!(many.device().unwrap().includes("Shelf"));
        assert!(all.device().is_none());
    }
}
