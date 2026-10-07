//! Where an area's exported specs go in the clone, and which repo account
//! stands in for each Auto Run account. One file per project beside the
//! Auto Run files: `<autorun root>/projects/<slug>.pwexport.json`. Kept on
//! this machine only.

use crate::autorun::recipe::project_slug;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where one area's specs live: `sl/<side>/<module>/<feature>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Placement {
    pub side: String,
    pub module: String,
    pub feature: String,
}

/// A project's export choices. `accounts[env_id][tcm_account_key]` is the
/// clone's user key.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, specta::Type)]
pub struct ExportMap {
    #[serde(default)]
    pub areas: BTreeMap<String, Placement>,
    #[serde(default)]
    pub accounts: BTreeMap<String, BTreeMap<String, String>>,
}

fn is_kebab(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.ends_with('-')
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn check(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("The {label} is empty."));
    }
    if !is_kebab(value) {
        return Err(format!(
            "The {label} \"{value}\" must be lowercase letters, digits and hyphens, not starting or ending with a hyphen."
        ));
    }
    Ok(())
}

impl Placement {
    pub fn validate(&self) -> Result<(), String> {
        check("side", &self.side)?;
        check("module", &self.module)?;
        check("feature", &self.feature)
    }

    pub fn seg(&self) -> String {
        format!("sl/{}/{}/{}", self.side, self.module, self.feature)
    }

    /// A first guess the person confirms.
    pub fn suggest(area_name: &str) -> Placement {
        let mut feature = String::new();
        for c in area_name.chars() {
            if c.is_ascii_alphanumeric() {
                feature.push(c.to_ascii_lowercase());
            } else if !feature.is_empty() && !feature.ends_with('-') {
                feature.push('-');
            }
        }
        while feature.ends_with('-') {
            feature.pop();
        }
        Placement { side: "admin".into(), module: "performance".into(), feature }
    }
}

pub fn path(root: &Path, org: &str, project: &str) -> PathBuf {
    root.join("projects").join(format!("{}.pwexport.json", project_slug(org, project)))
}

pub fn load(root: &Path, org: &str, project: &str) -> Result<ExportMap, String> {
    let p = path(root, org, project);
    match std::fs::read_to_string(&p) {
        Ok(s) => {
            let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
            serde_json::from_str(s).map_err(|e| format!("could not read {}: {e}", p.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ExportMap::default()),
        Err(e) => Err(format!("could not read {}: {e}", p.display())),
    }
}

pub fn save(root: &Path, org: &str, project: &str, map: &ExportMap) -> Result<(), String> {
    let p = path(root, org, project);
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    }
    let body = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    crate::ai_tools::atomic_write(&p, &body)
}
