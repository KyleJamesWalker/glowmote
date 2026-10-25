use std::time::Duration;

use glowmote_control::{Controller, LightInfo, LocalTime, TimerSetting};
use tauri::State;

const SCAN_TIME: Duration = Duration::from_secs(4);

type Ctl<'a> = State<'a, Controller>;
type CmdResult<T> = Result<T, String>;

#[tauri::command]
async fn status(c: Ctl<'_>) -> CmdResult<Option<String>> {
    Ok(c.status().await)
}

#[tauri::command]
async fn scan_lights(c: Ctl<'_>) -> CmdResult<Vec<String>> {
    Ok(c.scan_lights().await?)
}

#[tauri::command]
async fn connect(c: Ctl<'_>, name: String) -> CmdResult<()> {
    Ok(c.connect(&name).await?)
}

#[tauri::command]
async fn disconnect(c: Ctl<'_>) -> CmdResult<()> {
    Ok(c.disconnect().await?)
}

#[tauri::command]
async fn info(c: Ctl<'_>) -> CmdResult<LightInfo> {
    Ok(c.info().await?)
}

#[tauri::command]
async fn power(c: Ctl<'_>, on: bool) -> CmdResult<String> {
    Ok(c.power(on).await?)
}

#[tauri::command]
async fn color(c: Ctl<'_>, rgb: String, brightness: u8) -> CmdResult<String> {
    Ok(c.color(&rgb, brightness).await?)
}

#[tauri::command]
async fn scene_effect(
    c: Ctl<'_>,
    effect: u8,
    colors: Vec<String>,
    speed: u8,
    brightness: u8,
    reverse: bool,
) -> CmdResult<()> {
    Ok(c.scene_effect(effect, &colors, speed, brightness, reverse)
        .await?)
}

#[tauri::command]
async fn timers(
    c: Ctl<'_>,
    now: LocalTime,
    on: Option<TimerSetting>,
    off: Option<TimerSetting>,
) -> CmdResult<String> {
    Ok(c.timers(now, on, off).await?)
}

#[tauri::command]
async fn diy(c: Ctl<'_>, on: bool, keep: Option<bool>) -> CmdResult<()> {
    Ok(c.diy(on, keep.unwrap_or(false)).await?)
}

#[tauri::command]
async fn diy_pixel(
    c: Ctl<'_>,
    index: u16,
    leds: u16,
    rgb: String,
    brightness: u8,
) -> CmdResult<String> {
    Ok(c.diy_pixel(index, leds, &rgb, brightness).await?)
}

#[tauri::command]
async fn diy_fill(c: Ctl<'_>, leds: u16, rgb: String, brightness: u8) -> CmdResult<String> {
    Ok(c.diy_fill(leds, &rgb, brightness).await?)
}

#[tauri::command]
async fn diy_upload(c: Ctl<'_>, pixels: Vec<String>) -> CmdResult<()> {
    Ok(c.diy_upload(&pixels).await?)
}

#[tauri::command]
async fn music(c: Ctl<'_>, on: bool, effect: u8) -> CmdResult<()> {
    Ok(c.music(on, effect).await?)
}

#[cfg(not(target_os = "android"))]
mod listen {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::time::Duration;

    use glowmote_audio::Meter;
    use glowmote_control::Controller;
    use tauri::async_runtime::{self, JoinHandle};
    use tauri::{AppHandle, Emitter, Manager};
    use tokio::sync::Mutex;

    /// The vendor app's pace for level blocks.
    const TICK: Duration = Duration::from_millis(120);

    #[derive(Default)]
    pub struct Listening {
        task: Mutex<Option<JoinHandle<()>>>,
        effect: Arc<AtomicU8>,
        sensitivity: Arc<AtomicU8>,
    }

    impl Listening {
        pub fn tune(&self, effect: u8, sensitivity: u8) {
            self.effect.store(effect, Ordering::Relaxed);
            self.sensitivity
                .store(sensitivity.min(100), Ordering::Relaxed);
        }

        pub async fn start(&self, app: AppHandle) -> Result<(), String> {
            let mut task = self.task.lock().await;
            if task.as_ref().is_some_and(|t| !t.inner().is_finished()) {
                return Ok(());
            }
            let meter = Meter::start().map_err(|e| format!("{e:#}"))?;
            app.state::<Controller>().music_stop().await?;
            let (effect, sensitivity) = (self.effect.clone(), self.sensitivity.clone());
            *task = Some(async_runtime::spawn(async move {
                let controller = app.state::<Controller>();
                loop {
                    tokio::time::sleep(TICK).await;
                    let gain = f32::from(sensitivity.load(Ordering::Relaxed)) / 50.0;
                    let level = (meter.take_level() * gain * 100.0).clamp(1.0, 100.0) as u8;
                    let _ = app.emit("music-level", level);
                    let effect = effect.load(Ordering::Relaxed);
                    if let Err(e) = controller.music_level(effect, level).await {
                        let _ = app.emit("music-error", e.to_string());
                        break;
                    }
                }
            }));
            Ok(())
        }

        pub async fn stop(&self, controller: &Controller) -> Result<(), String> {
            if let Some(task) = self.task.lock().await.take() {
                task.abort();
                controller.music_stop().await?;
            }
            Ok(())
        }
    }
}

#[cfg(not(target_os = "android"))]
#[tauri::command]
async fn listen(
    app: tauri::AppHandle,
    c: Ctl<'_>,
    l: State<'_, listen::Listening>,
    on: bool,
    effect: u8,
    sensitivity: u8,
) -> CmdResult<()> {
    l.tune(effect, sensitivity);
    if on {
        l.start(app).await
    } else {
        l.stop(&c).await
    }
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn listen(on: bool, effect: u8, sensitivity: u8) -> CmdResult<()> {
    let _ = (on, effect, sensitivity);
    Err("listening with this device's microphone is not supported on Android yet".into())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "android")]
    let builder = builder.plugin(tauri_plugin_blec::init());
    #[cfg(not(target_os = "android"))]
    let builder = builder.manage(listen::Listening::default());
    builder
        .manage(Controller::new(SCAN_TIME))
        .invoke_handler(tauri::generate_handler![
            status,
            scan_lights,
            connect,
            disconnect,
            info,
            power,
            color,
            scene_effect,
            timers,
            diy,
            diy_pixel,
            diy_fill,
            diy_upload,
            music,
            listen
        ])
        .run(tauri::generate_context!())
        .expect("error while running glowmote");
}
