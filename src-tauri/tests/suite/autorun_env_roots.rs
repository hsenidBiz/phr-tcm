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
    // Not the selected environment: writes into it are allowed on purpose.
    let _allow = roots::allow_writes_to(&e);
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

/// The string value of `const NAME: &str = "..."` in `text`. The name must
/// match exactly: the next character is `:` or a space, never more name.
fn const_value(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let l = line.trim_start();
        let l = l.strip_prefix("pub(crate) ").or_else(|| l.strip_prefix("pub ")).unwrap_or(l);
        let Some(rest) = l.strip_prefix("const ").and_then(|r| r.strip_prefix(name)) else { continue };
        if !rest.starts_with(':') && !rest.starts_with(' ') {
            continue;
        }
        if let Some(v) = rest.split_once('=').map(|(_, v)| v.trim()) {
            return v.strip_prefix('"').and_then(|v| v.split_once('"')).map(|(s, _)| s.to_string());
        }
    }
    None
}

/// The first path segment a `.join(` argument names, when it can be read:
/// a string literal, a `format!` whose first argument is one (up to its
/// first `{`), or a `CONST` of this file or any other file (`a::B::CONST`).
/// `Ok(None)` is a variable or a call, which cannot be read here; `Err` is
/// a constant that no file defines, which fails the test.
fn first_segment(arg: &str, text: &str, all: &[(String, String)]) -> Result<Option<String>, String> {
    let literal = |s: &str| {
        let s = s.split('{').next().unwrap_or("");
        s.split('/').next().unwrap_or("").to_string()
    };
    if let Some(lit) = arg.strip_prefix('"') {
        return Ok(lit.split_once('"').map(|(s, _)| literal(s)));
    }
    if let Some(inner) = arg.strip_prefix("format!(\"") {
        return Ok(inner.split_once('"').map(|(s, _)| literal(s)));
    }
    let path: String = arg.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == ':').collect();
    let name = path.rsplit("::").next().unwrap_or("");
    if name.is_empty() || name.chars().any(|c| c.is_ascii_lowercase()) {
        return Ok(None);
    }
    let found = const_value(text, name).or_else(|| all.iter().find_map(|(_, t)| const_value(t, name)));
    found.map(|v| Some(literal(&v))).ok_or_else(|| format!("the constant {name} is not defined as a &str here"))
}

/// The expression just before a `.join(`: what it is called on.
fn receiver(before: &str) -> String {
    let line = before.rsplit('\n').next().unwrap_or("");
    // `shared_of(root)` ends in `)`; take it with the call's name.
    if line.ends_with(')') {
        let open = line.rfind("shared_of(").map(|i| &line[i..]).unwrap_or("(call)");
        return open.to_string();
    }
    line.chars().rev().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect::<Vec<_>>().into_iter().rev().collect()
}

/// A `root.join(..)` in a file other than a test, scanned for where the
/// name it joins belongs: a name in neither pinned list is unclassified, a
/// SHARED name joined on a bare `root` would split shared data per
/// environment, and a per-environment name joined on the shared root would
/// put one environment's data where all of them read it.
///
/// What it cannot see (so a reviewer still must): a path built by a
/// variable or a call (`root.join(name)`, `root.join(x.path())`), a
/// receiver renamed to something else for an unlisted name (only `root` is
/// held to the unclassified rule; `dir`, `base` and the like are everywhere
/// for unrelated folders), and a string pushed with `PathBuf::push`.
#[test]
fn every_root_join_in_src_is_classified() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = vec![];
    collect_rs(&src, &mut paths);
    assert!(paths.len() > 50, "the walk found {} files", paths.len());
    let all: Vec<(String, String)> = paths
        .iter()
        .map(|p| {
            let rel = p.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
            (rel, std::fs::read_to_string(p).unwrap())
        })
        .collect();
    let per_env: Vec<&str> = roots::PER_ENV_ITEMS.iter().map(|(n, _)| *n).collect();
    let shared_names: Vec<&str> = roots::SHARED_ITEMS.to_vec();
    // Where a shared name is joined on a bare `root` on purpose: backup.rs
    // works on the shared root already, and remove_legacy_files re-binds
    // `root` to `shared_of(root)` on its first line.
    let shared_on_root_ok = |rel: &str| rel == "backup.rs" || rel == "environments.rs";
    let mut bad = vec![];
    let mut seen = 0;
    for (rel, text) in &all {
        if NOT_AUTORUN_ROOTS.iter().any(|p| rel.starts_with(p)) {
            continue;
        }
        for (at, _) in text.match_indices(".join(") {
            let who = receiver(&text[..at]);
            let arg = &text[at + ".join(".len()..];
            let segment = match first_segment(arg, text, &all) {
                Ok(Some(s)) => s,
                Ok(None) => continue,
                Err(e) => {
                    bad.push(format!("{rel}: {e}"));
                    continue;
                }
            };
            let is_shared_name = shared_names.contains(&segment.as_str());
            let is_per_env_name = per_env.contains(&segment.as_str());
            let on_shared = who.starts_with("shared_of(") || who == "shared" || who == "shared_root";
            if who == "root" {
                seen += 1;
                if !is_shared_name && !is_per_env_name {
                    bad.push(format!("{rel}: root.join(\"{segment}\") is in neither pinned list"));
                } else if is_shared_name && !shared_on_root_ok(rel) {
                    bad.push(format!("{rel}: shared \"{segment}\" joined on a bare `root` (use shared_of(root))"));
                }
            } else if on_shared && is_per_env_name {
                bad.push(format!("{rel}: per-environment \"{segment}\" joined on the shared root"));
            }
        }
    }
    assert!(seen > 20, "the scan saw only {seen} joins on `root`; the tripwire is not looking");
    assert!(bad.is_empty(), "the Auto Run layout tripwire found:\n{}", bad.join("\n"));
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
    let _allow = roots::allow_writes_to(&b_root);
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

