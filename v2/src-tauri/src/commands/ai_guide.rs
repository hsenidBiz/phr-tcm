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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai_guide::GuideFlavor;

    fn opts(flavors: Vec<GuideFlavor>) -> GuideOptions {
        GuideOptions {
            organization: "acme".into(),
            project: "Web".into(),
            area: None,
            modules: vec!["Login".into()],
            tags: vec![],
            modules_discovered: true,
            doc_paths: vec![],
            conventions: String::new(),
            flavors,
            generated_on: "2026-07-20".into(),
        }
    }

    #[test]
    fn write_creates_nested_flavor_files() {
        let dir = std::env::temp_dir().join(format!("tcm_ai_guide_write_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let written = write_ai_guide(
            dir.to_str().unwrap().into(),
            opts(vec![GuideFlavor::Generic, GuideFlavor::ClaudeSkill]),
        )
        .unwrap();

        assert_eq!(
            written,
            vec!["AI_TEST_CASES.md", ".claude/skills/generate-test-cases/SKILL.md"]
        );
        let skill = std::fs::read_to_string(
            dir.join(".claude/skills/generate-test-cases/SKILL.md"),
        )
        .unwrap();
        assert!(skill.contains("acme/Web"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_with_no_flavors_errors() {
        let dir = std::env::temp_dir().join(format!("tcm_ai_guide_no_flavors_{}", std::process::id()));
        assert!(write_ai_guide(dir.to_str().unwrap().into(), opts(vec![])).is_err());
    }

    #[test]
    fn preview_returns_the_generic_body() {
        assert!(preview_ai_guide(opts(vec![])).contains("# AI guide"));
    }
}
