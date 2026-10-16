//! The open Crystal Reports report (`crystal`): open, read, edit and save,
//! compare, check, new and preview, against copies of the vendored samples.
//!
//! Every test works on a copy in its own temp dir - the samples beside the
//! bundle are never written - and takes the `crystal` lock, because the
//! open report is one process-wide slot.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use v2_lib::crystal::{self, ReadFilter};

use crate::serial;

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../crystal-reports/Sample Report");
const SAMPLE: &str = "PMSV10_GoalAlignCascade.rpt";

/// A copy of `name` from the samples in `dir`, as a path string.
fn copy_sample(dir: &Path, name: &str) -> String {
    let to = dir.join(name);
    std::fs::copy(Path::new(SAMPLES).join(name), &to).expect("the sample copies");
    to.to_string_lossy().into_owned()
}

fn objects(ir: &Value) -> &Vec<Value> {
    ir["objects"].as_array().expect("the IR lists its objects")
}

/// A Text object `set_text` can rewrite, whose name picks it out alone.
fn editable_text(ir: &Value) -> Value {
    let all = objects(ir);
    all.iter()
        .find(|o| {
            let name = o["name"].as_str().unwrap_or("");
            o["kind"] == "Text"
                && o["text"]["editable"] == true
                && !name.is_empty()
                && all.iter().filter(|p| p["name"].as_str().unwrap_or("").eq_ignore_ascii_case(name)).count() == 1
        })
        .cloned()
        .expect("the sample has a plain text object with a unique name")
}

fn object_named<'a>(ir: &'a Value, name: &str) -> &'a Value {
    objects(ir).iter().find(|o| o["name"] == name).expect("the object is in the IR")
}

/// `$.report_definition.areas[0].sections[1].objects[2]` out of a compare
/// line: the object a difference belongs to.
fn object_path(line: &str) -> Option<&str> {
    let at = line.find(".objects[")?;
    let close = line[at..].find(']')?;
    Some(&line[..at + close + 1])
}

#[test]
fn opening_returns_the_ir_and_reading_can_narrow_it() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);

    let ir = crystal::open(&path).expect("a sample opens");
    assert_eq!(ir["format"], "crpt-ir/1");
    assert_eq!(crystal::current_path().as_deref(), Some(path.as_str()));

    let whole = crystal::read(&ReadFilter::default()).unwrap();
    assert_eq!(objects(&whole).len(), objects(&ir).len());

    let first = objects(&ir).iter().find(|o| !o["name"].as_str().unwrap_or("").is_empty()).unwrap().clone();
    let name = first["name"].as_str().unwrap().to_string();
    let one = crystal::read(&ReadFilter { section: None, object: Some(name.clone()) }).unwrap();
    assert_eq!(objects(&one).len(), 1, "{one}");
    assert_eq!(objects(&one)[0]["name"], name.as_str());
    let by_id = crystal::read(&ReadFilter { section: None, object: first["id"].as_str().map(String::from) }).unwrap();
    assert_eq!(objects(&by_id)[0]["id"], first["id"]);

    let section = first["section"].as_str().unwrap().to_string();
    let narrowed = crystal::read(&ReadFilter { section: Some(section.clone()), object: None }).unwrap();
    let areas = narrowed["areas"].as_array().unwrap();
    assert_eq!(areas.len(), 1);
    let sections = areas[0]["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0]["name"], section.as_str());
    let ids: Vec<&Value> = sections[0]["object_ids"].as_array().unwrap().iter().collect();
    assert_eq!(objects(&narrowed).len(), ids.len());
    assert!(objects(&narrowed).iter().all(|o| o["section"] == section.as_str()));

    let err = crystal::read(&ReadFilter { section: None, object: Some("no-such".into()) }).unwrap_err();
    assert!(err.contains("no-such"), "{err}");
    crystal::close();
    assert_eq!(crystal::current_path(), None);
    assert!(crystal::read(&ReadFilter::default()).is_err());
}

