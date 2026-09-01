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
            brightness,
            light,
        } => ControlCommand::White {
            value,
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
