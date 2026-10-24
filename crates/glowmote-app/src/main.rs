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
            color
        ])
        .run(tauri::generate_context!())
        .expect("error while running glowmote");
}
