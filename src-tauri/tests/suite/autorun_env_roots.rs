//! Auto Run's files per environment: the roots, the shared-root
//! normalisation the shared stores do, the selection of the active
//! environment, and the tripwire that keeps every `root.join(..)` in the
//! source classified as per-environment or shared.

use crate::api_templates_runner::{ORG, PAGE, PROJECT};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use v2_lib::api_templates::{store as template_store, ApiTemplate, Proven};
use v2_lib::autorun::roots::{self, CopyTo};
use v2_lib::autorun::store;
use v2_lib::autorun::test_made::{self, TestMade};
use v2_lib::environments::{self, Environment};

const A: &str = "env-0000000a";
const B: &str = "env-0000000b";

/// Leaves the process-wide layout on a throw-away flat folder, so a later
/// test never sees a path that is gone.
struct Restore;
impl Drop for Restore {
    fn drop(&mut self) {
        store::set_root(std::env::temp_dir().join("tcm-autorun-roots-restored"));
    }
}

fn env_named(id: &str, name: &str) -> Environment {
    Environment {
        id: id.into(),
        name: name.into(),
        start_url: String::new(),
        allowed_origins: vec![],
        db_id: "dev-read".into(),
        test_environment: false,
        test_prefix: "AUTOTEST".into(),
        db_auto_approve: false,
    }
}

#[test]
fn shared_of_strips_an_environment_root() {
    let t = Path::new("t");
    assert_eq!(roots::shared_of(&t.join("envs").join(A)), t);
    assert_eq!(roots::shared_of(&t.join("envs").join("_seed")), t);
    assert_eq!(roots::shared_of(t), t);
    let odd = t.join("envs").join("Not-An-Id");
    assert_eq!(roots::shared_of(&odd), odd);
    // A folder with the id's name under something that is not `envs` does not count.
    let other = t.join("other").join(A);
    assert_eq!(roots::shared_of(&other), other);
}

#[test]
fn env_id_of_reads_only_valid_folder_ids() {
    let t = Path::new("t");
    assert_eq!(roots::env_id_of(&t.join("envs").join(A)).as_deref(), Some(A));
    assert_eq!(roots::env_id_of(&t.join("envs").join("_seed")).as_deref(), Some("_seed"));
    assert_eq!(roots::env_id_of(t), None);
    assert_eq!(roots::env_id_of(&t.join("envs")), None);
    assert_eq!(roots::env_id_of(&t.join("envs").join("..").join("x")), None);
    assert_eq!(roots::env_id_of(&t.join("envs").join("ENV-1")), None);
    assert_eq!(roots::env_id_of(&t.join("envs").join("env-0000000A")), None);
}

#[test]
fn env_root_beside_stays_in_the_layout_it_was_given() {
    let t = Path::new("t");
    assert_eq!(roots::env_root_beside(&t.join("envs").join(A), B), t.join("envs").join(B));
    assert_eq!(roots::env_root_beside(t, B), t);
    // An id that is not a folder id never becomes a path.
    assert_eq!(roots::env_root_beside(&t.join("envs").join(A), "../x"), t.join("envs").join(A));
}

#[test]
fn the_pinned_lists_are_what_the_plan_says() {
    let every: Vec<&str> = roots::PER_ENV_ITEMS.iter().filter(|(_, c)| *c == CopyTo::Every).map(|(n, _)| *n).collect();
    let active_only: Vec<&str> =
        roots::PER_ENV_ITEMS.iter().filter(|(_, c)| *c == CopyTo::ActiveOnly).map(|(n, _)| *n).collect();
    assert_eq!(every, ["scripts", "deleted-scripts", "orders", "projects", "templates", "fixtures", "flows"]);
    assert_eq!(active_only, ["runs", "shots", "downloads", "reports", "replays", "explore", "captures"]);
    assert!(roots::is_export_record(Path::new("projects/acme-hr.pwexport.json")));
    assert!(!roots::is_export_record(Path::new("projects/acme-hr.nav.json")));
    assert!(!roots::is_export_record(Path::new("projects/sub/acme-hr.pwexport.json")));
    assert!(!roots::is_export_record(Path::new("scripts/x.pwexport.json")));
}

