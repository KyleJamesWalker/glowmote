//! One shared light connection behind the command set the desktop app and the proxy expose.

use std::future::Future;
use std::time::Duration;

use glowmote_ble::{Found, Light};
use glowmote_core::{Rgb, ideal};
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("not connected")]
    NotConnected,
    #[error("{0} not found; scan again")]
    NotFound(String),
    #[error("{0}")]
    Device(String),
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Self::Device(format!("{e:#}"))
    }
}

impl From<Error> for String {
    fn from(e: Error) -> Self {
        e.to_string()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// `MULT` effects seen in a vendor app capture; the firmware has 12.
pub const CAPTURED_SCENE_EFFECTS: [u8; 5] = [0, 1, 7, 8, 9];

/// Finds and opens lights. [`Ble`] is the real one; tests substitute a fake.
pub trait Radio: Send + Sync + 'static {
    type Found: Clone + Send;
    type Link: Link;

    fn name(found: &Self::Found) -> &str;
    fn scan(
        &self,
        duration: Duration,
    ) -> impl Future<Output = anyhow::Result<Vec<Self::Found>>> + Send;
    fn connect(
        &self,
        found: Self::Found,
    ) -> impl Future<Output = anyhow::Result<Self::Link>> + Send;
}

/// An open connection, as [`glowmote_ble::Light`] provides it.
pub trait Link: Send + 'static {
    fn name(&self) -> &str;
    fn send(
        &mut self,
        plain: &ideal::Packet,
    ) -> impl Future<Output = anyhow::Result<Option<String>>> + Send;
    fn send_unacked(
        &mut self,
        plain: &ideal::Packet,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn send_scene(
        &mut self,
        header: &ideal::Packet,
        frame: &[u8],
    ) -> impl Future<Output = anyhow::Result<bool>> + Send;
    fn disconnect(self) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn info(&mut self) -> impl Future<Output = anyhow::Result<LightInfo>> + Send;
}

/// What a light reports about itself: the model string and the two `VE` firmware versions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LightInfo {
    pub name: String,
    pub model: Option<String>,
    pub firmware: [Option<String>; 2],
}

pub struct Ble;

impl Radio for Ble {
    type Found = Found;
    type Link = Light;

    fn name(found: &Found) -> &str {
        &found.name
    }

    async fn scan(&self, duration: Duration) -> anyhow::Result<Vec<Found>> {
        glowmote_ble::scan(duration).await
    }

    async fn connect(&self, found: Found) -> anyhow::Result<Light> {
        Light::connect(found).await
    }
}

impl Link for Light {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&mut self, plain: &ideal::Packet) -> anyhow::Result<Option<String>> {
        Light::send(self, plain).await
    }

    async fn send_unacked(&mut self, plain: &ideal::Packet) -> anyhow::Result<()> {
        Light::send_unacked(self, plain).await
    }

    async fn send_scene(&mut self, header: &ideal::Packet, frame: &[u8]) -> anyhow::Result<bool> {
        Light::send_scene(self, header, frame).await
    }

    async fn disconnect(self) -> anyhow::Result<()> {
        Light::disconnect(self).await
    }

    async fn info(&mut self) -> anyhow::Result<LightInfo> {
        Ok(LightInfo {
            name: self.name.clone(),
            model: self.read_model().await?,
            firmware: self.versions().await?,
        })
    }
}

/// Holds the last scan and at most one open light. Commands queue on the light's lock in
/// arrival order, so writes from concurrent callers never interleave.
pub struct Controller<R: Radio = Ble> {
    radio: R,
    scan_time: Duration,
    found: Mutex<Vec<R::Found>>,
    light: Mutex<Option<R::Link>>,
}

impl Controller<Ble> {
    pub fn new(scan_time: Duration) -> Self {
        Self::with_radio(Ble, scan_time)
    }
}

impl<R: Radio> Controller<R> {
    pub fn with_radio(radio: R, scan_time: Duration) -> Self {
        Self {
            radio,
            scan_time,
            found: Mutex::default(),
            light: Mutex::default(),
        }
    }

    /// The connected light's name.
    pub async fn status(&self) -> Option<String> {
        self.light
            .lock()
            .await
            .as_ref()
            .map(|l| l.name().to_owned())
    }

    pub async fn scan_lights(&self) -> Result<Vec<String>> {
        let mut found = self.found.lock().await;
        *found = self.radio.scan(self.scan_time).await?;
        Ok(found.iter().map(|f| R::name(f).to_owned()).collect())
    }

    /// Scans, then connects to the one light whose name starts with `wanted`, or the only light
    /// if `wanted` is `None`. Returns its name.
    pub async fn connect_matching(&self, wanted: Option<&str>) -> Result<String> {
        let names = self.scan_lights().await?;
        let matches: Vec<_> = names
            .into_iter()
            .filter(|n| wanted.is_none_or(|w| n.starts_with(w)))
            .collect();
        match &matches[..] {
            [] => Err(Error::NotFound(wanted.unwrap_or("a light").to_owned())),
            [name] => {
                self.connect(name).await?;
                Ok(name.clone())
            }
            _ => Err(Error::Invalid(format!(
                "several lights match: {}",
                matches.join(", ")
            ))),
        }
    }

