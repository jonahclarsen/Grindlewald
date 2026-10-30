use serde::{Deserialize, Serialize};

use crate::protocol::Scene;

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
        device: Option<String>,
    },
    BreatheWhite {
        sweep_seconds: u32,
        device: Option<String>,
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
    // Built-in scenes; lights whose model lacks the scene are left unchanged.
    Scene {
        scene: Scene,
        device: Option<String>,
    },
    // H6005 only: the light breathes through these colors by itself.
    DiyBreathe {
        colors: Vec<String>,
        speed: u8,
        device: Option<String>,
    },
    // Finds the closest frame spacing one light reliably keeps (src/spacing.rs).
    MeasureStepInterval {
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
            | Self::Breathe { device, .. }
            | Self::BreatheWhite { device, .. }
            | Self::PartyFrame { device, .. }
            | Self::BreathingFrame { device, .. }
            | Self::Experiment { device, .. }
            | Self::Scene { device, .. }
            | Self::DiyBreathe { device, .. }
            | Self::MeasureStepInterval { device } => device.as_deref(),
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