#[test]
fn flat_root_is_its_own_shared_root() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_root(t.path().to_path_buf());
    assert_eq!(store::configured_root(), Some(t.path().to_path_buf()));
    assert_eq!(store::shared_root(), Some(t.path().to_path_buf()));
    assert_eq!(store::select_environment(A), Ok(t.path().to_path_buf()));
    assert_eq!(store::configured_root(), Some(t.path().to_path_buf()));
}

#[test]
fn configured_root_follows_the_selected_environment() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    assert_eq!(store::shared_root(), Some(t.path().to_path_buf()));

    // No environment selected yet: the active one (Default, made now).
    let default_id = environments::active_id(t.path()).unwrap();
    assert_eq!(store::configured_root(), Some(roots::env_root(t.path(), &default_id)));
    assert!(roots::env_root(t.path(), &default_id).is_dir());

    let file = environments::save_env(t.path(), env_named("", "QA"), &["dev-read".to_string()]).unwrap();
    let b = file.environments.iter().find(|e| e.name == "QA").unwrap().id.clone();
    assert_eq!(store::select_environment(&b), Ok(roots::env_root(t.path(), &b)));
    assert_eq!(store::configured_root(), Some(roots::env_root(t.path(), &b)));

    // A name that is not an environment id is refused and changes nothing.
    assert!(store::select_environment("../elsewhere").is_err());
    assert_eq!(store::configured_root(), Some(roots::env_root(t.path(), &b)));

    // Shared data is found from the environment root as well.
    assert_eq!(roots::shared_of(&store::configured_root().unwrap()), t.path());
    // The file still says Default is active: re-reading it goes back there.
    let again = store::reselect_from_disk().unwrap();
    assert_eq!(again, Some(roots::env_root(t.path(), &default_id)));
}

#[test]
fn selecting_something_that_is_not_an_environment_is_refused() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    assert_eq!(store::select_environment("nope"), Err("that environment is not there any more".to_string()));
    assert!(!t.path().join("envs").join("nope").exists());
}

#[test]
fn per_environment_stores_write_under_the_environment_root() {
    let t = tempfile::tempdir().unwrap();
    let e = roots::env_root(t.path(), A);
    let inside = |p: PathBuf, what: &str| assert!(p.starts_with(&e), "{what}: {}", p.display());

    // Writes.
    let script: v2_lib::autorun::CaseScript =
        serde_json::from_value(serde_json::json!({ "case_id": 7, "title": "t", "steps": [] })).unwrap();
    store::save_script(&e, &script).unwrap();
    assert!(e.join("scripts").join("case-7.json").is_file());
    let run: v2_lib::autorun::LocalRun =
        serde_json::from_value(serde_json::json!({ "id": "run-1", "pbi_id": 1, "started_at": "1", "cases": [] }))
            .unwrap();
    store::save_run(&e, &run).unwrap();
    assert!(e.join("runs").join("run-1.json").is_file());
    store::save_order(&e, 5, &[1, 2]).unwrap();
    assert!(e.join("orders").is_dir());
    store::delete_script(&e, 7, 1_000).unwrap();
    assert!(e.join("deleted-scripts").is_dir());
    v2_lib::autorun::recipe::save_recipe(&e, ORG, PROJECT, &crate::common::recipe()).unwrap();
    assert!(std::fs::read_dir(e.join("projects")).unwrap().count() > 0);

    // Paths.
    inside(v2_lib::autorun::nav::nav_path(&e, ORG, PROJECT), "nav");
    inside(v2_lib::autorun::components::components_path(&e, ORG, PROJECT), "components");
    inside(v2_lib::autorun::quirks::quirks_path(&e, ORG, PROJECT), "quirks");
    inside(v2_lib::autorun::setups::setups_path(&e, ORG, PROJECT), "setups");
    inside(v2_lib::autorun::discovery_map::map_path(&e, ORG, PROJECT), "map");
    inside(v2_lib::autorun::divergences::path(&e, ORG, PROJECT), "divergences");
    inside(v2_lib::autorun::mapping_summary::summary_path(&e, ORG, PROJECT), "mapping");
    inside(v2_lib::pw_export::mapping::path(&e, ORG, PROJECT), "pwexport");
    inside(v2_lib::api_templates::store::templates_dir(&e, ORG, PROJECT), "templates");
    inside(v2_lib::api_templates::fixture_store::fixtures_dir(&e, ORG, PROJECT), "fixtures");
    inside(v2_lib::api_templates::flow_store::flows_dir(&e, ORG, PROJECT), "flows");
    inside(v2_lib::autorun::explore::session_path(&e, "s1"), "explore");
    inside(v2_lib::autorun::capture_tools::capture_file(&e, "c1"), "captures");
    inside(v2_lib::autorun::replay_record::dir(&e), "replays");
    inside(store::reports_dir(&e), "reports");
    inside(store::shot_path(&e, "a.png"), "shots");
    inside(store::downloads_dir(&e, "run-1"), "downloads");
}

