use clap::{Parser, Subcommand, ValueEnum};
use grindlewald_lib::{
    breathing::{
        color_step_from_degrees, default_color_step, frame_interval, resolve_interval_ms,
        validate_white_sweep_seconds,
    },
    command::{CommandResponse, ControlCommand},
    ipc::socket_path,
    protocol::{MAX_DIY_COLORS, MAX_DIY_SPEED, Scene},
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn parse_hue_step(value: &str) -> Result<u16, String> {
    color_step_from_degrees(value.parse::<f32>().map_err(|error| error.to_string())?)
}

fn parse_interval(value: &str) -> Result<u32, String> {
    let ms = value.parse::<u32>().map_err(|error| error.to_string())?;
    frame_interval(ms)?;
    Ok(ms)
}

fn scene_name(scene: Scene) -> String {
    serde_json::to_value(scene)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn parse_scene(value: &str) -> Result<Scene, String> {
    Scene::ALL
        .into_iter()
        .find(|scene| scene_name(*scene).eq_ignore_ascii_case(value))
        .ok_or_else(|| {
            let names = Scene::ALL.map(scene_name).join(", ");
            format!("unknown scene {value:?}; choose one of {names}")
        })
}

fn parse_white_sweep(value: &str) -> Result<u32, String> {
    let seconds = value.parse::<u32>().map_err(|error| error.to_string())?;
    validate_white_sweep_seconds(seconds)?;
    Ok(seconds)
}

#[derive(Parser)]
#[command(
    name = "grindlewaldctl",
    about = "Control the running Grindlewald menu-bar app"
)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Clone, ValueEnum)]
enum PowerState {
    On,
    Off,
}

