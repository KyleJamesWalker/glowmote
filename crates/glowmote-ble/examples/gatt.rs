use std::time::Duration;

use btleplug::api::Peripheral as _;
use glowmote_ble::{pick, scan};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let light = pick(scan(Duration::from_secs(5)).await?, None)?.peripheral;
    light.connect().await?;
    light.discover_services().await?;
    for s in light.services() {
        println!("service {}", s.uuid);
        for c in &s.characteristics {
            let value = if c.properties.contains(btleplug::api::CharPropFlags::READ) {
                light
                    .read(c)
                    .await
                    .map(|v| format!(" = {:02x?} {:?}", v, String::from_utf8_lossy(&v)))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            println!("  char {} {:?}{}", c.uuid, c.properties, value);
            for d in &c.descriptors {
                let v = light
                    .read_descriptor(d)
                    .await
                    .map(|v| format!(" = {:02x?} {:?}", v, String::from_utf8_lossy(&v)))
                    .unwrap_or_else(|e| format!(" (read failed: {e})"));
                println!("    desc {}{}", d.uuid, v);
            }
        }
    }
    light.disconnect().await?;
    Ok(())
}
