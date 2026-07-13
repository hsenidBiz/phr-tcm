//! System-audio spectrum for the flask equalizer rings. Captures whatever is
//! playing on the default output device via WASAPI loopback (an input stream
//! built on an output device - Windows only, cpal's documented loopback
//! path), FFTs it into log-spaced bands and emits `AudioSpectrum` events
//! ~30x/second while a subscriber is active.
//!
//! Decorative-only: every failure (no device, exclusive-mode output, capture
//! error) just means no events - the UI keeps its ambient animation.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tauri_specta::Event;

/// Bands emitted per frame (one bar each on the ring).
pub const BAND_COUNT: usize = 36;
const FFT_SIZE: usize = 2048;
const FRAME_MS: u64 = 33;
/// Band range: below ~40 Hz is rumble, above ~16 kHz is air.
const F_LO: f32 = 40.0;
const F_HI: f32 = 16_000.0;

#[derive(Clone, serde::Serialize, specta::Type, tauri_specta::Event)]
pub struct AudioSpectrum {
    /// `BAND_COUNT` values in 0..=1, low frequencies first.
    pub bands: Vec<f32>,
}

/// Hann window + FFT + log-spaced band energies, auto-gain applied by the
/// caller. Pure so it can be golden-tested without a sound card.
pub fn analyze(samples: &[f32], sample_rate: f32, n_bands: usize) -> Vec<f32> {
    use rustfft::{num_complex::Complex, FftPlanner};
    let n = samples.len();
    if n == 0 || sample_rate <= 0.0 {
        return vec![0.0; n_bands];
    }
    let mut buf: Vec<Complex<f32>> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let w = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos();
            Complex::new(s * w, 0.0)
        })
        .collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);

    let bin_hz = sample_rate / n as f32;
    let hi = F_HI.min(sample_rate / 2.0);
    let ratio = hi / F_LO;
    let mut bands = vec![0.0f32; n_bands];
    for (b, out) in bands.iter_mut().enumerate() {
        let f0 = F_LO * ratio.powf(b as f32 / n_bands as f32);
        let f1 = F_LO * ratio.powf((b + 1) as f32 / n_bands as f32);
        let i0 = ((f0 / bin_hz) as usize).max(1);
        let i1 = (((f1 / bin_hz) as usize).max(i0 + 1)).min(n / 2);
        let sum: f32 = buf[i0..i1].iter().map(|c| c.norm()).sum();
        // Average magnitude, sqrt-compressed so quiet detail stays visible.
        *out = (sum / (i1 - i0) as f32 / n as f32).sqrt();
    }
    bands
}

struct Capture {
    stop: Arc<AtomicBool>,
}

fn slot() -> &'static Mutex<Option<Capture>> {
    static SLOT: OnceLock<Mutex<Option<Capture>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Start the loopback capture thread (no-op if already running).
pub fn start(app: tauri::AppHandle) -> Result<(), String> {
    let mut guard = slot().lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    // The cpal Stream is !Send: build it, poll it and drop it on one thread.
    std::thread::Builder::new()
        .name("audio-spectrum".into())
        .spawn(move || capture_loop(app, stop_thread))
        .map_err(|e| e.to_string())?;
    *guard = Some(Capture { stop });
    Ok(())
}

/// Signal the capture thread to end (no-op if not running).
pub fn stop() {
    if let Some(c) = slot().lock().unwrap().take() {
        c.stop.store(true, Ordering::Relaxed);
    }
}

fn capture_loop(app: tauri::AppHandle, stop: Arc<AtomicBool>) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let clear = || {
        // Whatever way we exit, free the slot so a later start() can retry
        // (e.g. after the user plugs in a device).
        slot().lock().unwrap().take();
    };

    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        return clear();
    };
    let Ok(config) = device.default_output_config() else {
        return clear();
    };
    let sample_rate = config.sample_rate() as f32;
    let channels = (config.channels() as usize).max(1);

    let ring: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    let ring_cb = ring.clone();
    let on_err = |_e: cpal::Error| {};
    // Loopback: an INPUT stream on the OUTPUT device (WASAPI). Mix to mono.
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config.into(),
            move |data: &[f32], _: &_| push_mono(&ring_cb, data, channels),
            on_err,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config.into(),
            move |data: &[i16], _: &_| {
                let f: Vec<f32> = data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                push_mono(&ring_cb, &f, channels);
            },
            on_err,
            None,
        ),
        _ => return clear(),
    };
    let Ok(stream) = stream else {
        return clear();
    };
    if stream.play().is_err() {
        return clear();
    }

    // Auto-gain: normalize by a slowly-decaying running peak so the ring
    // looks alive at any playback volume.
    let mut peak = 1e-4f32;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(FRAME_MS));
        let samples: Vec<f32> = {
            let mut r = ring.lock().unwrap();
            while r.len() > FFT_SIZE {
                r.pop_front();
            }
            if r.len() < FFT_SIZE {
                continue; // not enough audio yet this frame
            }
            r.iter().copied().collect()
        };
        let mut bands = analyze(&samples, sample_rate, BAND_COUNT);
        let frame_max = bands.iter().copied().fold(0.0f32, f32::max);
        peak = (peak * 0.995).max(frame_max).max(1e-4);
        for b in &mut bands {
            *b = (*b / peak).clamp(0.0, 1.0);
        }
        let _ = AudioSpectrum { bands }.emit(&app);
    }
    drop(stream);
    clear();
}

fn push_mono(ring: &Mutex<VecDeque<f32>>, data: &[f32], channels: usize) {
    let mut r = ring.lock().unwrap();
    for frame in data.chunks_exact(channels) {
        r.push_back(frame.iter().sum::<f32>() / channels as f32);
    }
    // Bound the buffer even if the drain loop stalls.
    let excess = r.len().saturating_sub(FFT_SIZE * 4);
    if excess > 0 {
        r.drain(..excess);
    }
}
