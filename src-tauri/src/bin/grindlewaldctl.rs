use clap::{Parser, Subcommand, ValueEnum};
use grindlewald_lib::{
    breathing::{color_step_from_degrees, default_color_step},
    command::{CommandResponse, ControlCommand},
    ipc::socket_path,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn parse_hue_step(value: &str) -> Result<u16, String> {
    color_step_from_degrees(value.parse::<f32>().map_err(|error| error.to_string())?)
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
        #[arg(long, default_value_t = 0.75, value_parser = clap::value_parser!(f32))]
        pace: f32,
        /// RGB steps along the color wheel per update (1-510); 1 is the smallest change.
        #[arg(long, default_value_t = default_color_step(), value_parser = clap::value_parser!(u16).range(1..=510))]
        color_step: u16,
        /// Legacy degrees per update (0.1-120), rounded to the nearest RGB step.
        #[arg(long, conflicts_with = "color_step", value_parser = parse_hue_step)]
        hue_step: Option<u16>,
        #[arg(short, long)]
        light: Option<String>,
    },
    /// Stop party mode and restore the selected static color.
    StopParty,
    /// Stop any running party or breathing effect.
    StopEffect,
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
            pace,
            color_step,
            hue_step,
            light,
        } => ControlCommand::Breathe {
            pace_seconds: pace,
            color_step: hue_step.unwrap_or(color_step),
            hue_step_degrees: None,
            device: light,
        },
        CliCommand::StopParty => ControlCommand::StopParty,
        CliCommand::StopEffect => ControlCommand::StopEffect,
        CliCommand::Experiment { payload, light } => ControlCommand::Experiment {
            payload,
            device: Some(light),
        },
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

    #[test]
    fn breathing_cli_accepts_integer_steps_and_converts_legacy_degrees() {
        for (args, expected) in [
            (vec!["ctl", "breathe"], 9),
            (vec!["ctl", "breathe", "--color-step", "1"], 1),
            (vec!["ctl", "breathe", "--color-step", "510"], 510),
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
        for invalid in ["0", "511", "1.5", "-1"] {
            assert!(Cli::try_parse_from(["ctl", "breathe", "--color-step", invalid]).is_err());
        }
        assert!(
            Cli::try_parse_from(["ctl", "breathe", "--color-step", "1", "--hue-step", "2"])
                .is_err()
        );
    }
}
