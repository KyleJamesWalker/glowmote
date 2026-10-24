#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;

use glowmote_ble::{Found, Light, scan};
use glowmote_core::{Rgb, ideal};
use tauri::State;
use tokio::sync::Mutex;

const SCAN_TIME: Duration = Duration::from_secs(4);

#[derive(Default)]
struct App {
    found: Mutex<Vec<Found>>,
    light: Mutex<Option<Light>>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

#[tauri::command]
async fn scan_lights(app: State<'_, App>) -> CmdResult<Vec<String>> {
    let found = scan(SCAN_TIME).await.map_err(err)?;
    let names = found.iter().map(|f| f.name.clone()).collect();
    *app.found.lock().await = found;
    Ok(names)
}

#[tauri::command]
async fn connect(app: State<'_, App>, name: String) -> CmdResult<()> {
    let target = app
        .found
        .lock()
        .await
        .iter()
        .find(|f| f.name == name)
        .cloned()
        .ok_or_else(|| format!("{name} not found; scan again"))?;
    let mut slot = app.light.lock().await;
    if let Some(old) = slot.take() {
        old.disconnect().await.map_err(err)?;
    }
    *slot = Some(Light::connect(target).await.map_err(err)?);
    Ok(())
}

#[tauri::command]
async fn disconnect(app: State<'_, App>) -> CmdResult<()> {
    if let Some(light) = app.light.lock().await.take() {
        light.disconnect().await.map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
async fn power(app: State<'_, App>, on: bool) -> CmdResult<String> {
    send(&app, ideal::power(on)).await
}

#[tauri::command]
async fn color(app: State<'_, App>, rgb: String, brightness: u8) -> CmdResult<String> {
    let rgb: Rgb = rgb.parse().map_err(|e| format!("{e}"))?;
    send(&app, ideal::solid_color(rgb, brightness.min(100))).await
}

#[tauri::command]
async fn scene(
    app: State<'_, App>,
    first: String,
    second: String,
    speed: u8,
    brightness: u8,
    reverse: bool,
) -> CmdResult<()> {
    let parse = |hex: &str| hex.parse::<Rgb>().map_err(|e| format!("{e}"));
    let frame = ideal::scene_frame([parse(&first)?, parse(&second)?], brightness);
    let header = ideal::scene(reverse, speed, brightness);
    let mut slot = app.light.lock().await;
    let light = slot.as_mut().ok_or("not connected")?;
    match light.send_scene(&header, &frame).await.map_err(err)? {
        true => Ok(()),
        false => Err("the light did not confirm the scene colors".into()),
    }
}

#[tauri::command]
async fn diy(app: State<'_, App>, on: bool) -> CmdResult<()> {
    let packet = if on {
        ideal::diy_enter()
    } else {
        ideal::diy_exit(false)
    };
    let mut slot = app.light.lock().await;
    let light = slot.as_mut().ok_or("not connected")?;
    light.send_unacked(&packet).await.map_err(err)
}

#[tauri::command]
async fn diy_pixel(
    app: State<'_, App>,
    index: u16,
    leds: u16,
    rgb: String,
    brightness: u8,
) -> CmdResult<String> {
    if index >= leds {
        return Err(format!("index {index} is past the last LED"));
    }
    let rgb: Rgb = rgb.parse().map_err(|e| format!("{e}"))?;
    send(&app, ideal::diy_pixel(index, rgb, brightness)).await
}

#[tauri::command]
async fn diy_fill(
    app: State<'_, App>,
    leds: u16,
    rgb: String,
    brightness: u8,
) -> CmdResult<String> {
    let rgb: Rgb = rgb.parse().map_err(|e| format!("{e}"))?;
    send(&app, ideal::diy_fill(leds, rgb, brightness)).await
}

async fn send(app: &App, packet: ideal::Packet) -> CmdResult<String> {
    let mut slot = app.light.lock().await;
    let light = slot.as_mut().ok_or("not connected")?;
    light
        .send(&packet)
        .await
        .map_err(err)?
        .ok_or_else(|| "no acknowledgement within 2s".to_owned())
}

fn main() {
    tauri::Builder::default()
        .manage(App::default())
        .invoke_handler(tauri::generate_handler![
            scan_lights,
            connect,
            disconnect,
            power,
            color,
            scene,
            diy,
            diy_pixel,
            diy_fill
        ])
        .run(tauri::generate_context!())
        .expect("error while running glowmote");
}
