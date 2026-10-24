//! BLE transport for iDeal lights: scanning, and a connection that stays open across commands.
//!
//! btleplug drives the radio everywhere except Android, where blec does.

use std::pin::Pin;
use std::time::Duration;

use anyhow::{Result, bail};
use futures::{Stream, StreamExt};
use glowmote_core::ideal;
use tokio::time::Instant;

#[cfg(target_os = "android")]
#[path = "blec.rs"]
mod backend;
#[cfg(not(target_os = "android"))]
#[path = "btleplug.rs"]
mod backend;

#[cfg(not(target_os = "android"))]
pub use backend::characteristic;
pub use backend::{Peripheral, scan};

const ACK_TIMEOUT: Duration = Duration::from_secs(2);
/// The vendor app never sends commands closer together than this.
const MIN_SEND_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone)]
pub struct Found {
    pub name: String,
    pub peripheral: Peripheral,
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

/// Values notified on the light's notify characteristic.
type Notifications = Pin<Box<dyn Stream<Item = Vec<u8>> + Send>>;

/// An open connection. The lights blink 3 times each time one is opened.
pub struct Light {
    pub name: String,
    peripheral: Peripheral,
    link: backend::Link,
    notifications: Notifications,
    last_write: Option<Instant>,
}

impl Light {
    pub async fn connect(found: Found) -> Result<Self> {
        let Found { name, peripheral } = found;
        let (link, notifications) = backend::Link::open(&peripheral).await?;
        Ok(Self {
            name,
            peripheral,
            link,
            notifications,
            last_write: None,
        })
    }

    /// Sends a plaintext packet and returns the acknowledged tag, or `None` if no matching
    /// acknowledgement arrived. Reconnects first if the connection is down.
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
        self.link.write_bulk(frame).await?;
        Ok(self.wait_ack("MULTCP").await.is_some())
    }

    /// Sends a bulk DIY drawing frame by frame, then shows it. Returns `false` if the light
    /// stopped acknowledging; the upload is then closed and discarded.
    pub async fn send_diy_upload(&mut self, upload: &ideal::DiyUpload) -> Result<bool> {
        if self.send(&upload.header).await?.is_none() {
            return Ok(false);
        }
        let mut complete = true;
        for (i, frame) in upload.frames.iter().enumerate() {
            self.link.write_bulk(frame).await?;
            let acked = self
                .wait_for(|plain| {
                    ideal::parse_frame_ack(plain)
                        .filter(|&(tag, index)| tag == "DOOTRE" && usize::from(index) == i)
                        .map(|(_, index)| index)
                })
                .await;
            if acked.is_none() {
                complete = false;
                break;
            }
        }
        // DOOTCP also resets the firmware's write offset, so send it even after a failed frame
        let closed = self.send(&ideal::diy_upload_done()).await?.is_some();
        let shown = complete && closed;
        self.send_unacked(&ideal::diy_upload_finish(shown)).await?;
        Ok(shown)
    }

    /// The model string, e.g. `TR2411R003-07`, from the write characteristic's user description.
    /// `None` where the platform cannot read GATT descriptors (Android, via blec).
    pub async fn read_model(&mut self) -> Result<Option<String>> {
        self.link.read_model().await
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
            while let Some(value) = self.notifications.next().await {
                if let Ok(block) = <ideal::Packet>::try_from(value.as_slice())
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
        if !self.link.is_connected().await? {
            (self.link, self.notifications) = backend::Link::open(&self.peripheral).await?;
        }
        if let Some(last) = self.last_write {
            tokio::time::sleep_until(last + MIN_SEND_INTERVAL).await;
        }
        self.last_write = Some(Instant::now());
        self.link.write(&ideal::encrypt(plain)).await
    }

    pub async fn disconnect(self) -> Result<()> {
        self.link.disconnect().await
    }
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
