use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ControlCommand {
    Color {
        value: String,
        brightness: Option<f32>,
        device: Option<String>,
    },
    White {
        value: String,
        #[serde(default)]
        kelvin: Option<u16>,
        brightness: Option<f32>,
        device: Option<String>,
    },
    Brightness {
        value: f32,
        device: Option<String>,
    },
    Power {
        on: bool,
        device: Option<String>,
    },
    Preset {
        name: String,
        device: Option<String>,
    },
    Party {
        device: Option<String>,
    },
    Breathe {
        pace_seconds: f32,
        #[serde(default = "default_breathing_hue_step_degrees")]
        hue_step_degrees: f32,
        device: Option<String>,
    },
    StopParty,
    StopEffect,
    PartyFrame {
        value: String,
        enter: bool,
        device: Option<String>,
    },
    BreathingFrame {
        value: String,
        device: Option<String>,
    },
    Experiment {
        payload: String,
        device: Option<String>,
    },
}

fn default_breathing_hue_step_degrees() -> f32 {
    12.0
}

impl ControlCommand {
    pub fn device(&self) -> Option<&str> {
        match self {
            Self::Color { device, .. }
            | Self::White { device, .. }
            | Self::Brightness { device, .. }
            | Self::Power { device, .. }
            | Self::Preset { device, .. }
            | Self::Party { device }
            | Self::Breathe { device, .. }
            | Self::PartyFrame { device, .. }
            | Self::BreathingFrame { device, .. }
            | Self::Experiment { device, .. } => device.as_deref(),
            Self::StopParty | Self::StopEffect => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResponse {
    pub ok: bool,
    pub message: String,
}
