//! The AI Bridge tab's Writing style card (`crate::writing_style`).

use crate::writing_style::{self, WritingStyle};

/// The saved style. The starting one is created on the first read when
/// this machine has none yet.
#[tauri::command]
#[specta::specta]
pub fn writing_style_get() -> WritingStyle {
    match writing_style::dir() {
        Some(dir) => writing_style::get_or_create(&dir),
        None => WritingStyle::starting(),
    }
}

/// Save the style. A refusal (too large, empty while on) is the sentence
/// the person reads; a disk failure is logged and reported without a path.
#[tauri::command]
#[specta::specta]
pub fn writing_style_save(style: WritingStyle) -> Result<(), String> {
    if let Some(why) = writing_style::refusal(&style) {
        return Err(why.to_string());
    }
    let dir = writing_style::dir().ok_or_else(|| "the writing style is not available yet".to_string())?;
    writing_style::save(&dir, &style).map_err(|e| {
        crate::applog::warn(format!("saving the writing style failed: {e}"));
        "The writing style could not be saved. Settings → Logs has the details.".to_string()
    })
}

/// The text of a `.md` or `.markdown` file the person picked with Upload
/// .md, for the editor. Nothing is saved until they press Save.
#[tauri::command]
#[specta::specta]
pub fn writing_style_read_file(path: String) -> Result<String, String> {
    writing_style::read_markdown(std::path::Path::new(&path))
}
