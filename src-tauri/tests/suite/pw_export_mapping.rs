//! Playwright export: the clone-path setting and the per-project area and
//! account mappings kept beside the Auto Run files.

use std::collections::BTreeMap;
use v2_lib::app_settings::AppSettings;
use v2_lib::pw_export::mapping::{load, save, ExportMap, Placement};

fn placement(side: &str, module: &str, feature: &str) -> Placement {
    Placement { side: side.into(), module: module.into(), feature: feature.into() }
}

#[test]
fn a_missing_map_is_empty_and_a_saved_one_reads_back() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(load(d.path(), "org", "proj").unwrap(), ExportMap::default());

    let mut map = ExportMap::default();
    map.areas.insert("Definition Wizard".into(), placement("admin", "performance", "definition-wizard"));
    let mut env = BTreeMap::new();
    env.insert("automation".to_string(), "AutomationSL.performance-management.general".to_string());
    map.accounts.insert("env-1".into(), env);

    save(d.path(), "org", "proj", &map).unwrap();
    assert_eq!(load(d.path(), "org", "proj").unwrap(), map);
}

#[test]
fn placement_rules() {
    assert!(placement("admin", "performance", "proficiency-levels").validate().is_ok());
    let side = placement("Admin", "performance", "x").validate().unwrap_err();
    assert!(side.to_lowercase().contains("side"), "{side}");
    for bad in ["foo", "user", ""] {
        let e = placement(bad, "performance", "x").validate().unwrap_err();
        assert!(e.to_lowercase().contains("side"), "{e}");
    }
    assert!(placement("self", "performance", "x").validate().is_ok());
    let dbl_m = placement("admin", "a--b", "x").validate().unwrap_err();
    assert!(dbl_m.to_lowercase().contains("module"), "{dbl_m}");
    let dbl_f = placement("admin", "performance", "a--b").validate().unwrap_err();
    assert!(dbl_f.to_lowercase().contains("feature"), "{dbl_f}");
    let module = placement("admin", "Perf Mgmt", "x").validate().unwrap_err();
    assert!(module.to_lowercase().contains("module"), "{module}");
    let feature = placement("admin", "performance", "-x").validate().unwrap_err();
    assert!(feature.to_lowercase().contains("feature"), "{feature}");
    let empty = placement("admin", "performance", "").validate().unwrap_err();
    assert!(empty.to_lowercase().contains("feature"), "{empty}");
    assert_eq!(
        placement("admin", "performance", "proficiency-levels").seg(),
        "sl/admin/performance/proficiency-levels"
    );
}

#[test]
fn suggest_kebabs_the_area_name() {
    let p = Placement::suggest("Definition Wizard");
    assert_eq!(p.feature, "definition-wizard");
    assert_eq!(p.side, "admin");
    assert_eq!(p.module, "performance");
}

#[test]
fn the_clone_path_is_a_setting() {
    assert_eq!(AppSettings::default().playwright_clone, "");
    let old = r#"{"close_to_tray":false,"beta_updates":true}"#;
    let s: AppSettings = serde_json::from_str(old).unwrap();
    assert_eq!(s.playwright_clone, "");
}