#[derive(Subcommand)]
enum CliCommand {
    /// Set an RGB color, such as #ff4500.
    Color {
        value: String,
        #[arg(short, long)]
        brightness: Option<f32>,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Set a dedicated-white color, such as #ffd5ad.
    White {
        value: String,
        /// Native color temperature for H6005 lights (2000-9000 K).
        #[arg(long, value_parser = clap::value_parser!(u16).range(2000..=9000))]
        kelvin: Option<u16>,
        #[arg(short, long)]
        brightness: Option<f32>,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Change only brightness (0.0 through 1.0).
    Brightness {
        value: f32,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Turn lights on or off.
    Power {
        state: PowerState,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Apply a preset by name.
    Preset {
        name: String,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Start a locally streamed rainbow party mode.
    Party {
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Start a slow color-breathing effect.
    Breathe {
        /// Milliseconds between color steps (the lights' measured minimum up to 1000; default 350).
        #[arg(long, conflicts_with_all = ["pace", "cycle_seconds"], value_parser = parse_interval)]
        interval_ms: Option<u32>,
        /// Legacy full-cycle duration, converted to the nearest supported step interval.
        #[arg(long, hide = true, value_parser = clap::value_parser!(u32).range(1..=3600))]
        cycle_seconds: Option<u32>,
        /// Seconds between steps, in whole milliseconds, within the same bounds.
        #[arg(long, conflicts_with = "cycle_seconds", value_parser = clap::value_parser!(f32))]
        pace: Option<f32>,
        /// RGB steps along the color wheel per update (1-100); 1 is the smallest change.
        #[arg(long, default_value_t = default_color_step(), value_parser = clap::value_parser!(u16).range(1..=100))]
        color_step: u16,
        /// Legacy degrees per update (0.1-120), rounded and clamped to 1-100 RGB steps.
        #[arg(long, conflicts_with = "color_step", value_parser = parse_hue_step)]
        hue_step: Option<u16>,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Sweep dedicated white LEDs between warm and cool, keeping brightness steady.
    /// Refused for H6005 lights, which whined during the sweep.
    BreatheWhite {
        /// Seconds for each one-way sweep (5-120, in steps of 5).
        #[arg(long, default_value_t = 30, value_parser = parse_white_sweep)]
        sweep_seconds: u32,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Capture timing from an already-running breathing effect without changing its pace.
    TraceBreathing {
        #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u32).range(5..=300))]
        seconds: u32,
        /// Compare the previous path that looks up the control characteristic for every write.
        #[arg(long)]
        uncached_characteristic: bool,
    },
    /// Stop party mode and restore the selected static color.
    StopParty,
    /// Stop any running party or breathing effect; breathing keeps its current color.
    StopEffect,
    /// Measure one light's minimum step interval and mode-switch spacing (about 20 minutes).
    MeasureStepInterval {
        /// Required when more than one light is enabled.
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Start a built-in scene on every light whose model offers it: breathe, reading,
    /// romantic, relax, vivid, colorful, morning, sunset (H6001), candlelight, fade,
    /// energetic, heartbeat, blinking (H6005).
    Scene {
        #[arg(value_parser = parse_scene)]
        scene: Scene,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// H6005 only: upload a breathing effect through 1-8 colors that the light runs by itself.
    DiyBreathe {
        #[arg(required = true, num_args = 1..=MAX_DIY_COLORS)]
        colors: Vec<String>,
        /// 0 (slowest) to 100.
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u8).range(0..=i64::from(MAX_DIY_SPEED)))]
        speed: u8,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Try raw hexadecimal bytes after the safe 33 05 color/mode prefix.
    Experiment {
        payload: String,
        #[arg(short, long)]
        light: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let command = match cli.command {
        CliCommand::Color {
            value,
            brightness,
            light,
        } => ControlCommand::Color {
            value,
            brightness,
            device: light,
        },
        CliCommand::White {
            value,
            kelvin,
            brightness,
            light,
        } => ControlCommand::White {
            value,
            kelvin,
            brightness,
            device: light,
        },
        CliCommand::Brightness { value, light } => ControlCommand::Brightness {
            value,
            device: light,
        },
        CliCommand::Power { state, light } => ControlCommand::Power {
            on: matches!(state, PowerState::On),
            device: light,
        },
        CliCommand::Preset { name, light } => ControlCommand::Preset {
            name,
            device: light,
        },
        CliCommand::Party { light } => ControlCommand::Party { device: light },
        CliCommand::Breathe {
            interval_ms,
            cycle_seconds,
            pace,
            color_step,
            hue_step,
            light,
        } => {
            let color_step = hue_step.unwrap_or(color_step);
            ControlCommand::Breathe {
                interval_ms: Some(
                    resolve_interval_ms(interval_ms, pace, cycle_seconds, color_step)
                        .map_err(anyhow::Error::msg)?,
                ),
                cycle_seconds: None,
                pace_seconds: None,
                color_step,
                hue_step_degrees: None,
                device: light,
            }
        }
        CliCommand::BreatheWhite {
            sweep_seconds,
            light,
        } => ControlCommand::BreatheWhite {
            sweep_seconds,
            device: light,
        },
        CliCommand::TraceBreathing {
            seconds,
            uncached_characteristic,
        } => ControlCommand::TraceBreathing {
            seconds,
            cache_characteristic: !uncached_characteristic,
        },
        CliCommand::StopParty => ControlCommand::StopParty,
        CliCommand::StopEffect => ControlCommand::StopEffect,
        CliCommand::Experiment { payload, light } => ControlCommand::Experiment {
            payload,
            device: Some(light),
        },
        CliCommand::Scene { scene, light } => ControlCommand::Scene {
            scene,
            device: light,
        },
        CliCommand::DiyBreathe {
            colors,
            speed,
            light,
        } => ControlCommand::DiyBreathe {
            colors,
            speed,
            device: light,
        },
        CliCommand::MeasureStepInterval { light } => {
            ControlCommand::MeasureStepInterval { device: light }
        }
    };

    let path = socket_path();
    let stream = tokio::net::UnixStream::connect(&path)
        .await
        .map_err(|error| {
            anyhow::anyhow!(
                "could not reach the Grindlewald menu-bar app at {}: {error}",
                path.display()
            )
        })?;
    let (reader, mut writer) = stream.into_split();
    writer
        .write_all(format!("{}\n", serde_json::to_string(&command)?).as_bytes())
        .await?;
    let mut response = String::new();
    BufReader::new(reader).read_line(&mut response).await?;
    let response: CommandResponse = serde_json::from_str(&response)?;
    if response.ok {
        println!("{}", response.message);
        Ok(())
    } else {
        Err(anyhow::anyhow!(response.message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grindlewald_lib::breathing::{MAX_INTERVAL_MS, MIN_INTERVAL_MS};

    #[test]
    fn scene_and_diy_cli_validate_their_arguments() {
        let cli = Cli::try_parse_from(["ctl", "scene", "Breathe"]).unwrap();
        assert!(matches!(
            cli.command,
            CliCommand::Scene {
                scene: Scene::Breathe,
                ..
            }
        ));
        assert!(Cli::try_parse_from(["ctl", "scene", "disco"]).is_err());
        let cli = Cli::try_parse_from(["ctl", "diy-breathe", "#ff4500"]).unwrap();
        assert!(matches!(
            cli.command,
            CliCommand::DiyBreathe { speed: 50, .. }
        ));
        assert!(Cli::try_parse_from(["ctl", "diy-breathe"]).is_err());
        assert!(Cli::try_parse_from(["ctl", "diy-breathe", "#ff4500", "--speed", "101"]).is_err());
        let nine = vec!["#000000"; MAX_DIY_COLORS + 1];
        assert!(Cli::try_parse_from([vec!["ctl", "diy-breathe"], nine].concat()).is_err());
    }

    #[test]
    fn white_breathing_cli_accepts_only_supported_sweep_durations() {
        let cli = Cli::try_parse_from(["ctl", "breathe-white"]).unwrap();
        assert!(matches!(
            cli.command,
            CliCommand::BreatheWhite {
                sweep_seconds: 30,
                ..
            }
        ));
        for invalid in ["0", "6", "125", "30.5"] {
            assert!(
                Cli::try_parse_from(["ctl", "breathe-white", "--sweep-seconds", invalid]).is_err()
            );
        }
    }

    #[test]
    fn interval_cli_accepts_only_supported_milliseconds() {
        for interval in [MIN_INTERVAL_MS, 251, 275, 350, MAX_INTERVAL_MS] {
            let cli =
                Cli::try_parse_from(["ctl", "breathe", "--interval-ms", &interval.to_string()])
                    .unwrap();
            let CliCommand::Breathe { interval_ms, .. } = cli.command else {
                panic!("expected breathe")
            };
            assert_eq!(interval_ms, Some(interval));
        }
        let too_fast = (MIN_INTERVAL_MS - 1).to_string();
        for invalid in ["0", &too_fast, "1001", "0.25", "-1"] {
            assert!(Cli::try_parse_from(["ctl", "breathe", "--interval-ms", invalid]).is_err());
        }
        for legacy in ["--pace", "--cycle-seconds"] {
            assert!(
                Cli::try_parse_from(["ctl", "breathe", "--interval-ms", "400", legacy, "1"])
                    .is_err()
            );
        }
    }

    #[test]
    fn breathing_cli_accepts_integer_steps_and_converts_legacy_degrees() {
        for (args, expected) in [
            (vec!["ctl", "breathe"], 1),
            (vec!["ctl", "breathe", "--color-step", "1"], 1),
            (vec!["ctl", "breathe", "--color-step", "100"], 100),
            (vec!["ctl", "breathe", "--hue-step", "2"], 9),
            (vec!["ctl", "breathe", "--hue-step", "0.1"], 1),
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            let CliCommand::Breathe {
                color_step,
                hue_step,
                ..
            } = cli.command
            else {
                panic!("expected breathe")
            };
            assert_eq!(hue_step.unwrap_or(color_step), expected);
        }
        for invalid in ["0", "101", "1.5", "-1"] {
            assert!(Cli::try_parse_from(["ctl", "breathe", "--color-step", invalid]).is_err());
        }
        assert!(
            Cli::try_parse_from(["ctl", "breathe", "--color-step", "1", "--hue-step", "2"])
                .is_err()
        );
    }
}
