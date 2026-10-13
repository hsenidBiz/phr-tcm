//! Which ADO ids the clone already has (`pw_repo::presence`), and the guard
//! that keeps Export from writing over one unless the person chose to
//! replace it. The fixture clone is `tests/fixtures/pw_repo`; the write runs
//! on Export to Playwright's own fake clone (`pw_export_write`). Every value
//! is made up.

use crate::pw_export_write::{docs, write, Fx, MD_REL, ORG, PROJECT};
use crate::pw_repo_sync::snapshot;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use v2_lib::pw_export::export::{ensure_exportable, preview_with, write_with};
use v2_lib::pw_repo::presence::{scan, INDEX_REL};
use v2_lib::pw_repo::sync::{ExportRecord, Exported};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pw_repo")
}

const RATING_MD: &str = "suites/sl/admin/performance/rating-methods/test-cases/rating-methods.md";
const RATING_SPEC: &str = "suites/sl/admin/performance/rating-methods/specs/rating-methods.spec.ts";
const SMOKE_SPEC: &str = "suites/sl/self/smoke/framework-smoke/specs/framework-smoke-again.spec.ts";

#[test]
fn each_of_the_three_places_is_found_on_the_fixture() {
    let p = scan(&fixture()).unwrap();

    // A `## <id>` heading in a test-case file, and nothing else.
    assert_eq!(p.places(135180), [RATING_MD.to_string()]);
    assert_eq!(p.note(135180).as_deref(), Some("in rating-methods.md"));
    // An index.json entry, and nothing else.
    assert_eq!(p.places(135190), [INDEX_REL.to_string()]);
    assert_eq!(p.note(135190).as_deref(), Some("in index.json"));
    // A `// ADO <id>` marker in a spec, and nothing else.
    assert_eq!(p.places(135301), [SMOKE_SPEC.to_string()]);
    assert_eq!(p.note(135301).as_deref(), Some("in specs/framework-smoke-again.spec.ts"));
    // All three, in that order.
    assert_eq!(p.places(135152), [RATING_MD.to_string(), INDEX_REL.to_string(), RATING_SPEC.to_string()]);
    assert_eq!(
        p.note(135152).as_deref(),
        Some("in rating-methods.md, index.json and specs/rating-methods.spec.ts")
    );

    // Nowhere: not in the repo. A heading that is not a case is no id.
    assert!(!p.has(135999));
    assert_eq!(p.note(135999), None);
    assert_eq!(p.cases.keys().copied().collect::<Vec<_>>(), vec![135152, 135179, 135180, 135185, 135190, 135301]);

    // A `## Excluded <id>` section: in the repo, and left out on purpose.
    assert!(p.excluded(135185) && p.has(135185));
    assert_eq!(p.cases[&135185].excluded_in, [RATING_MD.to_string()]);
    assert_eq!(p.excluded_note(135185).as_deref(), Some("Excluded in the repo (in rating-methods.md)"));
    assert!(!p.excluded(135180));
    assert_eq!(p.excluded_note(135180), None);

    // Whether this app exported it comes from the Sync record only.
    assert!(p.cases.values().all(|f| !f.from_app));
    let mut record = ExportRecord::default();
    record.cases.insert(
        135179,
        Exported { title: "t".into(), fingerprint: Default::default(), exported_at: "x".into(), files: vec![], module: String::new() },
    );
    let p = p.with_record(&record);
    assert!(p.cases[&135179].from_app);
    assert!(!p.cases[&135152].from_app);
}

#[test]
fn a_folder_that_is_not_the_clone_is_refused() {
    let d = tempfile::tempdir().unwrap();
    assert!(scan(d.path()).is_err());
}

#[test]
fn the_preview_says_which_cases_the_repo_has_and_where() {
    let fx = Fx::new();
    fx.good(20, "Twenty");
    fx.good(21, "Twenty one");
    write_with(fx.root.path(), ORG, PROJECT, &[20], &fx.clone_path(), &fx.modules.borrow(), &docs(&[(20, "Twenty")]), &[]).unwrap();

    let p = preview_with(fx.root.path(), ORG, PROJECT, &[20, 21], &fx.clone_path(), &fx.modules.borrow()).unwrap();
    let by: BTreeMap<i32, _> = p.cases.iter().map(|c| (c.case_id, c)).collect();
    assert!(by[&20].exportable && by[&20].in_repo);
    assert_eq!(
        by[&20].repo_note.as_deref(),
        Some("in proficiency-levels.md and index.json"),
        "the raw spec has no marker; its section and index entry name it"
    );
    assert!(by[&21].exportable && !by[&21].in_repo);
    assert_eq!(by[&21].repo_note, None);
}

