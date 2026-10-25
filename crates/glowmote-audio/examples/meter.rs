//! Prints the microphone level ten times a second; speak or clap to see it move.

use std::time::Duration;

fn main() -> anyhow::Result<()> {
    let meter = glowmote_audio::Meter::start()?;
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(100));
        let level = meter.take_level();
        println!("{:5.2} {}", level, "#".repeat((level * 40.0) as usize));
    }
    Ok(())
}
