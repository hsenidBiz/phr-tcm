//! Screen capture for the test runner (xcap, multi-monitor - the v1
//! region-select overlay is deferred; the runner captures whole monitors
//! and the user picks which shots to attach).

use base64::Engine;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ScreenShot {
    pub name: String,
    /// PNG, base64 (no data: prefix) - the shape add_result_attachment wants.
    pub b64_png: String,
}

pub fn capture_all_monitors() -> Result<Vec<ScreenShot>, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let mut shots = vec![];
    for (i, m) in monitors.iter().enumerate() {
        let name = m
            .name()
            .ok()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("Monitor {}", i + 1));
        let img = m.capture_image().map_err(|e| e.to_string())?;
        let mut png: Vec<u8> = vec![];
        img.write_to(
            &mut std::io::Cursor::new(&mut png),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
        shots.push(ScreenShot {
            name,
            b64_png: base64::engine::general_purpose::STANDARD.encode(&png),
        });
    }
    Ok(shots)
}