#[test]
fn a_write_of_the_default_ticks_skips_what_the_repo_has() {
    let fx = Fx::new();
    fx.good(20, "Twenty");
    fx.good(21, "Twenty one");
    write_with(fx.root.path(), ORG, PROJECT, &[20], &fx.clone_path(), &fx.modules.borrow(), &docs(&[(20, "Twenty")]), &[]).unwrap();
    fx.good(20, "Twenty, changed");
    let before = snapshot(fx.clone.path());
    let md_before = fx.read_clone(MD_REL);

    // The dialog's default: the new case ticked, the one the repo has not.
    let r = write_with(fx.root.path(), ORG, PROJECT, &[21], &fx.clone_path(), &fx.modules.borrow(), &docs(&[(21, "Twenty one")]), &[]).unwrap();
    assert_eq!(r.cases.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![21]);

    // 20's raw spec is as it was, and its section is still the first thing in the file.
    let after = snapshot(fx.clone.path());
    for (rel, bytes) in &before {
        if rel == MD_REL || rel == INDEX_REL {
            continue;
        }
        assert_eq!(&after[rel], bytes, "{rel} changed");
    }
    assert!(fx.read_clone(MD_REL).starts_with(md_before.trim_end()));
}

#[test]
fn a_case_the_repo_has_is_refused_unless_it_is_chosen_to_replace() {
    let fx = Fx::new();
    fx.good(22, "Twenty two");
    // Someone wrote 22's section by hand: no export of this app's.
    write(fx.clone.path().join(MD_REL), "# Proficiency Levels\n\n## 22 - Twenty two\n\n1. Hand written.\n");
    fx.good(23, "Twenty three");
    let before = snapshot(fx.clone.path());

    // Not chosen: the whole write is refused, the new case with it.
    let d = docs(&[(22, "Twenty two"), (23, "Twenty three")]);
    let err = write_with(fx.root.path(), ORG, PROJECT, &[23, 22], &fx.clone_path(), &fx.modules.borrow(), &d, &[]).unwrap_err();
    assert!(err.contains("case 22 is already in the repo (in proficiency-levels.md)"), "{err}");
    assert!(err.contains("Nothing was written"), "{err}");
    assert!(!err.contains('\u{2014}') && !err.contains('\u{2013}'), "{err}");
    assert_eq!(snapshot(fx.clone.path()), before);
    // Refused before any request is made, too.
    let early = ensure_exportable(fx.root.path(), ORG, PROJECT, &[22], &fx.clone_path(), &fx.modules.borrow(), &[]).unwrap_err();
    assert_eq!(early, err);
    // Another case chosen is not this one.
    assert!(write_with(fx.root.path(), ORG, PROJECT, &[22], &fx.clone_path(), &fx.modules.borrow(), &d, &[23]).is_err());
    assert_eq!(snapshot(fx.clone.path()), before);

    // Chosen on purpose: it is replaced.
    let r = write_with(fx.root.path(), ORG, PROJECT, &[22], &fx.clone_path(), &fx.modules.borrow(), &d, &[22]).unwrap();
    assert_eq!(r.cases.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![22]);
    assert!(!fx.read_clone(MD_REL).contains("Hand written"));
}

#[test]
fn only_an_exact_marker_line_counts() {
    use v2_lib::pw_repo::runner::marker_in;
    assert_eq!(marker_in("  // ADO 135270"), Some(135270));
    assert_eq!(marker_in("//ADO 7  "), Some(7));
    assert_eq!(marker_in("// ADO   99\r"), Some(99));
    assert_eq!(marker_in("// ADO 136431's finding that validation fires"), None);
    assert_eq!(marker_in("// see ADO 136430"), None);
    assert_eq!(marker_in("// ADO 136430."), None);
    assert_eq!(marker_in("// ADO136430"), None);
    assert_eq!(marker_in("// ADO"), None);

    // In a spec, prose that names a case does not put it in the repo.
    let fx = Fx::new();
    write(
        fx.clone.path().join("suites/sl/admin/performance/proficiency-levels/specs/proficiency-levels.spec.ts"),
        "test('a', async () => {\n    // ADO 136431's finding that validation fires via a toast\n});\n  // ADO 40\ntest('b', async () => {});\n",
    );
    let p = scan(fx.clone.path()).unwrap();
    assert!(!p.has(136431));
    assert!(p.has(40));
}

#[test]
fn an_excluded_case_is_never_exported_even_when_chosen_to_replace() {
    let fx = Fx::new();
    fx.good(26, "Twenty six");
    write(fx.clone.path().join(MD_REL), "# Proficiency Levels\n\n## Excluded 26 \u{2014} Twenty six\n\nLeft out on purpose.\n");
    let before = snapshot(fx.clone.path());
    let sentence = "Excluded in the repo (in proficiency-levels.md)";

    let p = preview_with(fx.root.path(), ORG, PROJECT, &[26], &fx.clone_path(), &fx.modules.borrow()).unwrap();
    let c = &p.cases[0];
    assert!(c.excluded && c.in_repo && !c.exportable);
    assert_eq!(c.repo_note.as_deref(), Some(sentence));

    let d = docs(&[(26, "Twenty six")]);
    for replace in [&[][..], &[26][..]] {
        let err = write_with(fx.root.path(), ORG, PROJECT, &[26], &fx.clone_path(), &fx.modules.borrow(), &d, replace).unwrap_err();
        assert!(err.contains(sentence) && err.contains("Nothing was written"), "{err}");
        let early = ensure_exportable(fx.root.path(), ORG, PROJECT, &[26], &fx.clone_path(), &fx.modules.borrow(), replace).unwrap_err();
        assert_eq!(early, err);
    }
    assert_eq!(snapshot(fx.clone.path()), before);
}
