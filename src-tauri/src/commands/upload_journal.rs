//! Reads and clears the upload journal (`crate::upload_journal`) for the webview.

use crate::upload_journal::{self, JournalRead, UploadJournal};

#[tauri::command]
#[specta::specta]
pub fn upload_journal_read(organization: String, pbi_id: i32) -> Result<JournalRead, String> {
    Ok(match upload_journal::directory() {
        Some(dir) => upload_journal::read_in(&dir, &crate::cache::owner(), &organization, pbi_id),
        None => JournalRead::None,
    })
}

#[tauri::command]
#[specta::specta]
pub fn upload_journal_clear(organization: String, pbi_id: i32) -> Result<(), String> {
    match upload_journal::directory() {
        Some(dir) => upload_journal::clear_in(&dir, &crate::cache::owner(), &organization, pbi_id),
        None => Ok(()),
    }
}

#[tauri::command]
#[specta::specta]
pub fn upload_journal_list(organization: String) -> Result<Vec<UploadJournal>, String> {
    Ok(match upload_journal::directory() {
        Some(dir) => upload_journal::list_in(&dir, &crate::cache::owner(), &organization),
        None => Vec::new(),
    })
}