#[test]
fn an_edit_is_saved_to_disk_and_only_that_changes() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);
    let original = dir.path().join("original.rpt");
    std::fs::copy(&path, &original).unwrap();

    let ir = crystal::open(&path).unwrap();
    let target = editable_text(&ir);
    let name = target["name"].as_str().unwrap();
    let after =
        crystal::edit(Some(json!([{ "op": "set_text", "object": { "name": name }, "text": "Edited by TCM" }])), None)
            .expect("the edit is accepted");
    assert_eq!(object_named(&after, name)["text"]["text"], "Edited by TCM");

    // The file on disk is what changed: read independently of the open report.
    let lines = crystal::compare(original.to_str().unwrap(), &path).unwrap();
    assert!(!lines.is_empty(), "the saved file differs from the original");
    let paths: std::collections::BTreeSet<Option<&str>> = lines.iter().map(|l| object_path(l)).collect();
    assert_eq!(paths.len(), 1, "every difference is in one object: {lines:#?}");
    assert!(paths.iter().next().unwrap().is_some(), "{lines:#?}");
    assert!(lines.iter().any(|l| l.contains("Edited by TCM")), "{lines:#?}");

    // A plan (the camelCase vocabulary) is the other way in, and saves the same way.
    let after = crystal::edit(
        None,
        Some(
            json!({ "planVersion": 1, "operations": [{ "action": "setFontSize", "target": name, "fontSizePt": 17 }] }),
        ),
    )
    .expect("the plan is accepted");
    assert_eq!(object_named(&after, name)["text"]["font"]["size_pt"], 17.0);
    let reread = rpt_core::open(&path).unwrap();
    let reread = rpt_core::build_ir(&reread, &rpt_core::locate::locate(&reread), &path);
    let reread = serde_json::to_value(&reread).unwrap();
    assert_eq!(object_named(&reread, name)["text"]["font"]["size_pt"], 17.0);
    assert_eq!(object_named(&reread, name)["text"]["text"], "Edited by TCM");

    let both = crystal::edit(Some(json!([])), Some(json!({ "operations": [] }))).unwrap_err();
    assert!(both.contains("not both"), "{both}");
    let neither = crystal::edit(None, None).unwrap_err();
    assert!(neither.contains("ops") && neither.contains("plan"), "{neither}");
    crystal::close();
}

#[test]
fn a_refused_batch_leaves_the_file_and_reloads() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);
    let ir = crystal::open(&path).unwrap();
    let name = editable_text(&ir)["name"].as_str().unwrap().to_string();
    let before = std::fs::read(&path).unwrap();

    let err = crystal::edit(
        Some(json!([
            { "op": "set_text", "object": { "name": name }, "text": "Never saved" },
            { "op": "set_text", "object": { "name": "no-such" }, "text": "x" },
        ])),
        None,
    )
    .unwrap_err();
    assert!(err.ends_with("The report was read again from disk: nothing in this batch was saved."), "{err}");
    assert_eq!(std::fs::read(&path).unwrap(), before, "the file is byte for byte as it was");
    let now = crystal::read(&ReadFilter::default()).unwrap();
    assert_ne!(object_named(&now, &name)["text"]["text"], "Never saved");
    crystal::close();
}

#[test]
fn a_locked_or_missing_file_is_refused_in_a_sentence() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();

    let missing = dir.path().join("Missing Report.rpt");
    let err = crystal::open(missing.to_str().unwrap()).unwrap_err();
    assert!(err.contains("Missing Report.rpt"), "{err}");

    let text = dir.path().join("notes.txt");
    std::fs::write(&text, "not a report").unwrap();
    let err = crystal::open(text.to_str().unwrap()).unwrap_err();
    assert!(err.contains("notes.txt") && err.contains("not a .rpt file"), "{err}");

    let path = copy_sample(dir.path(), SAMPLE);
    let ir = crystal::open(&path).unwrap();
    let name = editable_text(&ir)["name"].as_str().unwrap().to_string();
    let before = std::fs::read(&path).unwrap();
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms.clone()).unwrap();

    let result = crystal::edit(Some(json!([{ "op": "set_text", "object": { "name": name }, "text": "Locked" }])), None);
    let unchanged = std::fs::read(&path).unwrap() == before;
    let still = crystal::read(&ReadFilter::default()).map(|ir| object_named(&ir, &name)["text"]["text"].clone());
    let leftovers: Vec<PathBuf> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| *p != text && p.to_string_lossy() != path)
        .collect();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&path, perms).unwrap();

    let err = result.unwrap_err();
    assert!(err.contains(SAMPLE), "{err}");
    assert!(unchanged, "a read-only file is left as it was");
    assert_ne!(still.unwrap(), "Locked", "the open report keeps its old state");
    assert!(leftovers.is_empty(), "no half-written file is left behind: {leftovers:?}");
    crystal::close();
}