#[test]
fn shared_stores_write_under_the_shared_root_even_when_given_an_environment_root() {
    let t = tempfile::tempdir().unwrap();
    let e = roots::env_root(t.path(), A);
    std::fs::create_dir_all(&e).unwrap();
    let shared = |p: PathBuf, what: &str| {
        assert!(p.starts_with(t.path()) && !p.starts_with(t.path().join("envs")), "{what}: {}", p.display())
    };

    let file = environments::load_or_init(&e, None).unwrap();
    assert!(t.path().join("environments.json").is_file());
    assert!(!e.join("environments.json").exists());
    assert_eq!(environments::active_id(t.path()).unwrap(), file.active);

    shared(v2_lib::autorun::accounts::accounts_path_for(&e, A), "accounts");
    shared(v2_lib::autorun::accounts::sessions_dir_for(&e, A), "sessions");
    shared(v2_lib::autorun::accounts::session_path_for(&e, A, "k"), "session");
    shared(environments::proposals_path_for(&e, A), "proposals");
    shared(v2_lib::test_files::folder(&e, ORG, PROJECT), "test files");
    shared(test_made::path(&e), "test-made");
    shared(v2_lib::allow_record::AllowRecord::path(&e), "allow record");

    // The remembered browser lives with the machine, not an environment.
    store::remember_browser(&e, "chrome");
    assert!(t.path().join("last-browser.txt").is_file());
    assert!(!e.join("last-browser.txt").exists());
    assert_eq!(store::last_browser(&e), store::last_browser(t.path()));
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Files whose `root` is not the Auto Run root (a clone, the help folder).
const NOT_AUTORUN_ROOTS: [&str; 7] = [
    "pw_repo/",
    "pw_export/clone.rs",
    "pw_export/export.rs",
    "autorun/pw_rules.rs",
    "guide.rs",
    "workspace.rs",
    "commands/guide.rs",
];

/// The string value of `const NAME: &str = "..."` in `text`.
fn const_value(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim_start();
        let l = l.strip_prefix("pub(crate) ").or_else(|| l.strip_prefix("pub ")).unwrap_or(l);
        if let Some(rest) = l.strip_prefix("const ").and_then(|r| r.strip_prefix(name)) {
            if let Some(v) = rest.split_once('=').map(|(_, v)| v.trim()) {
                return v.strip_prefix('"').and_then(|v| v.split_once('"')).map(|(s, _)| s.to_string());
            }
        }
    }
    None
}

