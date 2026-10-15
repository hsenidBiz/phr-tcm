//! Where an area's exported specs go in the clone, and which repo account
//! stands in for each Auto Run account. One file per project beside the
//! Auto Run files: `<autorun root>/projects/<slug>.pwexport.json`. Kept on
//! this machine only.

use crate::autorun::recipe::project_slug;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where one area's specs live: the feature folder's path under `suites/`,
/// such as `sl/admin/performance/definition-wizard/competencies`.
///
/// Reads the older `{side, module, feature}` shape too (as
/// `sl/<side>/<module>/<feature>`); always writes `{path}`.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Placement {
    pub path: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Wire {
    Path { path: String },
    Old { side: String, module: String, feature: String },
}

impl<'de> Deserialize<'de> for Placement {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Wire::deserialize(d)? {
            Wire::Path { path } => Placement { path },
            Wire::Old { side, module, feature } => Placement { path: format!("sl/{side}/{module}/{feature}") },
        })
    }
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

impl Placement {
    pub fn validate(&self) -> Result<(), String> {
        if self.path.trim().is_empty() {
            return Err("The path is empty.".into());
        }
        let folders: Vec<&str> = self.path.split('/').collect();
        if folders.len() < 2 {
            return Err(format!(
                "The path \"{}\" needs at least a country and a feature folder, such as sl/admin/performance/rating-methods.",
                self.path
            ));
        }
        for f in folders {
            if !crate::pw_repo::layout::valid_name(f) {
                return Err(format!(
                    "The folder \"{f}\" must be lowercase letters, digits and hyphens, not starting or ending with a hyphen."
                ));
            }
        }
        Ok(())
    }

    /// The feature's folder path under `suites/`.
    pub fn seg(&self) -> &str {
        &self.path
    }

    /// The last folder: the feature's own name. Like `navigation_key`, this
    /// assumes a validated path (`validate`): an empty or odd one gives an
    /// empty or odd name.
    pub fn feature(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or("")
    }

    /// The key of this feature's `navigation.json` entry. Assumes a validated
    /// path, as `feature` does.
    pub fn navigation_key(&self) -> String {
        let folders: Vec<String> = self.path.split('/').map(str::to_string).collect();
        crate::pw_repo::layout::navigation_key(&folders)
    }

    /// A first guess the person confirms: the repo feature whose last folder
    /// is the area name in kebab case (the first by path order), if any.
    pub fn suggest(area_name: &str, features: &[Vec<String>]) -> Option<Placement> {
        let mut kebab = String::new();
        for c in area_name.chars() {
            if c.is_ascii_alphanumeric() {
                kebab.push(c.to_ascii_lowercase());
            } else if !kebab.is_empty() && !kebab.ends_with('-') {
                kebab.push('-');
            }
        }
        while kebab.ends_with('-') {
            kebab.pop();
        }
        if kebab.is_empty() {
            return None;
        }
        let mut found: Vec<&Vec<String>> = features.iter().filter(|f| f.last() == Some(&kebab)).collect();
        found.sort();
        found.first().map(|f| Placement { path: f.join("/") })
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
