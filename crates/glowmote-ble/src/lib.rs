//! BLE transport for iDeal lights: scanning, and a connection that stays open across commands.

use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, ValueNotification,
    WriteType,
};
use btleplug::platform::{Manager, Peripheral};
use futures::{Stream, StreamExt};
use glowmote_core::ideal;
use tokio::time::Instant;
use uuid::Uuid;

const ACK_TIMEOUT: Duration = Duration::from_secs(2);
/// The vendor app never sends commands closer together than this.
const MIN_SEND_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone)]
pub struct Found {
    pub name: String,
    pub peripheral: Peripheral,
}

pub async fn scan(duration: Duration) -> Result<Vec<Found>> {
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
    for peripheral in adapter.peripherals().await? {
        if let Some(name) = peripheral.properties().await?.and_then(|p| p.local_name)
            && ideal::is_ideal_name(&name)
        {
            found.push(Found { name, peripheral });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// Picks the one light whose name starts with `wanted`, or the only light if `wanted` is `None`.
pub fn pick(found: Vec<Found>, wanted: Option<&str>) -> Result<Found> {
    let mut matches: Vec<_> = found
        .into_iter()
        .filter(|f| wanted.is_none_or(|w| f.name.starts_with(w)))
        .collect();
    match matches.len() {
        0 => bail!("no matching light found (close the vendor app; it holds the connection)"),
        1 => Ok(matches.remove(0)),
        _ => {
            let names: Vec<_> = matches.iter().map(|f| f.name.as_str()).collect();
            bail!(
                "several lights match, pick one with --device: {}",
                names.join(", ")
            )
        }
    }
}

pub fn characteristic(peripheral: &Peripheral, uuid: &str) -> Result<Characteristic> {
    let uuid = Uuid::parse_str(uuid).expect("valid UUID constant");
    peripheral
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == uuid)
        .with_context(|| format!("characteristic {uuid} not found"))
}

type Notifications = Pin<Box<dyn Stream<Item = ValueNotification> + Send>>;

/// An open connection. The lights blink 3 times each time one is opened.
pub struct Light {
    pub name: String,
    peripheral: Peripheral,
    write: Characteristic,
    notifications: Notifications,
    last_write: Option<Instant>,
}

impl Light {
    pub async fn connect(found: Found) -> Result<Self> {
        let Found { name, peripheral } = found;
        let (write, notifications) = open(&peripheral).await?;
        Ok(Self {
            name,
            peripheral,
            write,
            notifications,
            last_write: None,
        })
    }

    /// Sends a plaintext packet and returns the acknowledged tag, or `None` if no matching
    /// acknowledgement arrived. Reconnects first if the light dropped the connection.
    pub async fn send(&mut self, plain: &ideal::Packet) -> Result<Option<String>> {
        if !self.peripheral.is_connected().await? {
            (self.write, self.notifications) = open(&self.peripheral).await?;
        }
        if let Some(last) = self.last_write {
            tokio::time::sleep_until(last + MIN_SEND_INTERVAL).await;
        }
        self.last_write = Some(Instant::now());
        self.peripheral
            .write(
                &self.write,
                &ideal::encrypt(plain),
                WriteType::WithoutResponse,
            )
            .await?;

        let tag = command_tag(plain);
        let ack = tokio::time::timeout(ACK_TIMEOUT, async {
            while let Some(n) = self.notifications.next().await {
                if let Ok(block) = <ideal::Packet>::try_from(n.value.as_slice())
                    && let Some(acked) = ideal::parse_ack(&ideal::decrypt(&block))
                    && acked == tag
                {
                    return Some(acked.to_owned());
                }
            }
            None
        })
        .await;
        Ok(ack.ok().flatten())
    }

    pub async fn disconnect(self) -> Result<()> {
        Ok(self.peripheral.disconnect().await?)
    }
}

async fn open(peripheral: &Peripheral) -> Result<(Characteristic, Notifications)> {
    peripheral.connect().await?;
    peripheral.discover_services().await?;
    let write = characteristic(peripheral, ideal::WRITE_CHAR_UUID)?;
    let notify = characteristic(peripheral, ideal::NOTIFY_CHAR_UUID)?;
    peripheral.subscribe(&notify).await?;
    Ok((write, peripheral.notifications().await?))
}

fn command_tag(plain: &ideal::Packet) -> &str {
    let len = plain[1..]
        .iter()
        .take_while(|b| b.is_ascii_uppercase())
        .count();
    std::str::from_utf8(&plain[1..1 + len]).expect("ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;
    use glowmote_core::Rgb;

    #[test]
    fn command_tag_stops_at_arguments() {
        assert_eq!(command_tag(&ideal::power(true)), "TURN");
        assert_eq!(
            command_tag(&ideal::solid_color(Rgb::new(0xff, 0, 0), 100)),
            "SGLS"
        );
    }
}
