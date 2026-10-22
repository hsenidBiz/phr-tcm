//! The page area every browser the app drives shows.
//!
//! Every page the app attaches to is laid out 1280 x 720 at a device scale
//! factor of 1 - Playwright's own default - so a layout seen while a script
//! is written is the layout a run sees, on any screen and at any Windows
//! display scale. The numbers live here and nowhere else.

/// The page's width, in CSS pixels.
pub const PAGE_WIDTH: u32 = 1280;
/// The page's height, in CSS pixels.
pub const PAGE_HEIGHT: u32 = 720;
/// One device pixel per CSS pixel, whatever the screen's own scale.
pub const DEVICE_SCALE: f64 = 1.0;

/// What a window needs on top of the page for the browser's own frame
/// (borders) and its tab strip and toolbar, so a headed window holds the
/// whole page without scrolling it.
const FRAME_WIDTH: u32 = 16;
const FRAME_HEIGHT: u32 = 140;

/// `--window-size=` for a window that holds a `PAGE_WIDTH` x `PAGE_HEIGHT`
/// page with the browser's frame around it.
pub fn window_size_arg() -> &'static str {
    static ARG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ARG.get_or_init(|| format!("--window-size={},{}", PAGE_WIDTH + FRAME_WIDTH, PAGE_HEIGHT + FRAME_HEIGHT))
}

/// The parameters of `Emulation.setDeviceMetricsOverride`, sent on every
/// page the app attaches to before the page is used. The override wins over
/// the window: a person who resizes a headed window keeps the same page.
pub fn device_metrics_params() -> serde_json::Value {
    serde_json::json!({
        "width": PAGE_WIDTH,
        "height": PAGE_HEIGHT,
        "deviceScaleFactor": DEVICE_SCALE,
        "mobile": false,
    })
}

/// The command `device_metrics_params` goes with.
pub const SET_DEVICE_METRICS: &str = "Emulation.setDeviceMetricsOverride";
