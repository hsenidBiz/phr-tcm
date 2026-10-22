//! The page area every browser the app drives shows.
//!
//! Every page the app attaches to is laid out at one size at a device scale
//! factor of 1, so a layout seen while a script is written is the layout a
//! run sees, on any screen and at any Windows display scale. The size is a
//! setting (Settings, Auto Run, "Browser page size") that ships as 1280 x 720,
//! Playwright's own default. The scale is not a setting.

use serde::{Deserialize, Serialize};

/// The default page's width, in CSS pixels.
pub const PAGE_WIDTH: u32 = 1280;
/// The default page's height, in CSS pixels.
pub const PAGE_HEIGHT: u32 = 720;
/// One device pixel per CSS pixel, whatever the screen's own scale.
pub const DEVICE_SCALE: f64 = 1.0;

/// A page area, in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct PageSize {
    pub width: u32,
    pub height: u32,
}

/// What the app ships with, and what an unset or unreadable value reads as.
pub const DEFAULT_PAGE: PageSize = PageSize { width: PAGE_WIDTH, height: PAGE_HEIGHT };
/// The smallest page a person may ask for.
pub const MIN_PAGE: PageSize = PageSize { width: 800, height: 600 };
/// The largest page a person may ask for.
pub const MAX_PAGE: PageSize = PageSize { width: 3840, height: 2160 };

/// The sentence a size outside `MIN_PAGE..=MAX_PAGE` is refused with.
pub const OUT_OF_RANGE: &str = "The page size must be between 800 x 600 and 3840 x 2160.";

impl PageSize {
    /// Whether both sides lie within `MIN_PAGE..=MAX_PAGE`.
    pub fn in_range(self) -> bool {
        (MIN_PAGE.width..=MAX_PAGE.width).contains(&self.width) && (MIN_PAGE.height..=MAX_PAGE.height).contains(&self.height)
    }
}

/// `size` if it is allowed, otherwise the sentence that says what is.
pub fn validate(size: PageSize) -> Result<PageSize, String> {
    if size.in_range() {
        Ok(size)
    } else {
        Err(OUT_OF_RANGE.to_string())
    }
}

/// The page size in force: the saved setting, loaded at start-up and
/// replaced the moment a new one is saved (`crate::app_settings`), or the
/// default before that. A browser already open keeps the size it was given;
/// the next one the app opens reads this.
pub fn current() -> PageSize {
    crate::app_settings::current().page_size
}

/// How the size reads in a sentence: "1280 x 720".
pub fn describe(size: PageSize) -> String {
    format!("{} x {}", size.width, size.height)
}

/// The guides say what the page size is in one fixed sentence: "... shows a
/// 1280 x 720 page at scale 1, the same as Playwright's default, so ...".
/// This returns `text` with that clause saying the size in force instead,
/// and, when the person has changed it, that it is theirs and not
/// Playwright's. At the default it returns `text` unchanged.
pub fn size_in_force(text: &str) -> String {
    let now = current();
    if now == DEFAULT_PAGE {
        return text.to_string();
    }
    text.replace(
        "1280 x 720 page at scale 1, the same as\nPlaywright's default",
        &format!(
            "{} page at scale 1, a size set in Settings rather than Playwright's default of {}",
            describe(now),
            describe(DEFAULT_PAGE)
        ),
    )
}

/// What a window needs on top of the page for the browser's own frame
/// (borders) and its tab strip and toolbar, so a headed window holds the
/// whole page without scrolling it.
const FRAME_WIDTH: u32 = 16;
const FRAME_HEIGHT: u32 = 140;

/// `--window-size=` for a window that holds a `current()` page with the
/// browser's frame around it.
pub fn window_size_arg() -> String {
    let size = current();
    format!("--window-size={},{}", size.width + FRAME_WIDTH, size.height + FRAME_HEIGHT)
}

/// The parameters of `Emulation.setDeviceMetricsOverride` for the size in
/// force now.
pub fn device_metrics_params() -> serde_json::Value {
    device_metrics_for(current())
}

/// The parameters of `Emulation.setDeviceMetricsOverride` for `size`, sent on
/// every page the app attaches to before the page is used. The override wins
/// over the window: a person who resizes a headed window keeps the same page.
/// A browser holds the size it opened with and gives it to its own tabs.
pub fn device_metrics_for(size: PageSize) -> serde_json::Value {
    serde_json::json!({
        "width": size.width,
        "height": size.height,
        "deviceScaleFactor": DEVICE_SCALE,
        "mobile": false,
    })
}

/// The command `device_metrics_params` goes with.
pub const SET_DEVICE_METRICS: &str = "Emulation.setDeviceMetricsOverride";