#[test]
fn a_fresh_install_has_no_root_until_default_is_made_with_the_chosen_database() {
    use v2_lib::commands::environments::list_view;
    use v2_lib::db::credentials::MemoryStore;
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());

    // Resolving the root never makes an environment.
    assert_eq!(store::resolve_root(), Err(store::NO_ENVIRONMENT_YET.to_string()));
    assert_eq!(store::configured_root(), None);
    assert!(!t.path().join("environments.json").exists());

    // The list is what makes Default, with the database the person chose.
    let view = list_view(t.path(), &MemoryStore::default(), Some("qa-read")).unwrap();
    assert_eq!(view.environments[0].db_id, "qa-read");
    // The first read of the root after that selects it; the list does not.
    assert_eq!(store::configured_root(), Some(roots::env_root(t.path(), &view.active)));
}

#[test]
fn a_failed_switch_leaves_the_file_the_root_and_the_bridge_on_the_old_environment() {
    use v2_lib::ai_bridge::BridgeContext;
    use v2_lib::commands::environments::move_bridge_to;
    use v2_lib::db::credentials::MemoryStore;
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    let a = environments::load_or_init(t.path(), Some("dev-read")).unwrap().active;
    let b = environments::save_env(t.path(), env_named("", "QA"), &["dev-read".to_string()])
        .unwrap()
        .environments
        .into_iter()
        .find(|e| e.name == "QA")
        .unwrap()
        .id;
    store::select_environment(&a).unwrap();
    let ctx = std::sync::Mutex::new(BridgeContext::default());
    ctx.lock().unwrap().gate.env_id = Some(a.clone());

    // Something that is a file where B's folder has to be: the create fails.
    std::fs::create_dir_all(t.path().join("envs")).unwrap();
    std::fs::write(roots::env_root(t.path(), &b), "in the way").unwrap();

    let err = move_bridge_to(t.path(), &MemoryStore::default(), &b, Some(&ctx)).unwrap_err();
    assert!(err.contains("folder could not be made"), "{err}");
    assert_eq!(environments::active_id(t.path()).unwrap(), a, "the file went back");
    assert_eq!(store::configured_root(), Some(roots::env_root(t.path(), &a)));
    assert_eq!(ctx.lock().unwrap().gate.env_id.as_deref(), Some(a.as_str()), "the bridge never moved");

    // And with the way clear, the same switch lands everywhere.
    std::fs::remove_file(roots::env_root(t.path(), &b)).unwrap();
    move_bridge_to(t.path(), &MemoryStore::default(), &b, Some(&ctx)).unwrap();
    assert_eq!(environments::active_id(t.path()).unwrap(), b);
    assert_eq!(store::configured_root(), Some(roots::env_root(t.path(), &b)));
    assert_eq!(ctx.lock().unwrap().gate.env_id.as_deref(), Some(b.as_str()));
}

