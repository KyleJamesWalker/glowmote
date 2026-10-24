use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Result, bail};
pub use blec::models::BleDevice as Peripheral;
use blec::models::{ScanFilter, WriteType};
use blec::{Connection, Handler, OnDisconnectHandler};
use futures::StreamExt;
use glowmote_core::ideal;
use tokio::time::Instant;
use uuid::Uuid;

use crate::{Found, Notifications};

/// How long to wait for `tauri-plugin-blec` to initialize blec once the app is ready.
const INIT_TIMEOUT: Duration = Duration::from_secs(10);
/// ATT header bytes that a write must fit next to its payload within the MTU.
const ATT_WRITE_OVERHEAD: usize = 3;

/// The blec handler, which `tauri-plugin-blec` creates on the main thread after startup.
async fn handler() -> Result<&'static Handler> {
    let deadline = Instant::now() + INIT_TIMEOUT;
    loop {
        if let Ok(handler) = blec::get_handler() {
            return Ok(handler);
        }
        if Instant::now() >= deadline {
            bail!("Bluetooth is not ready: blec was not initialized");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

pub async fn scan(duration: Duration) -> Result<Vec<Found>> {
    let handler = handler().await?;
    if !blec::check_permissions(true).await? {
        bail!("Bluetooth permission denied; allow Nearby devices for glowmote in Settings");
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let millis = u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
    handler
        .discover(Some(tx), millis, ScanFilter::None, false)
        .await?;

    let mut seen = HashMap::new();
    while let Some(devices) = rx.recv().await {
        for device in devices {
            seen.insert(device.address.clone(), device);
        }
    }
    let mut found: Vec<_> = seen
        .into_values()
        .filter(|d| ideal::is_ideal_name(&d.name))
        .map(|peripheral| Found {
            name: peripheral.name.clone(),
            peripheral,
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("valid UUID constant")
}

pub(crate) struct Link {
    connection: Connection,
}

impl Link {
    pub(crate) async fn open(peripheral: &Peripheral) -> Result<(Self, Notifications)> {
        let connection = handler()
            .await?
            .connect(&peripheral.address, OnDisconnectHandler::None, false)
            .await?;
        let (tx, rx) = futures::channel::mpsc::unbounded();
        connection
            .subscribe(
                uuid(ideal::NOTIFY_CHAR_UUID),
                None,
                move |value: Vec<u8>| {
                    let _ = tx.unbounded_send(value);
                },
            )
            .await?;
        Ok((Self { connection }, rx.boxed()))
    }

    pub(crate) async fn is_connected(&self) -> Result<bool> {
        Ok(self.connection.is_connected())
    }

    pub(crate) async fn write(&self, data: &[u8]) -> Result<()> {
        self.connection
            .send_data(
                uuid(ideal::WRITE_CHAR_UUID),
                None,
                data,
                WriteType::WithoutResponse,
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn write_bulk(&self, frame: &[u8]) -> Result<()> {
        let mtu = usize::from(self.connection.mtu().await?);
        if frame.len() + ATT_WRITE_OVERHEAD > mtu {
            bail!(
                "MTU {mtu} is too small for a {}-byte scene frame",
                frame.len()
            );
        }
        self.connection
            .send_data(
                uuid(ideal::BULK_WRITE_CHAR_UUID),
                None,
                frame,
                WriteType::WithoutResponse,
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn write_rhythm(&self, data: &[u8]) -> Result<()> {
        self.connection
            .send_data(
                uuid(ideal::RHYTHM_CHAR_UUID),
                None,
                data,
                WriteType::WithoutResponse,
            )
            .await?;
        Ok(())
    }

    pub(crate) async fn read_model(&self) -> Result<Option<String>> {
        Ok(None)
    }

    pub(crate) async fn disconnect(self) -> Result<()> {
        Ok(self.connection.disconnect().await?)
    }
}
