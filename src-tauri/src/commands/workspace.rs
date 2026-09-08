//! Working-repository commands: the `.test-cases` folder and copying a
//! picked file into it. The rules live in `crate::workspace`; this is I/O
//! plumbing for the frontend.

use std::path::Path;

/// Create `<root>/.test-cases` if needed and return its path.
#[tauri::command]
#[specta::specta]
pub fn ensure_cases_dir(root: String) -> Result<String, String> {
    crate::workspace::ensure_cases_dir(Path::new(&root)).map(|p| p.to_string_lossy().to_string())
}

/// Copy a picked JSON file into `<root>/.test-cases` (a file already there
/// is returned as is) and return the path the app should import from.
#[tauri::command]
#[specta::specta]
pub fn copy_into_cases(root: String, source: String) -> Result<String, String> {
    crate::workspace::copy_into_cases(Path::new(&root), Path::new(&source))
        .map(|p| p.to_string_lossy().to_string())
}