#[test]
fn every_root_join_in_src_is_classified() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![];
    collect_rs(&src, &mut files);
    assert!(files.len() > 50, "the walk found {} files", files.len());
    let known: Vec<&str> = roots::PER_ENV_ITEMS.iter().map(|(n, _)| *n).chain(roots::SHARED_ITEMS).collect();
    let mut bad = vec![];
    let mut seen = 0;
    for file in files {
        let rel = file.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        if NOT_AUTORUN_ROOTS.iter().any(|p| rel.starts_with(p)) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (at, _) in text.match_indices("root.join(") {
            // Only a bare `root`; `x_root.join(` is something else.
            let before = text[..at].chars().next_back();
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
            let arg = &text[at + "root.join(".len()..];
            let segment = if let Some(lit) = arg.strip_prefix('"') {
                lit.split_once('"').map(|(s, _)| s.to_string())
            } else {
                let ident: String = arg.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
                if ident.is_empty() || ident.chars().any(|c| c.is_ascii_lowercase()) {
                    continue; // a variable, not a constant
                }
                const_value(&text, &ident)
            };
            let Some(segment) = segment else { continue };
            seen += 1;
            let first = segment.split('/').next().unwrap_or("").to_string();
            if !known.contains(&first.as_str()) {
                bad.push(format!("{rel}: root.join(\"{segment}\")"));
            }
        }
    }
    assert!(seen > 20, "the scan saw only {seen} joins; the tripwire is not looking");
    assert!(
        bad.is_empty(),
        "a path under the Auto Run root is in neither roots::PER_ENV_ITEMS nor roots::SHARED_ITEMS:\n{}",
        bad.join("\n")
    );
}

#[test]
fn cleanup_preview_in_another_environment_uses_its_templates() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let shared = t.path();
    let file = environments::load_or_init(shared, None).unwrap();
    let a = file.active.clone();
    let b = environments::save_env(shared, env_named("", "Staging"), &["dev-read".to_string()])
        .unwrap()
        .environments
        .into_iter()
        .find(|e| e.name == "Staging")
        .unwrap();
    let (a_root, b_root) = (roots::env_root(shared, &a), roots::env_root(shared, &b.id));
    std::fs::create_dir_all(&a_root).unwrap();
    std::fs::create_dir_all(&b_root).unwrap();

    // A proven delete template saved only in Staging's folder.
    let mut template: ApiTemplate = serde_json::from_value(serde_json::json!({
        "id": "remove-cycle", "title": "Delete a draft", "module": "PMS", "effect": "delete",
        "description": "d", "sources": ["x:1"], "antiforgery": { "page": PAGE },
        "params": [ { "name": "id", "type": "string", "required": true } ],
        "steps": [ { "name": "Delete", "method": "POST", "path": "/hr/pmsv10/performancecycle",
                     "query": { "handler": "DeleteCycle" }, "form": { "Id": "{{id}}" } } ],
        "outputs": [], "deletes_kind": "cycle"
    }))
    .unwrap();
    template.proven = Some(Proven {
        at: "2026-10-01 09:00:00".into(),
        origin: "https://hr.example.internal".into(),
        account: "admin".into(),
        outputs: BTreeMap::new(),
        environment: Some(b.name.clone()),
        via: None,
    });
    template_store::save(&b_root, ORG, PROJECT, &template).unwrap();

    let made = TestMade {
        environment: b.id.clone(),
        kind: "cycle".into(),
        id: "1".into(),
        name: "AUTOTEST cycle".into(),
        created_at: "2026-09-01T00:00:00Z".into(),
        fixture: "draft-cycle".into(),
        run_id: "run-1".into(),
        case_id: None,
        status: "present".into(),
    };
    std::fs::write(shared.join("test-made.json"), serde_json::to_string(&[made]).unwrap()).unwrap();

    let query = v2_lib::autorun::cleanup::CleanupQuery {
        environment: b.id.clone(),
        prefix: "AUTOTEST".into(),
        older_than_days: 7,
    };
    let now = "2026-10-06T12:00:00Z".parse().unwrap();
    // The active environment's root is A's; Staging's templates still count.
    let lines = v2_lib::autorun::cleanup::preview(&a_root, ORG, PROJECT, &query, now).unwrap();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].deletable, "{lines:?}");
}
