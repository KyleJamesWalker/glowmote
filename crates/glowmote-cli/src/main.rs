use std::io::{IsTerminal, Write as _};
use std::time::Duration;

use anyhow::{Context, Result};
use btleplug::api::{Peripheral as _, WriteType};
use clap::{Parser, Subcommand};
use glowmote_ble::{Found, Light, characteristic, pick, scan};
use glowmote_core::{Rgb, ideal};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Parser)]
#[command(version, about = "One remote for all your LED lights")]
struct Cli {
    /// Device name, or a prefix of it (e.g. ISP-50542B)
    #[arg(short, long, global = true)]
    device: Option<String>,

    /// Seconds to scan for devices
    #[arg(long, global = true, default_value_t = 5)]
    scan_secs: u64,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List supported lights in range
    Scan,
    /// Connect once and read commands from stdin, one per line
    Shell,
    /// Step slowly through connect, subscribe, send `on`, disconnect, to see which step blinks
    Probe {
        /// Seconds to wait between steps
        #[arg(long, default_value_t = 5)]
        pause_secs: u64,
    },
    #[command(flatten)]
    Action(Action),
}

#[derive(Subcommand)]
enum Action {
    On,
    Off,
    /// Set a solid color
    Color {
        /// RRGGBB hex, e.g. ff0000
        rgb: Rgb,
        /// 0-100
        #[arg(short, long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(0..=100))]
        brightness: u8,
    },
}

impl Action {
    fn packet(&self) -> ideal::Packet {
        match *self {
            Action::On => ideal::power(true),
            Action::Off => ideal::power(false),
            Action::Color { rgb, brightness } => ideal::solid_color(rgb, brightness),
        }
    }
}

#[derive(Parser)]
#[command(no_binary_name = true, disable_version_flag = true)]
struct ShellLine {
    #[command(subcommand)]
    command: ShellCommand,
}

#[derive(Subcommand)]
enum ShellCommand {
    #[command(flatten)]
    Action(Action),
    /// Disconnect and exit
    #[command(alias = "exit")]
    Quit,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let found = scan(Duration::from_secs(cli.scan_secs)).await?;

    if let Command::Scan = cli.command {
        if found.is_empty() {
            println!("no supported lights found (close the vendor app; it holds the connection)");
        }
        for f in &found {
            println!("{}", f.name);
        }
        return Ok(());
    }

    let light = pick(found, cli.device.as_deref())?;
    let name = light.name.clone();
    match cli.command {
        Command::Scan => unreachable!(),
        Command::Shell => shell(light).await,
        Command::Probe { pause_secs } => probe(light, Duration::from_secs(pause_secs)).await,
        Command::Action(action) => {
            let mut light = Light::connect(light).await?;
            report(light.send(&action.packet()).await?);
            light.disconnect().await
        }
    }
    .with_context(|| format!("talking to {name}"))
}

fn report(ack: Option<String>) {
    match ack {
        Some(tag) => println!("ok ({tag})"),
        None => println!("sent, but no acknowledgement within 2s"),
    }
}

async fn shell(found: Found) -> Result<()> {
    let interactive = std::io::stdin().is_terminal();
    let mut light = Light::connect(found).await?;
    if interactive {
        println!("connected to {}; type help for commands", light.name);
    }

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        if interactive {
            print!("> ");
            std::io::stdout().flush()?;
        }
        let Some(line) = lines.next_line().await? else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        match ShellLine::try_parse_from(line.split_whitespace()) {
            Ok(ShellLine {
                command: ShellCommand::Quit,
            }) => break,
            Ok(ShellLine {
                command: ShellCommand::Action(action),
            }) => match light.send(&action.packet()).await {
                Ok(ack) => report(ack),
                Err(e) => println!("error: {e:#}"),
            },
            Err(e) => print!("{e}"),
        }
    }
    light.disconnect().await
}

async fn probe(found: Found, pause: Duration) -> Result<()> {
    let light = found.peripheral;
    let start = std::time::Instant::now();
    let step = |label: &str| println!("{:>5.1}s  {label}", start.elapsed().as_secs_f32());

    step("connecting");
    light.connect().await?;
    light.discover_services().await?;
    step("connected");
    tokio::time::sleep(pause).await;

    step("subscribing");
    light
        .subscribe(&characteristic(&light, ideal::NOTIFY_CHAR_UUID)?)
        .await?;
    tokio::time::sleep(pause).await;

    step("sending on");
    let write = characteristic(&light, ideal::WRITE_CHAR_UUID)?;
    light
        .write(
            &write,
            &ideal::encrypt(&ideal::power(true)),
            WriteType::WithoutResponse,
        )
        .await?;
    tokio::time::sleep(pause).await;

    step("disconnecting");
    light.disconnect().await?;
    tokio::time::sleep(pause).await;
    step("done");
    Ok(())
}
