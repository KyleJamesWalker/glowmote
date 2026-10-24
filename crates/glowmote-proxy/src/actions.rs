//! Named sequences of light commands, triggered with `POST /api/action/<name>` by anything that can
//! send an HTTP request: a motion sensor, a button, a script.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use glowmote_control::{Controller, Radio};
use glowmote_core::Rgb;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::task::AbortHandle;

use crate::{ApiError, Args, NoArgs};

const MAX_WAIT: Duration = Duration::from_secs(600);
const MAX_REPEAT: u32 = 1000;
const MAX_DEPTH: usize = 4;

/// One step of an action. Only commands whose bytes are captured are available.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    Power(bool),
    Color {
        rgb: String,
        #[serde(default = "full")]
        brightness: u8,
    },
    Scene {
        first: String,
        second: String,
        #[serde(default = "half")]
        speed: u8,
        #[serde(default = "full")]
        brightness: u8,
        #[serde(default)]
        reverse: bool,
    },
    WaitMs(u64),
    Repeat {
        times: u32,
        steps: Vec<Step>,
    },
}

fn full() -> u8 {
    100
}

fn half() -> u8 {
    50
}

/// Parses an actions file (`{"name": [step, ...], ...}`) and checks every step, so a bad color or
/// range fails at startup rather than in the middle of an action.
pub fn parse(json: &str) -> Result<BTreeMap<String, Vec<Step>>, String> {
    let defs: BTreeMap<String, Vec<Step>> =
        serde_json::from_str(json).map_err(|e| format!("actions: {e}"))?;
    for (name, steps) in &defs {
        check(steps, 0).map_err(|e| format!("action {name}: {e}"))?;
    }
    Ok(defs)
}

fn check(steps: &[Step], depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("repeats nest deeper than {MAX_DEPTH}"));
    }
    let rgb = |s: &str| s.parse::<Rgb>().map(|_| ()).map_err(|e| e.to_string());
    let pct = |what: &str, v: u8| {
        (v <= 100)
            .then_some(())
            .ok_or_else(|| format!("{what} {v} is above 100"))
    };
    for step in steps {
        match step {
            Step::Power(_) => {}
            Step::Color { rgb: c, brightness } => {
                rgb(c)?;
                pct("brightness", *brightness)?;
            }
            Step::Scene {
                first,
                second,
                speed,
                brightness,
                ..
            } => {
                rgb(first)?;
                rgb(second)?;
                pct("speed", *speed)?;
                pct("brightness", *brightness)?;
            }
            Step::WaitMs(ms) => {
                if Duration::from_millis(*ms) > MAX_WAIT {
                    return Err(format!("wait of {ms} ms is longer than {MAX_WAIT:?}"));
                }
            }
            Step::Repeat { times, steps } => {
                if *times > MAX_REPEAT {
                    return Err(format!("repeat {times} is more than {MAX_REPEAT}"));
                }
                check(steps, depth + 1)?;
            }
        }
    }
    Ok(())
}

/// Runs one action at a time; triggering another stops the one in progress.
pub struct Actions<R: Radio> {
    controller: Arc<Controller<R>>,
    defs: BTreeMap<String, Vec<Step>>,
    running: Mutex<Option<AbortHandle>>,
}

impl<R: Radio> Actions<R> {
    pub fn new(controller: Arc<Controller<R>>, defs: BTreeMap<String, Vec<Step>>) -> Self {
        Self {
            controller,
            defs,
            running: Mutex::default(),
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.defs.keys().cloned().collect()
    }

    /// Starts the named action in the background. Returns `false` if there is no such action.
    pub fn trigger(&self, name: &str) -> bool {
        let Some(steps) = self.defs.get(name).cloned() else {
            return false;
        };
        let controller = self.controller.clone();
        let name = name.to_owned();
        let task = tokio::spawn(async move {
            if let Err(e) = run(&controller, &steps).await {
                eprintln!("action {name} stopped: {e}");
            }
        });
        self.replace(Some(task.abort_handle()));
        true
    }

    pub fn stop(&self) {
        self.replace(None);
    }

    fn replace(&self, next: Option<AbortHandle>) {
        let mut running = self.running.lock().expect("actions lock");
        if let Some(previous) = std::mem::replace(&mut *running, next) {
            previous.abort();
        }
    }
}

fn run<'a, R: Radio>(
    c: &'a Controller<R>,
    steps: &'a [Step],
) -> Pin<Box<dyn Future<Output = glowmote_control::Result<()>> + Send + 'a>> {
    Box::pin(async move {
        for step in steps {
            match step {
                Step::Power(on) => {
                    c.power(*on).await?;
                }
                Step::Color { rgb, brightness } => {
                    c.color(rgb, *brightness).await?;
                }
                Step::Scene {
                    first,
                    second,
                    speed,
                    brightness,
                    reverse,
                } => {
                    c.scene(first, second, *speed, *brightness, *reverse)
                        .await?
                }
                Step::WaitMs(ms) => tokio::time::sleep(Duration::from_millis(*ms)).await,
                Step::Repeat { times, steps } => {
                    for _ in 0..*times {
                        run(c, steps).await?;
                    }
                }
            }
        }
        Ok(())
    })
}

pub fn router<R: Radio>(actions: Arc<Actions<R>>) -> Router {
    Router::new()
        .route("/api/actions", get(list::<R>))
        .route("/api/action/{name}", post(start::<R>))
        .route("/api/action_stop", post(stop::<R>))
        .with_state(actions)
}

type Acts<R> = State<Arc<Actions<R>>>;

async fn list<R: Radio>(State(a): Acts<R>) -> Json<Value> {
    Json(json!({ "ok": a.names() }))
}

async fn start<R: Radio>(
    State(a): Acts<R>,
    Path(name): Path<String>,
    _: Args<NoArgs>,
) -> Result<Json<Value>, ApiError> {
    if a.trigger(&name) {
        Ok(Json(json!({ "ok": "started" })))
    } else {
        Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("unknown action {name}"),
        ))
    }
}

async fn stop<R: Radio>(State(a): Acts<R>, _: Args<NoArgs>) -> Json<Value> {
    a.stop();
    Json(json!({ "ok": null }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_checks_steps() {
        let defs = parse(
            r#"{"scare": [
                {"power": true},
                {"repeat": {"times": 3, "steps": [
                    {"color": {"rgb": "ffffff"}}, {"wait_ms": 100}, {"power": false}, {"wait_ms": 100}
                ]}},
                {"scene": {"first": "ff0000", "second": "200000", "speed": 90}}
            ]}"#,
        )
        .unwrap();
        assert_eq!(defs["scare"].len(), 3);
    }

    #[test]
    fn example_file_is_valid() {
        let defs = parse(include_str!("../actions.example.json")).unwrap();
        assert_eq!(defs.keys().collect::<Vec<_>>(), ["off", "porch", "scare"]);
    }

    #[test]
    fn rejects_bad_steps_at_load() {
        for bad in [
            r#"{"a": [{"color": {"rgb": "red"}}]}"#,
            r#"{"a": [{"color": {"rgb": "ff0000", "brightness": 101}}]}"#,
            r#"{"a": [{"scene": {"first": "ff0000", "second": "00ff00", "speed": 200}}]}"#,
            r#"{"a": [{"wait_ms": 3600000}]}"#,
            r#"{"a": [{"repeat": {"times": 5000, "steps": []}}]}"#,
            r#"{"a": [{"blink": true}]}"#,
            r#"{"a": [{"color": {"rgb": "ff0000", "speed": 5}}]}"#,
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }
}