#[test]
fn new_refuses_an_existing_file_and_opens_a_blank_one() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let existing = copy_sample(dir.path(), SAMPLE);
    let before = std::fs::read(&existing).unwrap();
    let err = crystal::new_report(&existing, Default::default()).unwrap_err();
    assert!(err.contains(SAMPLE), "{err}");
    assert_eq!(std::fs::read(&existing).unwrap(), before);

    let fresh = dir.path().join("Fresh.rpt");
    let opts = rpt_core::blank::NewReportOptions { title: Some("Fresh report".into()), ..Default::default() };
    let ir = crystal::new_report(fresh.to_str().unwrap(), opts).expect("a blank report is created");
    assert_eq!(ir["format"], "crpt-ir/1");
    assert!(fresh.is_file());
    assert_eq!(crystal::current_path().as_deref(), fresh.to_str());
    assert_eq!(ir["summary"]["title"], "Fresh report");
    crystal::close();
}

#[test]
fn check_reports_no_errors_on_a_sample() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);

    let report = crystal::check(Some(&path)).unwrap();
    assert_eq!(report["errors"], json!([]), "{report}");
    assert!(report["warnings"].is_array());

    crystal::close();
    assert!(crystal::check(None).is_err(), "with nothing open and no path there is nothing to check");
    crystal::open(&path).unwrap();
    assert_eq!(crystal::check(None).unwrap()["errors"], json!([]));
    crystal::close();
}

#[test]
fn a_saved_preview_has_pages() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);

    crystal::open(&path).unwrap();
    // Opening already rendered one, so the screen always has something to show.
    let (auto, pdf) = crystal::last_pdf().expect("opening renders a preview");
    assert!(auto.page_count > 0 && pdf.starts_with(b"%PDF"), "{auto:?}");

    let info = crystal::preview_saved().unwrap();
    assert_eq!(info.path, path);
    assert!(info.page_count > 0);
    assert_eq!(info.live, None);
    let (stored, pdf) = crystal::last_pdf().unwrap();
    assert_eq!(stored.page_count, info.page_count);
    assert!(pdf.starts_with(b"%PDF"));

    crystal::close();
    assert!(crystal::last_pdf().is_none());
    assert!(crystal::preview_saved().is_err());
}

#[test]
fn the_guide_names_every_tool_and_the_units() {
    let guide = crystal::guide();
    for tool in [
        "open_report",
        "read_report",
        "edit_report",
        "compare_reports",
        "check_report",
        "new_report",
        "preview_report",
        "preview_report_live",
        "get_report_guide",
    ] {
        assert!(guide.contains(tool), "the guide names {tool}");
    }
    for term in ["twips", "1440", "#RRGGBB", "git"] {
        assert!(guide.contains(term), "the guide mentions {term}");
    }
}

// ---- live data ------------------------------------------------------------
//
// Nothing here reaches a server: the procedure check is pure, and every
// render is handed a stand-in for the bundle's fetch (`preview_live_with`).

use rpt_core::rpt_reader::Rpt;
use rpt_core::{LiveDb, PreviewOutput};
use rpt_db_mssql::TableFetch;
use v2_lib::db::Connection;

fn procedure(name: &str) -> TableFetch {
    TableFetch::StoredProcedure { rpc_name: name.to_string(), binds: Vec::new() }
}

/// A login whose host and password are distinctive, so an answer that
/// carried either is easy to catch.
pub fn secret_login(user: &str) -> Connection {
    Connection {
        server: "secret-host".into(),
        database: "hrm".into(),
        user: user.into(),
        password: "pw-secret".into(),
        trust_cert: false,
    }
}

const SECRET_CONNECTION: &str =
    "Server=secret-host;Database=hrm;User Id=x_devlogin;Password=pw-secret;TrustServerCertificate=False";

/// Whether `text` carries the host, the password or any part of the
/// connection string, in any case.
pub fn leaks(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("secret-host") || lower.contains("pw-secret") || lower.contains("password=")
}

