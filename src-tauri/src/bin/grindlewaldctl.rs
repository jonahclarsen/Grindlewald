use clap::{Parser, Subcommand, ValueEnum};
use grindlewald_lib::{
    command::{CommandResponse, ControlCommand},
    ipc::socket_path,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

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
        #[arg(long, default_value_t = 2.0, value_parser = clap::value_parser!(f32))]
        pace: f32,
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
        CliCommand::Breathe { pace, light } => ControlCommand::Breathe {
            pace_seconds: pace,
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
