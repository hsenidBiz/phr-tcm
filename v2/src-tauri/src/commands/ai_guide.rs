//! AI-guide generation commands. Pure/local-file-write only - no ADO calls,
//! no DELETE surface. Discovery values arrive from the frontend's existing
//! queries; these commands never touch the network.

use crate::ai_guide::{build_guide_body, flavor_files, GuideOptions};

#[tauri::command]
#[specta::specta]
pub fn preview_ai_guide(options: GuideOptions) -> String {
    build_guide_body(&options)
}

#[tauri::command]
#[specta::specta]
pub fn write_ai_guide(dir: String, options: GuideOptions) -> Result<Vec<String>, String> {
    let body = build_guide_body(&options);
    let files = flavor_files(&body, &options.flavors);
    if files.is_empty() {
        return Err("No output flavor selected.".into());
    }
    let base = std::path::Path::new(&dir);
    let mut written = Vec::with_capacity(files.len());
    for (rel, content) in files {
        let target = base.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
        }
        std::fs::write(&target, content)
            .map_err(|e| format!("Could not write {}: {e}", target.display()))?;
        written.push(rel);
    }
    Ok(written)
}