#[test]
fn a_report_naming_another_database_is_refused_before_connecting() {
    use crystal::live_target_of as target;
    // The report's own procedure, plain or with its schema.
    assert_eq!(target(&[procedure("usp_perf_goal_align_sheet")], "hrm").unwrap(), "usp_perf_goal_align_sheet");
    assert_eq!(target(&[procedure("dbo.usp_x")], "hrm").unwrap(), "dbo.usp_x");
    // This database by name: the qualifier is dropped, in any case.
    assert_eq!(target(&[procedure("HRM.dbo.usp_x")], "hrm").unwrap(), "dbo.usp_x");
    assert_eq!(target(&[procedure("hrm..usp_x")], "HRM").unwrap(), "usp_x");

    // Another database, a linked server, a system or extended procedure.
    let other = target(&[procedure("otherdb.dbo.usp_x")], "hrm").unwrap_err();
    assert!(other.contains("otherdb"), "{other}");
    let linked = target(&[procedure("srv.hrm.dbo.usp_x")], "hrm").unwrap_err();
    assert!(linked.contains("srv.hrm.dbo.usp_x"), "{linked}");
    for name in ["xp_x", "dbo.sp_who", "hrm.dbo.XP_cmdshell"] {
        assert!(target(&[procedure(name)], "hrm").is_err(), "{name} was let through");
    }
    // Anything that is not a plain name: the bundle brackets the name it
    // runs, so a bracket or a separator in it could end the name early.
    for name in ["usp_x]; DROP TABLE t; --", "[dbo].[usp_x]", "usp x", "", "dbo.", "usp_x;DROP", "a.b.c.d.e"] {
        assert!(target(&[procedure(name)], "hrm").is_err(), "{name:?} was let through");
    }

    // A SQL command is never run, and an unfetchable table is named.
    assert_eq!(
        target(&[TableFetch::Command { sql: "SELECT 1".into() }], "hrm").unwrap_err(),
        "this report runs a SQL command, not a stored procedure: live preview only runs stored procedures"
    );
    let plain =
        TableFetch::NotSupported { table: "Orders".into(), reason: "plain joined tables are not generated".into() };
    let why = target(&[plain.clone()], "hrm").unwrap_err();
    assert!(why.contains("Orders") && why.contains("plain joined tables are not generated"), "{why}");
    assert!(target(&[], "hrm").is_err());
    // The bundle runs the first table it can fetch, so that is the one checked.
    assert_eq!(target(&[plain, procedure("usp_y")], "hrm").unwrap(), "usp_y");
    assert!(target(&[procedure("usp_y"), procedure("otherdb.dbo.usp_z")], "hrm").is_ok());

    // The samples: a report on a stored procedure names it, and one read
    // straight from a view and a table is refused, naming them.
    let mut procedures = 0;
    for sample in std::fs::read_dir(SAMPLES).unwrap().flatten() {
        let rpt = rpt_core::open(&sample.path().to_string_lossy()).unwrap();
        match crystal::live_target(rpt.report(), "hrm") {
            Ok(name) => {
                assert!(name.starts_with("usp_"), "{}: {name}", sample.path().display());
                procedures += 1;
            }
            Err(why) => assert!(why.contains("table "), "{}: {why}", sample.path().display()),
        }
    }
    assert!(procedures > 0, "no sample runs a stored procedure");
    let sample = rpt_core::open(&Path::new(SAMPLES).join(SAMPLE).to_string_lossy()).unwrap();
    assert_eq!(crystal::live_target(sample.report(), "hrm").unwrap(), "usp_perf_goal_align_sheet");
}

