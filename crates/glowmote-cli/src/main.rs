use std::time::Duration;

use anyhow::{Context, Result, bail};
use btleplug::api::{Central, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Manager, Peripheral};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use glowmote_core::{Rgb, ideal};
use uuid::Uuid;

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
    /// Step slowly through connect, subscribe, send `on`, disconnect, to see which step blinks
    Probe {
        /// Seconds to wait between steps
        #[arg(long, default_value_t = 5)]
        pause_secs: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let found = scan(Duration::from_secs(cli.scan_secs)).await?;

    let packet = match cli.command {
        Command::Scan => {
            if found.is_empty() {
                println!(
                    "no supported lights found (close the vendor app; it holds the connection)"
                );
            }
            for (name, _) in &found {
                println!("{name}");
            }
            return Ok(());
        }
        Command::Probe { pause_secs } => {
            let (name, light) = pick(found, cli.device.as_deref())?;
            return probe(&light, Duration::from_secs(pause_secs))
                .await
                .with_context(|| format!("probing {name}"));
        }
        Command::On => ideal::power(true),
        Command::Off => ideal::power(false),
        Command::Color { rgb, brightness } => ideal::solid_color(rgb, brightness),
    };

    let (name, light) = pick(found, cli.device.as_deref())?;
    send(&light, &packet)
        .await
        .with_context(|| format!("sending to {name}"))
}

async fn scan(duration: Duration) -> Result<Vec<(String, Peripheral)>> {
    let manager = Manager::new().await?;
    let adapter = manager
        .adapters()
        .await?
        .into_iter()
        .next()
        .context("no Bluetooth adapter")?;
    adapter.start_scan(ScanFilter::default()).await?;
    tokio::time::sleep(duration).await;
    adapter.stop_scan().await?;

    let mut found = Vec::new();
    for p in adapter.peripherals().await? {
        if let Some(name) = p.properties().await?.and_then(|props| props.local_name)
            && ideal::is_ideal_name(&name)
        {
            found.push((name, p));
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

fn pick(found: Vec<(String, Peripheral)>, wanted: Option<&str>) -> Result<(String, Peripheral)> {
    let mut matches: Vec<_> = found
        .into_iter()
        .filter(|(name, _)| wanted.is_none_or(|w| name.starts_with(w)))
        .collect();
    match matches.len() {
        0 => bail!("no matching light found (close the vendor app; it holds the connection)"),
        1 => Ok(matches.remove(0)),
        _ => {
            let names: Vec<_> = matches.iter().map(|(n, _)| n.as_str()).collect();
            bail!(
                "several lights match, pick one with --device: {}",
                names.join(", ")
            )
        }
    }
}

async fn probe(light: &Peripheral, pause: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    let step = |label: &str| println!("{:>5.1}s  {label}", start.elapsed().as_secs_f32());

    step("connecting");
    light.connect().await?;
    light.discover_services().await?;
    step("connected");
    tokio::time::sleep(pause).await;

    let notify = light
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == Uuid::parse_str(ideal::NOTIFY_CHAR_UUID).expect("valid UUID constant"))
        .context("notify characteristic not found")?;
    step("subscribing");
    light.subscribe(&notify).await?;
    tokio::time::sleep(pause).await;

    let write = light
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == Uuid::parse_str(ideal::WRITE_CHAR_UUID).expect("valid UUID constant"))
        .context("write characteristic not found")?;
    step("sending on");
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

async fn send(light: &Peripheral, plain: &ideal::Packet) -> Result<()> {
    light.connect().await?;
    light.discover_services().await?;
    let chars = light.characteristics();
    let find = |uuid: &str| {
        let uuid = Uuid::parse_str(uuid).expect("valid UUID constant");
        chars.iter().find(|c| c.uuid == uuid).cloned()
    };
    let write = find(ideal::WRITE_CHAR_UUID).context("write characteristic not found")?;
    let notify = find(ideal::NOTIFY_CHAR_UUID).context("notify characteristic not found")?;

    light.subscribe(&notify).await?;
    let mut notifications = light.notifications().await?;
    light
        .write(&write, &ideal::encrypt(plain), WriteType::WithoutResponse)
        .await?;

    let ack = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(n) = notifications.next().await {
            if let Ok(block) = <ideal::Packet>::try_from(n.value.as_slice()) {
                return ideal::parse_ack(&ideal::decrypt(&block)).map(str::to_owned);
            }
        }
        None
    })
    .await;
    light.disconnect().await?;

    match ack {
        Ok(Some(cmd)) => println!("ok ({cmd})"),
        _ => println!("sent, but no acknowledgement within 2s"),
    }
    Ok(())
}