    pub async fn connect(&self, name: &str) -> Result<()> {
        let target = self
            .found
            .lock()
            .await
            .iter()
            .find(|f| R::name(f) == name)
            .cloned()
            .ok_or_else(|| Error::NotFound(name.to_owned()))?;
        let mut slot = self.light.lock().await;
        if let Some(old) = slot.take() {
            old.disconnect().await?;
        }
        *slot = Some(self.radio.connect(target).await?);
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<()> {
        if let Some(light) = self.light.lock().await.take() {
            light.disconnect().await?;
        }
        Ok(())
    }

    pub async fn info(&self) -> Result<LightInfo> {
        let mut slot = self.light.lock().await;
        let light = slot.as_mut().ok_or(Error::NotConnected)?;
        Ok(light.info().await?)
    }

    pub async fn power(&self, on: bool) -> Result<String> {
        self.send(ideal::power(on)).await
    }

    pub async fn color(&self, rgb: &str, brightness: u8) -> Result<String> {
        self.send(ideal::solid_color(parse(rgb)?, brightness.min(100)))
            .await
    }

    pub async fn scene(
        &self,
        first: &str,
        second: &str,
        speed: u8,
        brightness: u8,
        reverse: bool,
    ) -> Result<()> {
        let frame = ideal::scene_frame([parse(first)?, parse(second)?], brightness);
        let header = ideal::scene(reverse, speed, brightness);
        let mut slot = self.light.lock().await;
        let light = slot.as_mut().ok_or(Error::NotConnected)?;
        match light.send_scene(&header, &frame).await? {
            true => Ok(()),
            false => Err(Error::Device(
                "the light did not confirm the scene colors".into(),
            )),
        }
    }

    /// A scene with any captured effect and 1 to 7 colors.
    pub async fn scene_effect(
        &self,
        effect: u8,
        colors: &[String],
        speed: u8,
        brightness: u8,
        reverse: bool,
    ) -> Result<()> {
        if !CAPTURED_SCENE_EFFECTS.contains(&effect) {
            return Err(Error::Invalid(format!(
                "scene effect {effect} is not captured yet"
            )));
        }
        let colors = colors
            .iter()
            .map(|c| parse(c))
            .collect::<Result<Vec<_>>>()?;
        let (header, frame) = ideal::multi_color(effect, reverse, speed, &colors, brightness)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let mut slot = self.light.lock().await;
        let light = slot.as_mut().ok_or(Error::NotConnected)?;
        match light.send_scene(&header, &frame).await? {
            true => Ok(()),
            false => Err(Error::Device(
                "the light did not confirm the scene colors".into(),
            )),
        }
    }

    /// Enters or leaves DIY mode; `keep_drawing` only matters when leaving.
    pub async fn diy(&self, on: bool, keep_drawing: bool) -> Result<()> {
        let packet = if on {
            ideal::diy_enter()
        } else {
            ideal::diy_exit(keep_drawing)
        };
        let mut slot = self.light.lock().await;
        let light = slot.as_mut().ok_or(Error::NotConnected)?;
        Ok(light.send_unacked(&packet).await?)
    }

    pub async fn diy_pixel(
        &self,
        index: u16,
        leds: u16,
        rgb: &str,
        brightness: u8,
    ) -> Result<String> {
        if index >= leds {
            return Err(Error::Invalid(format!(
                "index {index} is past the last LED"
            )));
        }
        let packet = ideal::diy_pixel(index, parse(rgb)?, brightness)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        self.send(packet).await
    }

    pub async fn diy_fill(&self, leds: u16, rgb: &str, brightness: u8) -> Result<String> {
        let packet = ideal::diy_fill(leds, parse(rgb)?, brightness)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        self.send(packet).await
    }

    async fn send(&self, packet: ideal::Packet) -> Result<String> {
        let mut slot = self.light.lock().await;
        let light = slot.as_mut().ok_or(Error::NotConnected)?;
        light
            .send(&packet)
            .await?
            .ok_or_else(|| Error::Device("no acknowledgement within 2s".into()))
    }
}

fn parse(rgb: &str) -> Result<Rgb> {
    rgb.parse().map_err(|e| Error::Invalid(format!("{e}")))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use super::*;

    type Log = Arc<StdMutex<Vec<ideal::Packet>>>;

    struct FakeRadio {
        names: Vec<&'static str>,
        log: Log,
    }

    struct FakeLink {
        name: String,
        log: Log,
    }

    impl Radio for FakeRadio {
        type Found = String;
        type Link = FakeLink;

        fn name(found: &String) -> &str {
            found
        }

        async fn scan(&self, _: Duration) -> anyhow::Result<Vec<String>> {
            Ok(self.names.iter().map(|n| n.to_string()).collect())
        }

        async fn connect(&self, name: String) -> anyhow::Result<FakeLink> {
            Ok(FakeLink {
                name,
                log: self.log.clone(),
            })
        }
    }

    impl Link for FakeLink {
        fn name(&self) -> &str {
            &self.name
        }

        async fn send(&mut self, plain: &ideal::Packet) -> anyhow::Result<Option<String>> {
            self.log.lock().unwrap().push(*plain);
            Ok(Some("OK".into()))
        }

        async fn send_unacked(&mut self, plain: &ideal::Packet) -> anyhow::Result<()> {
            self.log.lock().unwrap().push(*plain);
            Ok(())
        }

        async fn send_scene(&mut self, header: &ideal::Packet, _: &[u8]) -> anyhow::Result<bool> {
            self.log.lock().unwrap().push(*header);
            Ok(true)
        }

        async fn disconnect(self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn info(&mut self) -> anyhow::Result<LightInfo> {
            Ok(LightInfo {
                name: self.name.clone(),
                model: Some("TR0000R000-00".into()),
                firmware: [Some("V0".into()), None],
            })
        }
    }

    fn controller(names: Vec<&'static str>) -> (Controller<FakeRadio>, Log) {
        let log = Log::default();
        let radio = FakeRadio {
            names,
            log: log.clone(),
        };
        (Controller::with_radio(radio, Duration::ZERO), log)
    }

    #[tokio::test]
    async fn commands_need_a_connection() {
        let (c, _) = controller(vec![]);
        assert!(matches!(c.info().await, Err(Error::NotConnected)));
        assert!(matches!(c.power(true).await, Err(Error::NotConnected)));
        assert!(matches!(c.diy(true, false).await, Err(Error::NotConnected)));
    }

    #[tokio::test]
    async fn connect_needs_a_scan_first() {
        let (c, _) = controller(vec!["ISP-1"]);
        assert!(matches!(c.connect("ISP-1").await, Err(Error::NotFound(_))));
        c.scan_lights().await.unwrap();
        c.connect("ISP-1").await.unwrap();
        assert_eq!(c.status().await.as_deref(), Some("ISP-1"));
        c.disconnect().await.unwrap();
        assert_eq!(c.status().await, None);
    }

    #[tokio::test]
    async fn connect_matching_needs_exactly_one_match() {
        let (c, _) = controller(vec!["ISP-1", "ISP-2"]);
        assert!(matches!(
            c.connect_matching(None).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            c.connect_matching(Some("IDL-")).await,
            Err(Error::NotFound(_))
        ));
        assert_eq!(c.connect_matching(Some("ISP-2")).await.unwrap(), "ISP-2");
    }

    #[tokio::test]
    async fn sends_the_encoded_packets_in_order() {
        let (c, log) = controller(vec!["ISP-1"]);
        c.connect_matching(None).await.unwrap();
        c.power(true).await.unwrap();
        c.color("#ff0000", 250).await.unwrap();
        c.diy(true, false).await.unwrap();
        c.diy_pixel(3, 180, "00ff00", 50).await.unwrap();
        let red = Rgb::new(0xff, 0, 0);
        assert_eq!(
            *log.lock().unwrap(),
            [
                ideal::power(true),
                ideal::solid_color(red, 100),
                ideal::diy_enter(),
                ideal::diy_pixel(3, Rgb::new(0, 0xff, 0), 50).unwrap(),
            ]
        );
    }

    #[tokio::test]
    async fn scene_effects_are_limited_to_captured_ones() {
        let (c, log) = controller(vec!["ISP-1"]);
        c.connect_matching(None).await.unwrap();
        let colors = ["ff0000".to_string(), "00ff00".to_string()];
        c.scene_effect(8, &colors, 50, 100, false).await.unwrap();
        let red_green = [Rgb::new(0xff, 0, 0), Rgb::new(0, 0xff, 0)];
        let (header, _) = ideal::multi_color(8, false, 50, &red_green, 100).unwrap();
        assert_eq!(*log.lock().unwrap(), [header]);
        for effect in [2, 10, 12] {
            assert!(matches!(
                c.scene_effect(effect, &colors, 50, 100, false).await,
                Err(Error::Invalid(_))
            ));
        }
        assert!(matches!(
            c.scene_effect(0, &[], 50, 100, false).await,
            Err(Error::Invalid(_))
        ));
        assert_eq!(log.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rejects_bad_arguments_before_sending() {
        let (c, log) = controller(vec!["ISP-1"]);
        c.connect_matching(None).await.unwrap();
        assert!(matches!(c.color("red", 50).await, Err(Error::Invalid(_))));
        assert!(matches!(
            c.diy_pixel(180, 180, "ffffff", 50).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            c.diy_pixel(450, 500, "ffffff", 50).await,
            Err(Error::Invalid(_))
        ));
        assert!(matches!(
            c.diy_fill(401, "ffffff", 50).await,
            Err(Error::Invalid(_))
        ));
        assert!(log.lock().unwrap().is_empty());
    }
}