#[test]
fn a_live_preview_is_kept_like_a_saved_one_and_names_its_connection() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);
    let login = secret_login("x_devlogin");

    // Nothing open: refused before anything is fetched.
    crystal::close();
    let fetched = std::cell::Cell::new(false);
    let none = crystal::preview_live_with(&login, "Dev - dev login", vec![], |_: &Rpt, _: &LiveDb| {
        fetched.set(true);
        Err("unreachable".to_string())
    });
    assert!(none.is_err() && !fetched.get(), "{none:?}");

    crystal::open(&path).unwrap();
    let given = fitting_values(&path);
    let info = crystal::preview_live_with(&login, "Dev - dev login", given.clone(), |_: &Rpt, db: &LiveDb| {
        // The bundle is handed the login as an ADO string, and the params as given.
        assert_eq!(db.connection_string, SECRET_CONNECTION);
        assert_eq!(db.params, given);
        Ok(PreviewOutput { pdf: b"%PDF-live".to_vec(), page_count: 3, diagnostics: vec!["a note".into()] })
    })
    .unwrap();
    assert_eq!(info.live.as_deref(), Some("Dev - dev login"));
    assert_eq!(
        (info.path.as_str(), info.page_count, info.diagnostics.clone()),
        (path.as_str(), 3, vec!["a note".to_string()])
    );
    let (stored, pdf) = crystal::last_pdf().unwrap();
    assert_eq!((stored.live.as_deref(), pdf.as_slice()), (Some("Dev - dev login"), b"%PDF-live".as_slice()));

    // A saved-data preview afterwards is marked as such again.
    assert_eq!(crystal::preview_saved().unwrap().live, None);
    crystal::close();
}

/// Every log line this process has written lately, for checking that none
/// carries the login or the server.
fn logged() -> Vec<String> {
    v2_lib::applog::recent(400).into_iter().map(|l| l.message).collect()
}

#[test]
fn a_live_preview_never_answers_with_the_host_or_the_password() {
    let _lock = serial::crystal();
    let _log = serial::log_tail();
    let dir = tempfile::tempdir().unwrap();
    let path = copy_sample(dir.path(), SAMPLE);
    crystal::open(&path).unwrap();
    let login = secret_login("x_devlogin");

    // A driver that echoed everything it was given, in another case too.
    let fetched = std::cell::Cell::new(0);
    let why = crystal::preview_live_with(&login, "Dev - dev login", fitting_values(&path), |_: &Rpt, db: &LiveDb| {
        fetched.set(fetched.get() + 1);
        Err(format!(
            "cannot connect to SECRET-HOST/hrm: login failed for x_devlogin (pw-secret) using {}",
            db.connection_string
        ))
    })
    .unwrap_err();
    assert_eq!(fetched.get(), 1, "the fetch ran: {why}");
    assert!(!leaks(&why) && !why.contains(SECRET_CONNECTION), "{why}");
    assert!(why.contains(SAMPLE) && why.contains("Dev - dev login"), "{why}");
    let (stored, pdf) = crystal::last_pdf().unwrap();
    assert!(pdf.is_empty() && stored.page_count == 0, "a failed render is kept as the failure");
    assert!(stored.diagnostics.iter().all(|d| !leaks(d)), "{stored:?}");
    assert_eq!(stored.live.as_deref(), Some("Dev - dev login"));

    // The server's own name in its error (tiberius: `'...' on server NAME
    // executing ...`) is not the host the login gives, and an address may
    // stand in for either. None of it reaches the answer, the stored
    // diagnostics or the log - on a failure, or in a successful render's notes.
    let machine = "SQLBOX-07\\PROD";
    let server_said = format!(
        "Token error: 'Could not find stored procedure 'usp_perf_goal_align_sheet'.' on server {machine} executing  on line 1 (code: 2812, state: 62, class: 16); reached through 10.56.3.16,1433 and [fe80::1ff:fe23:4567:890a] and ::1"
    );
    let redirect = "Server requested a connection to an alternative address: `sqlbox-07.corp.local:1433`";
    let seen = |text: &str| {
        let lower = text.to_ascii_lowercase();
        ["sqlbox-07", "10.56.3.16", "fe80::1ff", "::1", "corp.local"].iter().any(|s| lower.contains(s)) || leaks(text)
    };
    let why = crystal::preview_live_with(&login, "Dev - dev login", fitting_values(&path), |_: &Rpt, _: &LiveDb| {
        Err(format!("{server_said}; {redirect}"))
    })
    .unwrap_err();
    assert!(!seen(&why), "{why}");
    assert!(
        why.contains("Could not find stored procedure") && why.contains("code: 2812"),
        "the message itself stays: {why}"
    );
    assert!(crystal::last_pdf().unwrap().0.diagnostics.iter().all(|d| !seen(d)));
    let notes = crystal::preview_live_with(&login, "Dev - dev login", fitting_values(&path), |_: &Rpt, _: &LiveDb| {
        Ok(PreviewOutput { pdf: b"%PDF-live".to_vec(), page_count: 1, diagnostics: vec![server_said.clone()] })
    })
    .unwrap();
    assert!(notes.diagnostics.iter().all(|d| !seen(d)), "{notes:?}");
    assert!(crystal::last_pdf().unwrap().0.diagnostics.iter().all(|d| !seen(d)));
    let log = logged();
    assert!(log.iter().any(|l| l.contains("live data")), "the outcomes are logged");
    assert!(log.iter().all(|l| !seen(l)), "{log:?}");

    // A login with a separator in it is refused before the bundle sees it:
    // `;Server=` inside a password would point the string at another host.
    let mut sly = secret_login("x_devlogin");
    sly.password = "pw-secret;Server=elsewhere".into();
    let fetched = std::cell::Cell::new(false);
    let why = crystal::preview_live_with(&sly, "Dev - dev login", fitting_values(&path), |_: &Rpt, _: &LiveDb| {
        fetched.set(true);
        Err("unreachable".into())
    })
    .unwrap_err();
    assert!(!fetched.get() && !leaks(&why) && !why.contains("elsewhere"), "{why}");

    // A parameter that does not fit its declared kind is refused before
    // anything is fetched, naming the parameter.
    let param = typed_parameter(&path);
    let why = crystal::preview_live_with(
        &login,
        "Dev - dev login",
        with_value(fitting_values(&path), &param, "not-a-value"),
        |_: &Rpt, _: &LiveDb| {
            fetched.set(true);
            Err("unreachable".into())
        },
    )
    .unwrap_err();
    assert!(!fetched.get(), "{why}");
    assert!(why.contains(&param) && !leaks(&why), "{why}");
    crystal::close();
}

