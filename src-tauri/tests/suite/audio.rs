use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use v2_lib::audio::{analyze, release_if_owner, Capture, BAND_COUNT};

/// The band edges are log-spaced 40 Hz..16 kHz; find which band a
/// frequency falls into (mirror of the mapping in analyze()).
fn band_of(freq: f32, n_bands: usize) -> usize {
    let ratio: f32 = 16_000.0 / 40.0;
    let pos = (freq / 40.0).ln() / ratio.ln();
    ((pos * n_bands as f32) as usize).min(n_bands - 1)
}

#[test]
fn pure_tone_peaks_in_its_band() {
    let sr = 48_000.0;
    let samples: Vec<f32> = (0..2048)
        .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr).sin())
        .collect();
    let bands = analyze(&samples, sr, BAND_COUNT);
    assert_eq!(bands.len(), BAND_COUNT);
    let loudest = bands
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    let expected = band_of(1000.0, BAND_COUNT);
    assert!(
        loudest.abs_diff(expected) <= 1,
        "1 kHz tone peaked in band {loudest}, expected ~{expected}: {bands:?}"
    );
}

#[test]
fn silence_and_empty_input_are_flat_zero() {
    assert!(analyze(&[], 48_000.0, BAND_COUNT).iter().all(|b| *b == 0.0));
    let silence = vec![0.0f32; 2048];
    assert!(analyze(&silence, 48_000.0, BAND_COUNT).iter().all(|b| *b == 0.0));
}

/// A stop immediately followed by a start: the old capture thread is still
/// winding down when the new one takes the slot. When the old one exits it
/// must leave the new one's slot alone - clearing it orphans a thread that
/// nothing can stop.
#[test]
fn an_exiting_capture_frees_only_its_own_slot() {
    let old = Arc::new(AtomicBool::new(true));
    let new = Arc::new(AtomicBool::new(false));
    let slot = Mutex::new(Some(Capture { stop: new.clone() }));

    assert!(!release_if_owner(&slot, &old), "not the old thread's slot any more");
    assert!(slot.lock().unwrap().is_some());

    assert!(release_if_owner(&slot, &new));
    assert!(slot.lock().unwrap().is_none());
    assert!(!release_if_owner(&slot, &new), "an empty slot has no owner");
}
