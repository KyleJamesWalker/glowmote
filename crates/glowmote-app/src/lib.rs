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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "android")]
    let builder = builder.plugin(tauri_plugin_blec::init());
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
            diy_upload
        ])
        .run(tauri::generate_context!())
        .expect("error while running glowmote");
}