/// A report that reads a view rather than running a procedure is refused
/// before anything is fetched, naming what it reads.
#[test]
fn a_report_on_a_view_never_reaches_the_fetch() {
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    let on_a_view = std::fs::read_dir(SAMPLES)
        .unwrap()
        .flatten()
        .find(|s| crystal::live_target(rpt_core::open(&s.path().to_string_lossy()).unwrap().report(), "hrm").is_err())
        .expect("one sample reads a view");
    let path = copy_sample(dir.path(), &on_a_view.file_name().to_string_lossy());
    crystal::open(&path).unwrap();
    let fetched = std::cell::Cell::new(false);
    // Every parameter given, so the procedure check is the only thing that stops it.
    let given = fitting_values(&path);
    assert_eq!(crystal::check_live_params(&rpt_core::open(&path).unwrap().report().clone(), &given), Ok(()));
    let why = crystal::preview_live_with(&secret_login("x_devlogin"), "Dev - dev login", given, |_: &Rpt, _: &LiveDb| {
        fetched.set(true);
        Ok(PreviewOutput { pdf: b"%PDF".to_vec(), page_count: 1, diagnostics: vec![] })
    })
    .unwrap_err();
    assert!(!fetched.get(), "{why}");
    assert!(why.contains("table "), "{why}");
    assert_eq!(crystal::last_pdf().unwrap().0.live, None, "the saved-data preview is still the one kept");
    crystal::close();
}

