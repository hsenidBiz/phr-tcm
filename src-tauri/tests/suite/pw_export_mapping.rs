//! Playwright export: the clone-path setting and the per-project area and
//! account mappings kept beside the Auto Run files.

use std::collections::BTreeMap;
use v2_lib::app_settings::AppSettings;
use v2_lib::pw_export::mapping::{load, save, ExportMap, Placement};

fn placement(path: &str) -> Placement {
    Placement { path: path.into() }
}

#[test]
fn a_missing_map_is_empty_and_a_saved_one_reads_back() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(load(d.path(), "org", "proj").unwrap(), ExportMap::default());

    let mut map = ExportMap::default();
    map.areas.insert("Definition Wizard".into(), placement("sl/admin/performance/definition-wizard"));
    let mut env = BTreeMap::new();
    env.insert("automation".to_string(), "AutomationSL.performance-management.general".to_string());
    map.accounts.insert("env-1".into(), env);

    save(d.path(), "org", "proj", &map).unwrap();
    assert_eq!(load(d.path(), "org", "proj").unwrap(), map);
}

#[test]
fn an_old_placement_reads_as_its_path() {
    for (side, want) in [("self", "sl/self/m/f"), ("admin", "sl/admin/m/f")] {
        let old = format!(r#"{{"side":"{side}","module":"m","feature":"f"}}"#);
        let p: Placement = serde_json::from_str(&old).unwrap();
        assert_eq!(p, placement(want));
        assert_eq!(serde_json::to_string(&p).unwrap(), format!(r#"{{"path":"{want}"}}"#));
    }
    let p: Placement = serde_json::from_str(r#"{"path":"sl/admin/x/y/z"}"#).unwrap();
    assert_eq!(p.path, "sl/admin/x/y/z");
    // An old map file on disk reads too, and is written back as paths.
    let d = tempfile::tempdir().unwrap();
    let file = v2_lib::pw_export::mapping::path(d.path(), "org", "proj");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, r#"{"areas":{"A":{"side":"admin","module":"performance","feature":"a"}}}"#).unwrap();
    let map = load(d.path(), "org", "proj").unwrap();
    assert_eq!(map.areas["A"].path, "sl/admin/performance/a");
    save(d.path(), "org", "proj", &map).unwrap();
    assert!(std::fs::read_to_string(&file).unwrap().contains("\"path\": \"sl/admin/performance/a\""));
}

#[test]
fn a_typed_path_is_trimmed_or_refused() {
    assert!(placement("sl/admin/performance/definition-wizard/competencies").validate().is_ok());
    assert!(placement("sl/self/x").validate().is_ok());
    assert_eq!(placement("").validate().unwrap_err(), "The path is empty.");
    assert_eq!(
        placement("sl").validate().unwrap_err(),
        "The path \"sl\" needs at least a country and a feature folder, such as sl/admin/performance/rating-methods."
    );
    // Any folder name the repo itself could hold.
    for ok in ["Sl/admin/x", "sl/admin/performance/definition-wizard/DW-competencies", "sl/admin/a--b", "sl/My Module/x_y"] {
        assert!(placement(ok).validate().is_ok(), "{ok}");
    }
    assert_eq!(placement("sl//x").validate().unwrap_err(), "The path \"sl//x\" cannot be used: a folder name is empty.");
    assert_eq!(
        placement("sl/_generated/x").validate().unwrap_err(),
        "The path \"sl/_generated/x\" cannot be used: \"_generated\" starts with _ or . (or is node_modules), so the repo's tree never reads it."
    );
    for bad in ["sl/../x", "sl/a:b", "sl/con", "sl/x.", "sl/a`b"] {
        assert!(placement(bad).validate().is_err(), "{bad}");
    }
    let p = placement("sl/admin/performance/definition-wizard/competencies");
    assert_eq!(p.seg(), "sl/admin/performance/definition-wizard/competencies");
    assert_eq!(p.feature(), "competencies");
    assert_eq!(p.navigation_key(), "definition-wizard/competencies");
}

#[test]
fn suggest_finds_the_feature_named_like_the_area() {
    let features: Vec<Vec<String>> = [
        "sl/admin/performance/definition-wizard/competencies",
        "sl/admin/performance/competencies",
        "sl/admin/performance/rating-methods",
    ]
    .iter()
    .map(|p| p.split('/').map(String::from).collect())
    .collect();
    // The first by path order when several match.
    assert_eq!(
        Placement::suggest("Competencies", &features),
        Some(placement("sl/admin/performance/competencies"))
    );
    assert_eq!(
        Placement::suggest("Rating  Methods!", &features),
        Some(placement("sl/admin/performance/rating-methods"))
    );
    assert_eq!(Placement::suggest("Nothing", &features), None);
}

#[test]
fn the_clone_path_is_a_setting() {
    assert_eq!(AppSettings::default().playwright_clone, "");
    let old = r#"{"close_to_tray":false,"beta_updates":true}"#;
    let s: AppSettings = serde_json::from_str(old).unwrap();
    assert_eq!(s.playwright_clone, "");
}

#[test]
fn the_component_map_round_trips() {
    let d = tempfile::tempdir().unwrap();
    let mut map = ExportMap::default();
    map.components.insert("Employee search".into(), "employee-search".into());
    map.components.insert("Pick a date".into(), "date-picker".into());
    save(d.path(), "org", "proj", &map).unwrap();
    assert_eq!(load(d.path(), "org", "proj").unwrap(), map);

    // A map saved before components existed reads as having none.
    let file = v2_lib::pw_export::mapping::path(d.path(), "org", "proj");
    std::fs::write(&file, r#"{"areas":{"A":{"path":"sl/admin/m/a"}}}"#).unwrap();
    let old = load(d.path(), "org", "proj").unwrap();
    assert!(old.components.is_empty());
    assert_eq!(old.areas["A"].path, "sl/admin/m/a");
}
