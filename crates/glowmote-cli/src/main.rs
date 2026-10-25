use std::io::{IsTerminal, Write as _};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use btleplug::api::{Peripheral as _, WriteType};
use clap::{Parser, Subcommand, ValueEnum};
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
    /// Show the light's model and firmware versions
    Info,
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
    /// Play the two-color scene effect
    Scene {
        /// First color, RRGGBB hex
        first: Rgb,
        /// Second color, RRGGBB hex
        second: Rgb,
        /// 0-100
        #[arg(short, long, default_value_t = 50, value_parser = clap::value_parser!(u8).range(0..=100))]
        speed: u8,
        /// 0-100
        #[arg(short, long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(0..=100))]
        brightness: u8,
        /// Run the effect in the other direction
        #[arg(long)]
        reverse: bool,
    },
    /// Enter DIY mode, as the vendor app does before pixel and fill commands
    Diy,
    /// Leave DIY mode
    DiyExit {
        /// Leave the drawing showing instead of returning to the previous mode
        #[arg(long)]
        keep: bool,
    },
    /// Set one LED (DIY mode)
    Pixel {
        /// 0-based LED index
        index: u16,
        /// RRGGBB hex, e.g. ff0000
        rgb: Rgb,
        /// 0-100
        #[arg(short, long, default_value_t = 50, value_parser = clap::value_parser!(u8).range(0..=100))]
        brightness: u8,
        /// LEDs on the strip
        #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u16).range(1..=ideal::MAX_LEDS as i64))]
        leds: u16,
    },
    /// Stream music levels for a fixed pattern instead of a microphone
    Music {
        /// Music effect, 0-9
        #[arg(value_parser = clap::value_parser!(u8).range(0..ideal::RHYTHM_EFFECT_COUNT as i64))]
        effect: u8,
        /// Level pattern to stream
        #[arg(value_enum)]
        pattern: Pattern,
        /// Seconds to stream
        #[arg(long, default_value_t = 6)]
        secs: u64,
    },
    /// Leave music mode, restoring the previous mode
    MusicStop,
    /// Set every LED (DIY mode)
    Fill {
        /// RRGGBB hex, e.g. ff0000
        rgb: Rgb,
        /// 0-100
        #[arg(short, long, default_value_t = 50, value_parser = clap::value_parser!(u8).range(0..=100))]
        brightness: u8,
        /// LEDs on the strip
        #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u16).range(1..=ideal::MAX_LEDS as i64))]
        leds: u16,
    },
}

/// The vendor app's pace for music level blocks.
const MUSIC_TICK: Duration = Duration::from_millis(120);

#[derive(Clone, Copy, ValueEnum)]
enum Pattern {
    /// Level 1 throughout
    Quiet,
    /// Level 100 throughout
    Loud,
    /// 1 up to 100 and back down, over 3 seconds each way
    Ramp,
    /// Alternate 1 and 100 every second
    Beat,
}

impl Pattern {
    fn level(self, tick: u64) -> u8 {
        let elapsed = tick * MUSIC_TICK.as_millis() as u64;
        match self {
            Pattern::Quiet => 1,
            Pattern::Loud => 100,
            Pattern::Ramp => {
                let phase = elapsed % 6000;
                let up = if phase < 3000 { phase } else { 6000 - phase };
                (1 + up * 99 / 3000) as u8
            }
            Pattern::Beat => {
                if (elapsed / 1000).is_multiple_of(2) {
                    100
                } else {
                    1
                }
            }
        }
    }
}

impl Action {
    async fn run(&self, light: &mut Light) -> Result<()> {
        let packet = match *self {
            Action::On => ideal::power(true),
            Action::Off => ideal::power(false),
            Action::Color { rgb, brightness } => ideal::solid_color(rgb, brightness),
            Action::Scene {
                first,
                second,
                speed,
                brightness,
                reverse,
            } => {
                let header = ideal::scene(reverse, speed, brightness);
                let frame = ideal::scene_frame([first, second], brightness);
                if light.send_scene(&header, &frame).await? {
                    println!("ok (MULTCP)");
                } else {
                    println!("sent, but the light did not confirm the colors");
                }
                return Ok(());
            }
            Action::Diy => {
                return light
                    .send_unacked(&ideal::diy_enter())
                    .await
                    .map(|()| println!("sent"));
            }
            Action::DiyExit { keep } => {
                return light
                    .send_unacked(&ideal::diy_exit(keep))
                    .await
                    .map(|()| println!("sent"));
            }
            Action::Pixel {
                index,
                rgb,
                brightness,
                leds,
            } => {
                if index >= leds {
                    bail!("index {index} is past the last LED ({})", leds - 1);
                }
                ideal::diy_pixel(index, rgb, brightness)?
            }
            Action::Fill {
                rgb,
                brightness,
                leds,
            } => ideal::diy_fill(leds, rgb, brightness)?,
            Action::Music {
                effect,
                pattern,
                secs,
            } => {
                let ticks =
                    Duration::from_secs(secs).as_millis() as u64 / MUSIC_TICK.as_millis() as u64;
                for tick in 0..ticks {
                    light
                        .send_rhythm(&ideal::rhythm(effect, pattern.level(tick))?)
                        .await?;
                    tokio::time::sleep(MUSIC_TICK).await;
                }
                println!("sent {ticks} levels");
                return Ok(());
            }
            Action::MusicStop => {
                return light
                    .send_unacked(&ideal::rhythm_stop())
                    .await
                    .map(|()| println!("sent"));
            }
        };
        match light.send(&packet).await? {
            Some(tag) => println!("ok ({tag})"),
            None => println!("sent, but no acknowledgement within 2s"),
        }
        Ok(())
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
        Command::Info => info(light).await,
        Command::Shell => shell(light).await,
        Command::Probe { pause_secs } => probe(light, Duration::from_secs(pause_secs)).await,
        Command::Action(action) => {
            let mut light = Light::connect(light).await?;
            action.run(&mut light).await?;
            light.disconnect().await
        }
    }
    .with_context(|| format!("talking to {name}"))
}

async fn info(found: Found) -> Result<()> {
    let mut light = Light::connect(found).await?;
    println!("name:     {}", light.name);
    let model = light.read_model().await?;
    println!(
        "model:    {}",
        model.as_deref().unwrap_or("unavailable on this platform")
    );
    let [first, second] = light.versions().await?;
    let show = |v: Option<String>| v.unwrap_or_else(|| "no reply".into());
    println!("firmware: {}", show(first));
    println!("          {}", show(second));
    light.disconnect().await
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
            }) => {
                if let Err(e) = action.run(&mut light).await {
                    println!("error: {e:#}");
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_stay_inside_1_to_100() {
        for pattern in [Pattern::Quiet, Pattern::Loud, Pattern::Ramp, Pattern::Beat] {
            for tick in 0..200 {
                assert!((1..=100).contains(&pattern.level(tick)));
            }
        }
        assert_eq!(Pattern::Ramp.level(0), 1);
        assert_eq!(Pattern::Ramp.level(25), 100);
        assert_eq!(Pattern::Beat.level(0), 100);
        assert_eq!(Pattern::Beat.level(9), 1);
    }
}