/// A name the report does not have is refused - a misspelling would run
/// the procedure with NULL for the real one - and so is leaving out one the
/// report needs. Names match in any case, with or without the `@`.
#[test]
fn live_parameters_are_checked_by_name_before_connecting() {
    use rpt_core::rpt_reader::model::FieldKindData;
    let sample_path = Path::new(SAMPLES).join(SAMPLE).to_string_lossy().into_owned();
    let sample = rpt_core::open(&sample_path).unwrap();
    let given = fitting_values(&sample_path);
    assert_eq!(crystal::check_live_params(sample.report(), &given), Ok(()));
    let (name, _) = given[0].clone();
    let bare = name.trim_start_matches('@').to_string();

    let mut stranger = given.clone();
    stranger.push(("a_note".into(), "x".into()));
    let why = crystal::check_live_params(sample.report(), &stranger).unwrap_err();
    assert!(why.contains("a_note") && why.contains(&name), "names the stranger and the real ones: {why}");
    for spelt in [bare.to_uppercase(), format!("@{}", bare.to_uppercase())] {
        let mut renamed = given.clone();
        renamed[0].0 = spelt.clone();
        assert_eq!(crystal::check_live_params(sample.report(), &renamed), Ok(()), "{spelt}");
    }

    // Every parameter made one the report needs: not optional, never
    // empty, nothing saved.
    let mut strict = sample.report().clone();
    let mut needed = Vec::new();
    for f in &mut strict.data_definition.field_definitions {
        if let FieldKindData::Parameter(p) = &mut f.kind {
            if p.report_name.is_none() {
                (p.optional_prompt, p.allow_null_value, p.has_current_value) = (false, false, false);
                needed.push(f.name.trim_start_matches(['{', '?']).trim_end_matches('}').to_string());
            }
        }
    }
    assert!(!needed.is_empty());
    let why = crystal::check_live_params(&strict, &[]).unwrap_err();
    for n in &needed {
        assert!(why.contains(n.as_str()), "{n} is named: {why}");
    }
    let all: Vec<(String, String)> = needed.iter().map(|n| (n.clone(), "1".to_string())).collect();
    assert_eq!(crystal::check_live_params(&strict, &all), Ok(()));
    // Leaving out only one names only that one.
    let why = crystal::check_live_params(&strict, &all[1..]).unwrap_err();
    assert_eq!(why, format!("the report needs a value for {}: give it in params", needed[0]));

    // Through the preview: refused, and nothing fetched.
    let _lock = serial::crystal();
    let dir = tempfile::tempdir().unwrap();
    crystal::open(&copy_sample(dir.path(), SAMPLE)).unwrap();
    let fetched = std::cell::Cell::new(false);
    let why = crystal::preview_live_with(
        &secret_login("x_devlogin"),
        "Dev - dev login",
        {
            let mut misspelt = fitting_values(&sample_path);
            misspelt.push(("perfomance_cycle".into(), "1".into()));
            misspelt
        },
        |_: &Rpt, _: &LiveDb| {
            fetched.set(true);
            Err("unreachable".into())
        },
    )
    .unwrap_err();
    assert!(!fetched.get() && why.contains("perfomance_cycle"), "{why}");
    crystal::close();
}

/// A parameter of the report at `path` whose declared kind is not text, so
/// a value can fail to fit it, by the name a caller would give it.
pub fn typed_parameter(path: &str) -> String {
    use rpt_core::rpt_reader::model::ParameterValueKind;
    let rpt = rpt_core::open(path).unwrap();
    let name = rpt
        .report()
        .data_definition
        .parameter_fields()
        .find(|(_, p)| p.value_kind != ParameterValueKind::StringParameter)
        .map(|(f, _)| f.name.trim_start_matches(['{', '?']).trim_end_matches('}').to_string())
        .expect("the sample declares a parameter that is not text");
    name
}

/// Every parameter of the report at `path` (the report's own, not a
/// subreport's) with a value that fits its kind - what a live preview of
/// it needs at the least.
pub fn fitting_values(path: &str) -> Vec<(String, String)> {
    use rpt_core::rpt_reader::model::ParameterValueKind as K;
    let rpt = rpt_core::open(path).unwrap();
    let mut out: Vec<(String, String)> = Vec::new();
    for (f, p) in rpt.report().data_definition.parameter_fields() {
        let name = f.name.trim_start_matches(['{', '?']).trim_end_matches('}').to_string();
        if p.report_name.is_some() || out.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let value = match p.value_kind {
            K::NumberParameter | K::CurrencyParameter => "42",
            K::BooleanParameter => "true",
            K::DateParameter => "2026-01-31",
            K::TimeParameter => "10:30:00",
            K::DateTimeParameter => "2026-01-31T10:30:00",
            _ => "text",
        };
        out.push((name, value.to_string()));
    }
    assert!(!out.is_empty(), "the sample declares parameters");
    out
}

/// `values` with `name`'s value set to `value`.
pub fn with_value(mut values: Vec<(String, String)>, name: &str, value: &str) -> Vec<(String, String)> {
    let slot = values.iter_mut().find(|(n, _)| n == name).expect("the parameter is among the values");
    slot.1 = value.to_string();
    values
}