#[test]
fn an_unreadable_environment_list_says_so_and_is_logged_without_a_path() {
    let _log = crate::serial::log_tail();
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("environments.json"), "{ half a file").unwrap();
    store::set_shared_root(t.path().to_path_buf());

    let said = store::resolve_root().unwrap_err();
    assert_eq!(said, store::ENVIRONMENTS_UNREADABLE);
    assert!(said.contains("environments") && said.contains("Logs"), "{said}");
    assert!(!said.contains("restart"), "restarting changes nothing: {said}");
    assert!(!said.contains(t.path().to_str().unwrap()), "{said}");
    let line = v2_lib::applog::recent(50)
        .into_iter()
        .rev()
        .find(|l| l.message.contains("the environments could not be read"))
        .expect("the reason is in the log");
    assert_eq!(line.level, "warn");
    assert!(!line.message.contains(t.path().to_str().unwrap()), "{}", line.message);
    // The broken file is left exactly as it was.
    assert_eq!(std::fs::read_to_string(t.path().join("environments.json")).unwrap(), "{ half a file");
}

#[test]
fn the_seed_is_not_an_environment_that_can_be_selected() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    assert!(store::select_environment("_seed").is_err());
    assert!(!roots::env_root(t.path(), "_seed").exists());
}

#[test]
fn the_first_lookup_does_not_undo_a_switch_that_got_there_first() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    // The file says A is active; B is selected before anything has looked.
    let a = environments::load_or_init(t.path(), Some("dev-read")).unwrap().active;
    assert_ne!(a, B);
    store::select_environment(B).unwrap();
    assert_eq!(store::resolve_root(), Ok(roots::env_root(t.path(), B)));
}

#[test]
fn shared_stores_do_not_make_an_environment_folder() {
    let t = tempfile::tempdir().unwrap();
    let gone = roots::env_root(t.path(), A);
    // A stale environment root (the folder was removed): shared writes go to the shared root only.
    environments::load_or_init(&gone, None).unwrap();
    assert!(!gone.exists(), "a shared write recreated an environment's folder");
}

#[test]
fn the_assistant_is_told_the_real_reason_there_is_no_root() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    let (status, said) = v2_lib::ai_bridge::autorun_root().unwrap_err();
    assert_eq!(status, 503);
    assert_eq!(said, store::NO_ENVIRONMENT_YET);
    assert!(!said.contains("restart") && !said.contains("Settings"), "{said}");

    std::fs::write(t.path().join("environments.json"), "[ a different break, never the one the log test uses").unwrap();
    let (_, said) = v2_lib::ai_bridge::autorun_root().unwrap_err();
    assert_eq!(said, store::ENVIRONMENTS_UNREADABLE);
}

#[test]
fn a_list_never_moves_the_root_back() {
    use v2_lib::commands::environments::{env_list_with, move_bridge_to};
    use v2_lib::db::credentials::MemoryStore;
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    let secrets = MemoryStore::default();
    let a = env_list_with(t.path(), &secrets, Some("dev-read")).unwrap().active;
    let b = environments::save_env(t.path(), env_named("", "QA"), &["dev-read".to_string()])
        .unwrap()
        .environments
        .into_iter()
        .find(|e| e.name == "QA")
        .unwrap()
        .id;
    assert_eq!(store::configured_root(), Some(roots::env_root(t.path(), &a)));
    move_bridge_to(t.path(), &secrets, &b, None).unwrap();
    // A list read taken before the switch, or any list after it.
    let _ = env_list_with(t.path(), &secrets, Some("dev-read")).unwrap();
    assert_eq!(store::configured_root(), Some(roots::env_root(t.path(), &b)));
}

#[test]
fn reselecting_from_disk_never_makes_default() {
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    store::set_shared_root(t.path().to_path_buf());
    assert_eq!(store::reselect_from_disk(), Err(store::NO_ENVIRONMENT_YET.to_string()));
    assert!(!t.path().join("environments.json").exists());
}

#[test]
fn a_write_when_no_environment_is_selected_says_why_not_that_it_was_switched() {
    let _log = crate::serial::log_tail();
    let _lock = crate::serial::autorun();
    let _restore = Restore;
    let t = tempfile::tempdir().unwrap();
    let root = roots::env_root(t.path(), A);

    // The list cannot be read.
    std::fs::write(t.path().join("environments.json"), "{ half a file").unwrap();
    store::set_shared_root(t.path().to_path_buf());
    assert_eq!(roots::guard(&root).map(drop), Err(store::ENVIRONMENTS_UNREADABLE.to_string()));

    // There is none yet.
    std::fs::remove_file(t.path().join("environments.json")).unwrap();
    store::set_shared_root(t.path().to_path_buf());
    assert_eq!(roots::guard(&root).map(drop), Err(store::NO_ENVIRONMENT_YET.to_string()));
}
