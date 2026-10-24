use std::time::Duration;

use anyhow::{Context, Result};
use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::Manager;
pub use btleplug::platform::Peripheral;
use futures::StreamExt;
use glowmote_core::ideal;
use uuid::Uuid;

use crate::{Found, Notifications};

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

pub fn characteristic(peripheral: &Peripheral, uuid: &str) -> Result<Characteristic> {
    let uuid = Uuid::parse_str(uuid).expect("valid UUID constant");
    peripheral
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == uuid)
        .with_context(|| format!("characteristic {uuid} not found"))
}

pub(crate) struct Link {
    peripheral: Peripheral,
    write: Characteristic,
}

impl Link {
    pub(crate) async fn open(peripheral: &Peripheral) -> Result<(Self, Notifications)> {
        peripheral.connect().await?;
        peripheral.discover_services().await?;
        let write = characteristic(peripheral, ideal::WRITE_CHAR_UUID)?;
        let notify = characteristic(peripheral, ideal::NOTIFY_CHAR_UUID)?;
        peripheral.subscribe(&notify).await?;
        let notifications = peripheral.notifications().await?.map(|n| n.value).boxed();
        let link = Self {
            peripheral: peripheral.clone(),
            write,
        };
        Ok((link, notifications))
    }

    pub(crate) async fn is_connected(&self) -> Result<bool> {
        Ok(self.peripheral.is_connected().await?)
    }

    pub(crate) async fn write(&self, data: &[u8]) -> Result<()> {
        Ok(self
            .peripheral
            .write(&self.write, data, WriteType::WithoutResponse)
            .await?)
    }

    pub(crate) async fn write_bulk(&self, frame: &[u8]) -> Result<()> {
        let bulk = characteristic(&self.peripheral, ideal::BULK_WRITE_CHAR_UUID)?;
        Ok(self
            .peripheral
            .write(&bulk, frame, WriteType::WithoutResponse)
            .await?)
    }

    pub(crate) async fn read_model(&self) -> Result<String> {
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

    pub(crate) async fn disconnect(self) -> Result<()> {
        Ok(self.peripheral.disconnect().await?)
    }
}
