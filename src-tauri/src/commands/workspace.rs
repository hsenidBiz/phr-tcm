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

/// Where a picked file landed, and where the copy it replaced went.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct CopiedIn {
    pub path: String,
    /// Set when a different file of the same name was already there - it
    /// now lives under `.test-cases/.history`.
    pub displaced: Option<String>,
}

/// Copy a picked JSON file into `<root>/.test-cases` (a file already there
/// is returned as is) and return the path the app should import from. A
/// different file already under that name is replaced - the pick is the
/// user's statement of intent - and the copy it displaces moves to
/// `.test-cases/.history`, never deleted.
#[tauri::command]
#[specta::specta]
pub fn copy_into_cases(root: String, source: String) -> Result<CopiedIn, String> {
    crate::workspace::copy_into_cases(Path::new(&root), Path::new(&source)).map(|(p, d)| CopiedIn {
        path: p.to_string_lossy().to_string(),
        displaced: d.map(|d| d.to_string_lossy().to_string()),
    })
}
