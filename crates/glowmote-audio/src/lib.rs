//! Loudness from the computer's default microphone, for driving the light's music mode.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};

/// Quietest level that registers, in dBFS; anything louder maps linearly up to 0 dBFS.
const FLOOR_DB: f32 = -60.0;

/// Listens on the default input device until dropped. The audio stream lives on its own thread
/// because cpal streams cannot move between threads on every platform.
pub struct Meter {
    peak: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Meter {
    pub fn start() -> Result<Self> {
        let peak = Arc::new(AtomicU32::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = thread::spawn({
            let (peak, stop) = (peak.clone(), stop.clone());
            move || match open(peak) {
                Ok(stream) => {
                    let _ = ready_tx.send(Ok(()));
                    while !stop.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(50));
                    }
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        });
        ready_rx
            .recv()
            .map_err(|_| anyhow!("microphone thread exited"))??;
        Ok(Self {
            peak,
            stop,
            thread: Some(thread),
        })
    }

    /// The loudest RMS since the last call, as 0 (silence) to 1 (full scale) on a dB scale.
    pub fn take_level(&self) -> f32 {
        let rms = f32::from_bits(self.peak.swap(0, Ordering::Relaxed));
        level_from_rms(rms)
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn level_from_rms(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

fn open(peak: Arc<AtomicU32>) -> Result<cpal::Stream> {
    let device = cpal::default_host()
        .default_input_device()
        .context("no microphone found")?;
    let config = device
        .default_input_config()
        .context("reading the microphone's format")?;
    let format = config.sample_format();
    let config = config.into();
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, peak),
        SampleFormat::I16 => build::<i16>(&device, config, peak),
        SampleFormat::U16 => build::<u16>(&device, config, peak),
        SampleFormat::I32 => build::<i32>(&device, config, peak),
        other => return Err(anyhow!("unsupported microphone format {other}")),
    }?;
    stream.play().context("starting the microphone")?;
    Ok(stream)
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    peak: Arc<AtomicU32>,
) -> Result<cpal::Stream>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            if data.is_empty() {
                return;
            }
            let sum: f32 = data
                .iter()
                .map(|&s| {
                    let v: f32 = cpal::FromSample::from_sample_(s);
                    v * v
                })
                .sum();
            let rms = (sum / data.len() as f32).sqrt();
            peak.fetch_max(rms.to_bits(), Ordering::Relaxed);
        },
        |e| eprintln!("microphone: {e}"),
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_follow_decibels() {
        assert_eq!(level_from_rms(0.0), 0.0);
        assert_eq!(level_from_rms(1.0), 1.0);
        assert!((level_from_rms(0.1) - 2.0 / 3.0).abs() < 1e-4);
        assert_eq!(level_from_rms(1e-5), 0.0);
    }
}
