//! The Playwright tab: the team's Playwright repo, read from its clone
//! folder. Offered exactly where Auto Run is (`ai_tools::autorun_offered`),
//! and refused everywhere else. The clone folder itself is the app setting
//! Export to Playwright uses (`set_playwright_clone`).

use crate::pw_repo::structure::{self, RepoStructure};

/// What the clone at `clone` holds, and the health line. Read only.
#[tauri::command]
#[specta::specta]
pub fn pw_repo_structure(clone: String) -> Result<RepoStructure, String> {
    structure::structure_for(crate::ai_tools::autorun_offered(), &clone, structure::node_found())
}
