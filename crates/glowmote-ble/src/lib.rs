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
        self.send_unacked(plain).await?;
        Ok(self.wait_ack(command_tag(plain)).await)
    }

    /// Sends a scene: the `MULT` header, then its color frame on the bulk channel. Returns
    /// `false` if the light did not confirm the colors with `MULTCPOK`.
    pub async fn send_scene(&mut self, header: &ideal::Packet, frame: &[u8]) -> Result<bool> {
        if self.send(header).await?.is_none() {
            return Ok(false);
        }
        let bulk = characteristic(&self.peripheral, ideal::BULK_WRITE_CHAR_UUID)?;
        self.peripheral
            .write(&bulk, frame, WriteType::WithoutResponse)
            .await?;
        Ok(self.wait_ack("MULTCP").await.is_some())
    }

    /// The model string, e.g. `TR2411R003-07`, from the write characteristic's user description.
    pub async fn read_model(&self) -> Result<String> {
        let description = Uuid::from_u128(0x0000_2901_0000_1000_8000_0080_5f9b_34fb);
        let descriptor = self
            .write
            .descriptors
            .iter()
            .find(|d| d.uuid == description)
            .context("no user description on the write characteristic")?;
        let raw = self.peripheral.read_descriptor(descriptor).await?;
        Ok(String::from_utf8_lossy(&raw)
            .trim_end_matches('\0')
            .to_owned())
    }

    /// The two `VE` version strings, `None` for any the light did not answer.
    pub async fn versions(&mut self) -> Result<[Option<String>; 2]> {
        let mut out = [None, None];
        for (slot, second) in out.iter_mut().zip([false, true]) {
            self.send_unacked(&ideal::version_query(second)).await?;
            *slot = self
                .wait_for(|plain| ideal::parse_version(plain).map(str::to_owned))
                .await;
        }
        Ok(out)
    }

    async fn wait_ack(&mut self, tag: &str) -> Option<String> {
        self.wait_for(|plain| {
            ideal::parse_ack(plain)
                .filter(|acked| *acked == tag)
                .map(str::to_owned)
        })
        .await
    }

    async fn wait_for<T>(
        &mut self,
        mut matches: impl FnMut(&ideal::Packet) -> Option<T>,
    ) -> Option<T> {
        let found = tokio::time::timeout(ACK_TIMEOUT, async {
            while let Some(n) = self.notifications.next().await {
                if let Ok(block) = <ideal::Packet>::try_from(n.value.as_slice())
                    && let Some(value) = matches(&ideal::decrypt(&block))
                {
                    return Some(value);
                }
            }
            None
        })
        .await;
        found.ok().flatten()
    }

    /// Sends a packet the light never acknowledges, such as `DEN` or `DSTOP`.
    pub async fn send_unacked(&mut self, plain: &ideal::Packet) -> Result<()> {
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
        Ok(())
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
