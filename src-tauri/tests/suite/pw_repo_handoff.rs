//! Finish in the repo (`pw_repo::handoff`): the prompt that runs `/gen-test`
//! steps 4, 5, 6 and 7 on what an export wrote, one per `<seg>`, and the
//! refusals. Each clone is made here in a temp folder whose path has a
//! space. Every id and name is made up.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use v2_lib::pw_repo::handoff::{cards_at, default_stem, no_user_key, prompt, valid_stem, HandoffChoice};
use v2_lib::pw_repo::sync::{ExportRecord, Exported};

const PC: &str = "sl/admin/performance/pc-cycle-setup";
const PC_MD: &str = "suites/sl/admin/performance/pc-cycle-setup/test-cases/pc-cycle-setup.md";
const RM: &str = "sl/admin/performance/rating-methods";
const RM_MD: &str = "suites/sl/admin/performance/rating-methods/test-cases/rating-methods.md";

struct Clone {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

fn put(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A clone with pc-cycle-setup (one spec) and rating-methods (two specs),
/// cases 9001 and 9002 in pc-cycle-setup, 9003 in rating-methods, 9004 in
/// the index only, and 9005 excluded.
fn clone() -> Clone {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("my clone");
    put(&root, "playwright.config.ts", "");
    put(&root, "src/navigation.json", r#"{"entries":{}}"#);
    put(&root, "src/users/users.json", "{}");
    put(
        &root,
        "suites/_generated/index.json",
        r#"{"9001":"pc-one.spec.ts","9002":"pc-two.spec.ts","9003":"rm-three.spec.ts","9004":"orphan.spec.ts","9005":"gone.spec.ts"}"#,
    );
    for f in ["pc-one", "pc-two", "rm-three", "orphan", "gone"] {
        put(&root, &format!("suites/_generated/{f}.spec.ts"), "");
    }
    put(&root, PC_MD, "# PC\n\n## 9001 One\n\n## 9002 Two\n\n## Excluded 9005 Gone\n");
    put(&root, &format!("suites/{PC}/specs/pc-cycle-setup.spec.ts"), "");
    put(&root, RM_MD, "# RM\n\n## 9003 Three\n");
    put(&root, &format!("suites/{RM}/specs/rating-methods.spec.ts"), "");
    put(&root, &format!("suites/{RM}/specs/rating-items.spec.ts"), "");
    put(&root, &format!("suites/{RM}/specs/NotAStem.spec.ts"), "");
    Clone { _dir: dir, root }
}

/// Each case recorded as exported with the user key `REPO_ADMIN`.
fn record(ids: &[i32]) -> ExportRecord {
    keyed(&ids.iter().map(|&id| (id, "REPO_ADMIN")).collect::<Vec<_>>())
}

/// Each case recorded as exported with its own user key ("" for none).
fn keyed(ids: &[(i32, &str)]) -> ExportRecord {
    let mut r = ExportRecord::default();
    for &(id, key) in ids {
        r.cases.insert(
            id,
            Exported {
                title: "t".into(),
                fingerprint: Default::default(),
                exported_at: "x".into(),
                files: vec![],
                module: String::new(),
                user_key: key.into(),
            },
        );
    }
    r
}

fn none() -> BTreeMap<i32, String> {
    BTreeMap::new()
}

#[test]
fn one_seg_with_its_single_spec_appends_to_it_by_default() {
    let c = clone();
    let cards = cards_at(&c.root, &record(&[9001, 9002]), &none(), &none(), &[9001, 9002], &[]).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].seg, PC);
    assert_eq!(cards[0].case_ids, vec![9001, 9002]);
    assert_eq!(cards[0].stems, vec!["pc-cycle-setup".to_string()]);
    assert_eq!(cards[0].stem.as_deref(), Some("pc-cycle-setup"));
    assert_eq!(
        cards[0].prompt,
        "Run steps 4, 5, 6 and 7 of .claude/commands/gen-test.md for ADO cases 9001 and 9002 in sl/admin/performance/pc-cycle-setup. \
         Steps 1-3 are already done: the Test Case Manager exported each case's raw spec \
         (`suites/_generated/pc-one.spec.ts`, `suites/_generated/pc-two.spec.ts`) and its test-case section \
         (`suites/sl/admin/performance/pc-cycle-setup/test-cases/pc-cycle-setup.md`). \
         Each raw spec runs as the seed's user: set `test.use({ userKey: 'REPO_ADMIN' })` in `suites/_generated/seed.spec.ts` to REPO_ADMIN before running them. \
         In step 4, run those raw specs and heal them in place if they fail; never regenerate them from the tickets. \
         Do not fetch the tickets and do not run git. \
         Put them in `suites/sl/admin/performance/pc-cycle-setup/specs/pc-cycle-setup.spec.ts` (append; the --spec contract). \
         The tests are independent (no --chain). \
         Stop and report on any stop-and-report condition the command defines."
    );
    // Never a bare /gen-test, which would fetch and regenerate; no dash but a hyphen.
    assert!(!cards[0].prompt.contains("/gen-test "));
    assert!(!cards[0].prompt.contains('\u{2013}') && !cards[0].prompt.contains('\u{2014}'));
}

#[test]
fn several_segs_make_one_card_each_in_the_order_asked() {
    let c = clone();
    let cards = cards_at(&c.root, &record(&[9001, 9002, 9003]), &none(), &none(), &[9003, 9001, 9002, 9003], &[]).unwrap();
    assert_eq!(cards.iter().map(|c| c.seg.as_str()).collect::<Vec<_>>(), vec![RM, PC]);
    assert_eq!(cards[0].case_ids, vec![9003]);
    assert_eq!(cards[1].case_ids, vec![9001, 9002]);
    // rating-methods has two usable specs (a name outside the rule is not
    // offered), so its default is one spec per case.
    assert_eq!(cards[0].stems, vec!["rating-items".to_string(), "rating-methods".to_string()]);
    assert_eq!(cards[0].stem, None);
    assert!(cards[0].prompt.contains("for ADO case 9003 in sl/admin/performance/rating-methods."));
    assert!(cards[0].prompt.contains("exported the case's raw spec (`suites/_generated/rm-three.spec.ts`)"));
    assert!(cards[0].prompt.contains(&format!("its test-case section (`{RM_MD}`)")));
    assert!(cards[0].prompt.contains(" One spec per case (no --spec). The tests are independent (no --chain). "));
    // One case: the singular forms.
    assert!(cards[0]
        .prompt
        .contains(" In step 4, run that raw spec and heal it in place if it fails; never regenerate it from the ticket. "));
    assert_eq!(cards[1].stem.as_deref(), Some("pc-cycle-setup"));
}

#[test]
fn a_chosen_stem_or_one_per_case_is_used_for_its_seg_only() {
    let c = clone();
    let choices = vec![
        HandoffChoice { seg: RM.into(), stem: Some("rating-items".into()) },
        HandoffChoice { seg: PC.into(), stem: None },
    ];
    let cards = cards_at(&c.root, &record(&[9001, 9003]), &none(), &none(), &[9001, 9003], &choices).unwrap();
    assert_eq!(cards[0].stem, None);
    assert!(cards[0].prompt.contains(" One spec per case (no --spec). "));
    assert_eq!(cards[1].stem.as_deref(), Some("rating-items"));
    assert!(cards[1]
        .prompt
        .contains("Put it in `suites/sl/admin/performance/rating-methods/specs/rating-items.spec.ts` (append; the --spec contract)."));

    // A new stem the person typed, if it follows the rule.
    let typed = vec![HandoffChoice { seg: PC.into(), stem: Some(" cycle-dates-2 ".into()) }];
    let cards = cards_at(&c.root, &record(&[9001]), &none(), &none(), &[9001], &typed).unwrap();
    assert_eq!(cards[0].stem.as_deref(), Some("cycle-dates-2"));
    assert!(cards[0].prompt.contains("/specs/cycle-dates-2.spec.ts`"));
}

#[test]
fn a_bad_stem_is_refused() {
    let c = clone();
    for bad in ["Cycle", "-cycle", "cycle setup", "cycle`x", "../up", ""] {
        let choices = vec![HandoffChoice { seg: PC.into(), stem: Some(bad.into()) }];
        let e = cards_at(&c.root, &record(&[9001]), &none(), &none(), &[9001], &choices).unwrap_err();
        assert!(e.contains("cannot be a spec name"), "{bad}: {e}");
    }
}

#[test]
fn paths_with_spaces_are_carried_in_backticks() {
    let c = clone();
    let md = "suites/sl/self/my area/new feature/test-cases/new feature.md";
    put(&c.root, md, "## 9010 Spaced\n");
    let mut index = std::fs::read_to_string(c.root.join("suites/_generated/index.json")).unwrap();
    index = index.replace('}', r#","9010":"spaced case.spec.ts"}"#);
    put(&c.root, "suites/_generated/index.json", &index);
    put(&c.root, "suites/_generated/spaced case.spec.ts", "");
    let cards = cards_at(&c.root, &record(&[9010]), &none(), &none(), &[9010], &[]).unwrap();
    assert_eq!(cards[0].seg, "sl/self/my area/new feature");
    assert!(cards[0].prompt.contains("in sl/self/my area/new feature."));
    assert!(cards[0].prompt.contains("(`suites/_generated/spaced case.spec.ts`)"));
    assert!(cards[0].prompt.contains(&format!("(`{md}`)")));
    // The clone's own folder (with its space) is never in the prompt.
    assert!(!cards[0].prompt.contains("my clone"));
}

#[test]
fn refusals_name_the_case_and_the_reason() {
    let c = clone();
    // Unknown to the clone and to this app.
    let e = cards_at(&c.root, &record(&[]), &none(), &none(), &[9999], &[]).unwrap_err();
    assert_eq!(e, "Case 9999 was not exported by this app, so there is nothing to finish for it here.");
    // In the repo, but not exported by this app.
    let e = cards_at(&c.root, &record(&[9001]), &none(), &none(), &[9001, 9002], &[]).unwrap_err();
    assert!(e.starts_with("Case 9002 was not exported by this app"), "{e}");
    // Exported, but the clone lost its section.
    let e = cards_at(&c.root, &record(&[9004]), &none(), &none(), &[9004], &[]).unwrap_err();
    assert_eq!(e, "Case 9004 has no section in a test-case file. Export it again first.");
    // Exported, but not in index.json.
    let e = cards_at(&c.root, &record(&[9020]), &none(), &none(), &[9020], &[]).unwrap_err();
    assert_eq!(e, "Case 9020 has no entry in suites/_generated/index.json. Export it again first.");
    // Excluded by the team.
    let e = cards_at(&c.root, &record(&[9005]), &none(), &none(), &[9005], &[]).unwrap_err();
    assert!(e.contains("excluded in the repo"), "{e}");
    // Its raw spec is gone.
    std::fs::remove_file(c.root.join("suites/_generated/pc-two.spec.ts")).unwrap();
    let e = cards_at(&c.root, &record(&[9002]), &none(), &none(), &[9002], &[]).unwrap_err();
    assert!(e.contains("pc-two.spec.ts is not in the clone"), "{e}");
    // Nothing asked.
    assert!(cards_at(&c.root, &record(&[]), &none(), &none(), &[], &[]).is_err());
    // Not a clone.
    let d = tempfile::tempdir().unwrap();
    assert!(cards_at(d.path(), &record(&[9001]), &none(), &none(), &[9001], &[]).is_err());
}

#[test]
fn a_case_written_this_run_needs_no_record() {
    let c = clone();
    let written: BTreeMap<i32, String> = [(9001, "REPO_EMP".to_string())].into_iter().collect();
    let cards = cards_at(&c.root, &record(&[]), &written, &none(), &[9001], &[]).unwrap();
    assert_eq!(cards[0].case_ids, vec![9001]);
    assert!(cards[0].prompt.contains("to REPO_EMP before running it."), "{}", cards[0].prompt);
}

#[test]
fn the_stem_rule_and_the_default() {
    for ok in ["pc-cycle-setup", "a", "9", "rating-methods-2"] {
        assert!(valid_stem(ok), "{ok}");
    }
    for bad in ["", "-a", "A", "a_b", "a.b", "a b", "é"] {
        assert!(!valid_stem(bad), "{bad}");
    }
    assert_eq!(default_stem(&["only".to_string()]).as_deref(), Some("only"));
    assert_eq!(default_stem(&[]), None);
    assert_eq!(default_stem(&["a".to_string(), "b".to_string()]), None);
}

#[test]
fn three_ids_are_listed_with_commas_and_and() {
    let keys: Vec<String> = vec!["K".into(), "K".into(), "K".into()];
    let p = prompt("sl/a/b/c", &[1, 2, 3], &["x".into()], &["y".into(), "z".into()], None, &keys);
    assert!(p.contains("for ADO cases 1, 2 and 3 in sl/a/b/c."));
    assert!(p.contains("its test-case sections (`y`, `z`)"));
}

#[test]
fn one_shared_user_key_sets_the_seed_once() {
    let c = clone();
    let cards = cards_at(&c.root, &keyed(&[(9001, "REPO_HR"), (9002, "REPO_HR")]), &none(), &none(), &[9001, 9002], &[]).unwrap();
    assert_eq!(cards[0].problem, None);
    assert!(cards[0].prompt.contains(
        "(`suites/sl/admin/performance/pc-cycle-setup/test-cases/pc-cycle-setup.md`). \
         Each raw spec runs as the seed's user: set `test.use({ userKey: 'REPO_HR' })` in `suites/_generated/seed.spec.ts` to REPO_HR before running them. \
         In step 4, run those raw specs"
    ), "{}", cards[0].prompt);
}

#[test]
fn different_user_keys_are_listed_case_by_case() {
    let c = clone();
    let cards = cards_at(&c.root, &keyed(&[(9001, "REPO_HR"), (9002, "REPO_EMP")]), &none(), &none(), &[9001, 9002], &[]).unwrap();
    assert!(cards[0].prompt.contains(
        "Each raw spec runs as the seed's user: before running each one, set `test.use({ userKey: ... })` in \
         `suites/_generated/seed.spec.ts` to its key: 9001 \u{2192} REPO_HR, 9002 \u{2192} REPO_EMP. In step 4,"
    ), "{}", cards[0].prompt);
}

#[test]
fn a_case_with_no_user_key_refuses_its_seg_only() {
    let c = clone();
    // 9002's record predates the key and the mapping gives none; 9003 is in another seg.
    let r = keyed(&[(9001, "REPO_HR"), (9002, ""), (9003, "REPO_HR")]);
    let cards = cards_at(&c.root, &r, &none(), &none(), &[9001, 9002, 9003], &[]).unwrap();
    assert_eq!(cards[0].seg, PC);
    assert_eq!(cards[0].problem.as_deref(), Some(no_user_key(9002).as_str()));
    assert_eq!(cards[0].prompt, "");
    assert!(no_user_key(9002).starts_with("Case 9002 needs a user key mapped in the export first"));
    assert_eq!(cards[1].seg, RM);
    assert_eq!(cards[1].problem, None);
    assert!(cards[1].prompt.contains("to REPO_HR before running it."));

    // The export's mapping fills a key the record does not have.
    let mapped: BTreeMap<i32, String> = [(9002, "REPO_HR".to_string())].into_iter().collect();
    let cards = cards_at(&c.root, &r, &none(), &mapped, &[9001, 9002], &[]).unwrap();
    assert_eq!(cards[0].problem, None);
    assert!(cards[0].prompt.contains("to REPO_HR before running them."));

    // A key the prompt could not carry as it is counts as none.
    let cards = cards_at(&c.root, &keyed(&[(9001, "it's")]), &none(), &none(), &[9001], &[]).unwrap();
    assert_eq!(cards[0].problem.as_deref(), Some(no_user_key(9001).as_str()));
}
