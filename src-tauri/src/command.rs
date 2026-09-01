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
    StopParty,
    PartyFrame {
        value: String,
        enter: bool,
        device: Option<String>,
    },
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
            | Self::PartyFrame { device, .. } => device.as_deref(),
            Self::StopParty => None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommandResponse {
    pub ok: bool,
    pub message: String,
}
