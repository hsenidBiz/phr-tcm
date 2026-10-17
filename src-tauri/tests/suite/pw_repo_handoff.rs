//! Finish in the repo (`pw_repo::handoff`): the prompt that runs `/gen-test`
//! steps 4, 5, 6 and 7 on what an export wrote, one per test-case file into
//! the spec of the same stem, and every refusal. Each clone is made here in a
//! temp folder whose path has a space. Every id, name and key is made up.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use v2_lib::pw_export::handover::ORDER_PROPOSAL;
use v2_lib::pw_repo::handoff::{already_in, cards_at, declared_user_key, no_user_key, user_key, valid_name};
use v2_lib::pw_repo::sync::{ExportRecord, Exported};

const PC: &str = "sl/admin/performance/pc-cycle-setup";
const PC_MD: &str = "suites/sl/admin/performance/pc-cycle-setup/test-cases/pc-cycle-setup.md";
const PC_SPEC: &str = "suites/sl/admin/performance/pc-cycle-setup/specs/pc-cycle-setup.spec.ts";
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

/// Adds `id -> file` to index.json, and the raw spec itself when `file` can
/// be a file name.
fn index_add(root: &Path, id: i32, file: &str) {
    let path = root.join("suites/_generated/index.json");
    let mut index: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    index.insert(id.to_string(), file.into());
    std::fs::write(&path, serde_json::to_string(&index).unwrap()).unwrap();
    if !file.contains('\n') && !file.contains('/') {
        put(root, &format!("suites/_generated/{file}"), "");
    }
}

/// A clone with pc-cycle-setup (cases 9001, 9002 and 9006, 9005 excluded; its
/// spec runs as REPO_HR and already has 9006) and rating-methods (case 9003,
/// no spec yet, no navigation entry); 9004 is in the index only.
fn clone() -> Clone {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("my clone");
    put(&root, "playwright.config.ts", "");
    put(&root, "src/navigation.json", r#"{"entries":{"performance/pc-cycle-setup":{}}}"#);
    put(&root, "src/users/users.json", r#"{"REPO_HR":{},"REPO_EMP":{},"it's":{},"a`b":{},"a\nb":{}}"#);
    put(&root, "suites/_generated/index.json", "{}");
    for (id, f) in [
        (9001, "pc-one.spec.ts"),
        (9002, "pc-two.spec.ts"),
        (9003, "rm-three.spec.ts"),
        (9004, "orphan.spec.ts"),
        (9005, "gone.spec.ts"),
        (9006, "pc-six.spec.ts"),
    ] {
        index_add(&root, id, f);
    }
    put(&root, PC_MD, "# PC\n\n## 9001 One\n\n## 9002 Two\n\n## 9006 Six\n\n## Excluded 9005 Gone\n");
    put(
        &root,
        PC_SPEC,
        "import { test } from '@fixtures';\n\ntest.use({ userKey: 'REPO_HR' });\n\ntest('six', async () => {\n  // ADO 9006\n});\n",
    );
    put(&root, RM_MD, "# RM\n\n## 9003 Three\n");
    Clone { _dir: dir, root }
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
                ado_rows: vec![],
            },
        );
    }
    r
}

/// Each case recorded as exported, running as REPO_HR.
fn record(ids: &[i32]) -> ExportRecord {
    keyed(&ids.iter().map(|&id| (id, "REPO_HR")).collect::<Vec<_>>())
}

fn none() -> BTreeMap<i32, String> {
    BTreeMap::new()
}

fn map(pairs: &[(i32, &str)]) -> BTreeMap<i32, String> {
    pairs.iter().map(|(id, k)| (*id, k.to_string())).collect()
}

#[test]
fn the_prompt_for_one_file_names_every_step_its_spec_and_its_user() {
    let c = clone();
    let cards = cards_at(&c.root, &record(&[9001, 9002]), &none(), &none(), &[9001, 9002]).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!((cards[0].seg.as_str(), cards[0].stem.as_str()), (PC, "pc-cycle-setup"));
    assert_eq!(cards[0].case_ids, vec![9001, 9002]);
    assert_eq!(cards[0].problem, None);
    assert!(cards[0].already.is_empty());
    assert_eq!(
        cards[0].prompt,
        "Run steps 4, 5, 6 and 7 of .claude/commands/gen-test.md for ADO cases 9001 and 9002 in sl/admin/performance/pc-cycle-setup. \
         Steps 1-3 are already done: the Test Case Manager exported each case's raw spec \
         (`suites/_generated/pc-one.spec.ts`, `suites/_generated/pc-two.spec.ts`) and its test-case section \
         (`suites/sl/admin/performance/pc-cycle-setup/test-cases/pc-cycle-setup.md`). \
         Each raw spec runs as the seed's user: set `test.use({ userKey: 'REPO_HR' })` in `suites/_generated/seed.spec.ts` to REPO_HR before running them. \
         In step 4, run those raw specs. Classify each failure first, as step 4 says. \
         If it is a missing precondition, record it and stop. Otherwise heal it in place within the healer's bounds, and never regenerate it from the ticket. \
         Afterwards, put suites/_generated/seed.spec.ts back the way it was. \
         Read docs/tcm-protected-baseline.md and the entries tagged tcm: in docs/app-quirks.md first; never fix a test by editing or deleting a record named in the protected baseline. \
         A failure on a step the From TCM block lists as a known product defect is a product defect, not something to heal. \
         Before step 5, run step 2's shared-component checkpoint: if any steps belong to a component under suites/common-components, add its **Uses:** line to the test case. \
         The values in the raw specs come from each case's Test Case Manager script, which passed against the live app. \
         Tag them by the repo's own provenance rules: `from-ticket` where the test-case section states the value, \
         otherwise `invented`, `assumed` or `derived` as usual. \
         Do not fetch the tickets and do not run git. \
         Put them in `suites/sl/admin/performance/pc-cycle-setup/specs/pc-cycle-setup.spec.ts` with --spec pc-cycle-setup \
         (it creates the spec or appends to it; the --spec contract). \
         The tests are independent (no --chain). \
         Stop and report on any stop-and-report condition the command defines."
    );
    // Never a bare /gen-test, which would fetch and regenerate; no dash but a hyphen.
    assert!(!cards[0].prompt.contains("/gen-test "));
    assert!(!cards[0].prompt.contains('\u{2013}') && !cards[0].prompt.contains('\u{2014}'));
}

#[test]
fn each_test_case_file_gets_its_own_card_in_the_order_asked() {
    let c = clone();
    let cards = cards_at(&c.root, &record(&[9001, 9002, 9003]), &none(), &none(), &[9003, 9001, 9002, 9003]).unwrap();
    assert_eq!(cards.iter().map(|c| c.seg.as_str()).collect::<Vec<_>>(), vec![RM, PC]);
    assert_eq!(cards[0].case_ids, vec![9003]);
    assert_eq!(cards[1].case_ids, vec![9001, 9002]);
    // A spec that does not exist yet is created: the stem is the file's.
    assert_eq!(cards[0].stem, "rating-methods");
    let p = &cards[0].prompt;
    assert!(p.contains("for ADO case 9003 in sl/admin/performance/rating-methods."));
    assert!(p.contains("exported the case's raw spec (`suites/_generated/rm-three.spec.ts`)"));
    assert!(p.contains("to REPO_HR before running it."));
    assert!(p.contains("In step 4, run that raw spec. Classify a failure first, as step 4 says."));
    assert!(p.contains(
        "Put it in `suites/sl/admin/performance/rating-methods/specs/rating-methods.spec.ts` with --spec rating-methods"
    ));
    // rating-methods has no navigation entry: capture comes first.
    assert!(p.contains(
        "src/navigation.json has no entry for performance/rating-methods yet: do step 2's navigation capture with me before step 5."
    ));
    assert!(!cards[1].prompt.contains("navigation.json has no entry"));
}

#[test]
fn a_case_a_spec_already_has_is_left_out_and_listed() {
    let c = clone();
    let cards = cards_at(&c.root, &record(&[9001, 9006]), &none(), &none(), &[9001, 9006]).unwrap();
    assert_eq!(cards[0].case_ids, vec![9001]);
    assert_eq!(cards[0].already, vec![already_in(9006, &["specs/pc-cycle-setup.spec.ts".into()])]);
    assert_eq!(
        cards[0].already[0],
        "#9006 is already in specs/pc-cycle-setup.spec.ts: change that test with /edit-spec in the repo."
    );
    assert!(cards[0].prompt.contains("for ADO case 9001 in"));
    assert!(!cards[0].prompt.contains("9006"));

    // Every case already in a spec: no prompt at all.
    let cards = cards_at(&c.root, &record(&[9006]), &none(), &none(), &[9006]).unwrap();
    assert!(cards[0].case_ids.is_empty());
    assert_eq!(cards[0].prompt, "");
    assert_eq!(cards[0].problem, None);
    assert_eq!(cards[0].already.len(), 1);
}

#[test]
fn one_spec_runs_as_one_account() {
    let c = clone();
    // Cases of one file with different keys.
    let cards = cards_at(&c.root, &keyed(&[(9001, "REPO_HR"), (9002, "REPO_EMP")]), &none(), &none(), &[9001, 9002]).unwrap();
    assert_eq!(
        cards[0].problem.as_deref(),
        Some(
            "One spec runs as one account, but these cases run as different ones (#9001 as REPO_HR, #9002 as REPO_EMP): \
             map the cases to the same account, or export them in separate runs."
        )
    );
    assert_eq!(cards[0].prompt, "");

    // A key that is not the existing spec's.
    let cards = cards_at(&c.root, &keyed(&[(9001, "REPO_EMP"), (9003, "REPO_EMP")]), &none(), &none(), &[9001, 9003]).unwrap();
    assert_eq!(
        cards[0].problem.as_deref(),
        Some(
            "These cases run as REPO_EMP. Map these cases to REPO_HR, the account specs/pc-cycle-setup.spec.ts already runs as, then export them again."
        )
    );
    // rating-methods has no spec yet, so any key the repo has will do.
    assert_eq!(cards[1].problem, None);
    assert!(cards[1].prompt.contains("to REPO_EMP before running it."));
}

#[test]
fn a_key_must_be_in_users_json_and_carriable() {
    let c = clone();
    let cards = cards_at(&c.root, &keyed(&[(9003, "REPO_GONE")]), &none(), &none(), &[9003]).unwrap();
    assert_eq!(cards[0].problem.as_deref(), Some("Account REPO_GONE is not in the repo's users.json."));
    for bad in ["it's", "a`b", "a\nb"] {
        let cards = cards_at(&c.root, &keyed(&[(9003, bad)]), &none(), &none(), &[9003]).unwrap();
        assert_eq!(
            cards[0].problem.as_deref(),
            Some(format!("Account {bad} has a name the prompt cannot carry as it is.").as_str()),
            "{bad:?}"
        );
        assert_eq!(cards[0].prompt, "");
    }
}

#[test]
fn a_case_with_no_user_key_refuses_its_file_only() {
    let c = clone();
    // 9002's record predates the key and the mapping gives none; 9003 is in another file.
    let r = keyed(&[(9001, "REPO_HR"), (9002, ""), (9003, "REPO_HR")]);
    let cards = cards_at(&c.root, &r, &none(), &none(), &[9001, 9002, 9003]).unwrap();
    assert_eq!(cards[0].problem.as_deref(), Some(no_user_key(9002).as_str()));
    assert!(no_user_key(9002).starts_with("Case 9002 needs a user key mapped in the export first"));
    assert_eq!(cards[0].prompt, "");
    assert_eq!(cards[1].problem, None);

    // The export's mapping fills a key the record does not have.
    let cards = cards_at(&c.root, &r, &none(), &map(&[(9002, "REPO_HR")]), &[9001, 9002]).unwrap();
    assert_eq!(cards[0].problem, None);
}

#[test]
fn the_key_is_this_runs_export_then_the_record_then_the_mapping() {
    let r = keyed(&[(1, "RECORDED"), (2, "RECORDED"), (3, "")]);
    let written = map(&[(1, "WRITTEN"), (3, " ")]);
    let mapped = map(&[(1, "MAPPED"), (2, "MAPPED"), (3, "MAPPED"), (4, "MAPPED")]);
    assert_eq!(user_key(&r, &written, &mapped, 1).as_deref(), Some("WRITTEN"));
    assert_eq!(user_key(&r, &written, &mapped, 2).as_deref(), Some("RECORDED"));
    // A blank one is no key: the next place is asked.
    assert_eq!(user_key(&r, &written, &mapped, 3).as_deref(), Some("MAPPED"));
    assert_eq!(user_key(&r, &written, &mapped, 4).as_deref(), Some("MAPPED"));
    assert_eq!(user_key(&r, &written, &mapped, 5), None);

    // Through cards: this run's key wins over the record's.
    let c = clone();
    let cards = cards_at(&c.root, &keyed(&[(9003, "REPO_HR")]), &map(&[(9003, "REPO_EMP")]), &none(), &[9003]).unwrap();
    assert!(cards[0].prompt.contains("to REPO_EMP before running it."));
}

#[test]
fn a_case_written_this_run_needs_no_record() {
    let c = clone();
    let cards = cards_at(&c.root, &ExportRecord::default(), &map(&[(9001, "REPO_HR")]), &none(), &[9001]).unwrap();
    assert_eq!(cards[0].case_ids, vec![9001]);
    assert_eq!(cards[0].problem, None);
}

#[test]
fn a_raw_spec_name_from_index_json_must_be_plain() {
    for (i, bad) in ["x`y.spec.ts", "x\ny.spec.ts", "../up.spec.ts", "sub/x.spec.ts", "x.ts"].into_iter().enumerate() {
        let c = clone();
        let id = 9100 + i as i32;
        index_add(&c.root, id, bad);
        put(&c.root, PC_MD, &format!("## 9001 One\n\n## {id} Bad\n"));
        let e = cards_at(&c.root, &record(&[id]), &none(), &none(), &[id]).unwrap_err();
        assert!(e.contains("is not a plain .spec.ts file name"), "{bad:?}: {e}");
    }
    // A space is fine: the name is carried in backticks.
    let c = clone();
    index_add(&c.root, 9200, "spaced case.spec.ts");
    put(&c.root, RM_MD, "## 9200 Spaced\n");
    let cards = cards_at(&c.root, &record(&[9200]), &none(), &none(), &[9200]).unwrap();
    assert!(cards[0].prompt.contains("(`suites/_generated/spaced case.spec.ts`)"));
    assert!(!cards[0].prompt.contains("my clone"));
}

#[test]
fn every_seg_part_and_the_file_stem_must_be_a_usable_name() {
    let refused = "Case 9300's test-case file is not suites/<folders>/<feature>/test-cases/<name>.md with folder and file names the app can use (no _ or . at the start, and none of / \\ < > : \" | ? * ` ').";
    for md in [
        "suites/sl/admin/mod`x/feature/test-cases/feature.md",
        "suites/sl/admin/module/feat`ure/test-cases/feature.md",
        "suites/sl/admin/module/feature/test-cases/it's.md",
        // Too shallow: one folder.
        "suites/x/test-cases/feature.md",
    ] {
        let c = clone();
        index_add(&c.root, 9300, "bad-seg.spec.ts");
        put(&c.root, md, "## 9300 Bad\n");
        let e = cards_at(&c.root, &record(&[9300]), &none(), &none(), &[9300]).unwrap_err();
        assert_eq!(e, refused, "{md}");
    }
}

#[test]
fn capitals_and_spaces_are_carried_and_a_spaced_spec_is_quoted() {
    // A folder name no other path shares, so a case-blind file system keeps its capital.
    for (md, seg, stem, arg) in [
        ("suites/sl/self/DW-competencies/test-cases/DW-competencies.md", "sl/self/DW-competencies", "DW-competencies", "--spec DW-competencies "),
        ("suites/sl/admin/My Module/test-cases/Feature Cases.md", "sl/admin/My Module", "Feature Cases", "--spec \"Feature Cases\" "),
    ] {
        let c = clone();
        index_add(&c.root, 9302, "spaced.spec.ts");
        put(&c.root, md, "## 9302 Spaced\n");
        let cards = cards_at(&c.root, &record(&[9302]), &none(), &none(), &[9302]).unwrap();
        assert_eq!((cards[0].seg.as_str(), cards[0].stem.as_str()), (seg, stem), "{md}");
        assert!(cards[0].prompt.contains(&format!("`suites/{seg}/specs/{stem}.spec.ts` with {arg}")), "{}", cards[0].prompt);
    }
}

#[test]
fn a_grouped_feature_gets_its_card_at_its_own_depth() {
    let c = clone();
    put(&c.root, "src/navigation.json", r#"{"entries":{"definition-wizard/competencies":{}}}"#);
    index_add(&c.root, 9400, "comp.spec.ts");
    let md = "suites/sl/admin/performance/definition-wizard/competencies/test-cases/competencies.md";
    put(&c.root, md, "# C\n\n## 9400 Comp\n");
    let cards = cards_at(&c.root, &record(&[9400]), &none(), &none(), &[9400]).unwrap();
    assert_eq!(cards[0].seg, "sl/admin/performance/definition-wizard/competencies");
    assert_eq!(cards[0].problem, None);
    let p = &cards[0].prompt;
    assert!(
        p.contains("`suites/sl/admin/performance/definition-wizard/competencies/specs/competencies.spec.ts` with --spec competencies"),
        "{p}"
    );
    // Captured under the last two folders: no capture sentence.
    assert!(!p.contains("navigation.json has no entry"), "{p}");
    // Not captured: the sentence names the last two folders.
    put(&c.root, "src/navigation.json", r#"{"entries":{"performance/competencies":{}}}"#);
    let cards = cards_at(&c.root, &record(&[9400]), &none(), &none(), &[9400]).unwrap();
    assert!(
        cards[0].prompt.contains("src/navigation.json has no entry for definition-wizard/competencies yet"),
        "{}",
        cards[0].prompt
    );
}

#[test]
fn refusals_name_the_case_and_the_reason() {
    let c = clone();
    let e = cards_at(&c.root, &record(&[]), &none(), &none(), &[9999]).unwrap_err();
    assert_eq!(e, "Case 9999 was not exported by this app, so there is nothing to finish for it here.");
    let e = cards_at(&c.root, &record(&[9001]), &none(), &none(), &[9001, 9002]).unwrap_err();
    assert!(e.starts_with("Case 9002 was not exported by this app"), "{e}");
    let e = cards_at(&c.root, &record(&[9004]), &none(), &none(), &[9004]).unwrap_err();
    assert_eq!(e, "Case 9004 has no section in a test-case file. Export it again first.");
    let e = cards_at(&c.root, &record(&[9020]), &none(), &none(), &[9020]).unwrap_err();
    assert_eq!(e, "Case 9020 has no entry in suites/_generated/index.json. Export it again first.");
    let e = cards_at(&c.root, &record(&[9005]), &none(), &none(), &[9005]).unwrap_err();
    assert!(e.contains("excluded in the repo"), "{e}");
    std::fs::remove_file(c.root.join("suites/_generated/pc-two.spec.ts")).unwrap();
    let e = cards_at(&c.root, &record(&[9002]), &none(), &none(), &[9002]).unwrap_err();
    assert!(e.contains("pc-two.spec.ts is not in the clone"), "{e}");
    assert!(cards_at(&c.root, &record(&[]), &none(), &none(), &[]).is_err());
    let d = tempfile::tempdir().unwrap();
    assert!(cards_at(d.path(), &record(&[9001]), &none(), &none(), &[9001]).is_err());
}

#[test]
fn the_spec_user_and_the_name_rule() {
    assert_eq!(
        declared_user_key("x\ntest.use({ userKey: 'AutomationSL.a.general' });\n").as_deref(),
        Some("AutomationSL.a.general")
    );
    assert_eq!(declared_user_key("  test.use({userKey:\"K\"});").as_deref(), Some("K"));
    assert_eq!(declared_user_key("test.use({ viewport: null });\n// userKey: 'no'"), None);
    for ok in ["pc-cycle-setup", "a", "9", "sl", "A", "a_b", "a.b", "a b", "DW-competencies"] {
        assert!(valid_name(ok), "{ok}");
    }
    for bad in ["", "_a", ".a", "a`b", "a'b", "a\nb", "a/b", "con"] {
        assert!(!valid_name(bad), "{bad:?}");
    }
}

/// The pc-cycle-setup file as an export writes it: an order block under the
/// header and whatever `cases` carry as each case's body.
fn pc_md(order: &[&str], cases: &[(i32, &str)]) -> String {
    let mut md = String::from("# PC\n\n**User:** REPO_HR\n\n");
    if !order.is_empty() {
        md.push_str("<!-- tcm:order -->\n**Order (from TCM):**\n\n");
        for line in order {
            md.push_str(&format!("- {line}\n"));
        }
        md.push_str("<!-- /tcm:order -->\n\n");
    }
    for (id, body) in cases {
        md.push_str(&format!("## {id} Case\n\n**Navigation:** captured in src/navigation.json.\n\n{body}"));
    }
    md
}

const FROM_TCM: &str = "**From TCM:**\n\n- must not save the form\n\n";
const USES: &str = "**Uses:** common-components/login\n\n";

#[test]
fn a_card_with_from_tcm_facts_and_order_says_so() {
    let c = clone();
    let proposal = ORDER_PROPOSAL;
    put(
        &c.root,
        PC_MD,
        &pc_md(
            &["run 9001 before 9002 (9001 needs a unchanged and 9002 changes it)", "run 9002 alone and last: it publishes", proposal],
            &[(9001, FROM_TCM), (9002, "")],
        ),
    );
    let p = cards_at(&c.root, &record(&[9001, 9002]), &none(), &none(), &[9001, 9002]).unwrap().remove(0).prompt;
    let from_tcm = "Read each case's **From TCM:** block before running: it says what the case must not save, \
                    what it appears to write or publish, and the cleanup it relies on.";
    let baseline = "Read docs/tcm-protected-baseline.md and the entries tagged tcm: in docs/app-quirks.md first;";
    // The proposal line is for the repo owner: the md keeps it, the prompt does not.
    let order = "Order: run 9001 before 9002 (9001 needs a unchanged and 9002 changes it); run 9002 alone and last: it publishes. \
                 Keep publishing and destructive cases out of routine runs.";
    for sentence in [from_tcm, baseline, order] {
        assert!(p.contains(sentence), "{sentence}\n{p}");
    }
    assert!(!p.contains("Proposed for the repo owner"), "{p}");
    // After the missing-precondition sentence, before the navigation and checkpoint ones, in this order.
    let at = |s: &str| p.find(s).unwrap_or_else(|| panic!("{s}"));
    assert!(at("put suites/_generated/seed.spec.ts back the way it was.") < at(from_tcm));
    assert!(at(from_tcm) < at(baseline));
    assert!(at(baseline) < at("A failure on a step the From TCM block lists"));
    assert!(at("A failure on a step the From TCM block lists") < at("Order: "));
    assert!(at("Order: ") < at("Before step 5"));
    // The order sentence replaces the independence one; no doubled full stop.
    assert!(!p.contains("no --chain") && !p.contains(".."), "{p}");
    assert!(p.contains("the --spec contract). Run them in the order above. Stop and report"), "{p}");

    // Facts about a case the card does not carry say nothing.
    put(&c.root, PC_MD, &pc_md(&[], &[(9001, ""), (9002, ""), (9006, FROM_TCM)]));
    let p = cards_at(&c.root, &record(&[9001, 9002]), &none(), &none(), &[9001, 9002]).unwrap().remove(0).prompt;
    assert!(!p.contains("Read each case's **From TCM:**") && !p.contains("Order: "), "{p}");
    assert!(p.contains("The tests are independent (no --chain)."));
    // A From TCM block in one case of several is enough.
    put(&c.root, PC_MD, &pc_md(&[], &[(9001, ""), (9002, FROM_TCM)]));
    let p = cards_at(&c.root, &record(&[9001, 9002]), &none(), &none(), &[9001, 9002]).unwrap().remove(0).prompt;
    assert!(p.contains("Read each case's **From TCM:** block before running"));
}

#[test]
fn written_uses_lines_drop_the_checkpoint() {
    let c = clone();
    let checkpoint = "shared-component checkpoint";
    // Every case has a Uses line: TCM wrote them, so the instruction goes.
    put(&c.root, PC_MD, &pc_md(&[], &[(9001, USES), (9002, USES)]));
    let p = cards_at(&c.root, &record(&[9001, 9002]), &none(), &none(), &[9001, 9002]).unwrap().remove(0).prompt;
    assert!(!p.contains(checkpoint) && !p.contains("**Uses:**"), "{p}");
    assert!(p.contains("The values in the raw specs come from"));
    // Only one of the two: the repo still has to look for the other's.
    put(&c.root, PC_MD, &pc_md(&[], &[(9001, USES), (9002, "")]));
    let p = cards_at(&c.root, &record(&[9001, 9002]), &none(), &none(), &[9001, 9002]).unwrap().remove(0).prompt;
    assert!(p.contains(checkpoint));
    // A Uses line in a code fence is no declaration.
    put(&c.root, PC_MD, &pc_md(&[], &[(9001, "```\n**Uses:** common-components/login\n```\n\n")]));
    let p = cards_at(&c.root, &record(&[9001]), &none(), &none(), &[9001]).unwrap().remove(0).prompt;
    assert!(p.contains(checkpoint));
}

#[test]
fn only_a_line_the_prompt_can_carry_reaches_the_order_sentence() {
    let c = clone();
    let long = format!("run 9001 before 9002 ({})", "x".repeat(200));
    let just_fits = format!("run 9002 alone and last: it {}", "y".repeat(200 - "run 9002 alone and last: it ".len()));
    assert_eq!(just_fits.chars().count(), 200);
    put(
        &c.root,
        PC_MD,
        &pc_md(
            &["run 9001 before 9002 (9001 needs `a` unchanged)", "state 'b' changes it", "needs ../c", &long, &just_fits, "run 9001 first"],
            &[(9001, ""), (9002, "")],
        ),
    );
    let p = cards_at(&c.root, &record(&[9001, 9002]), &none(), &none(), &[9001, 9002]).unwrap().remove(0).prompt;
    assert!(
        p.contains(&format!("Order: {just_fits}; run 9001 first. Keep publishing and destructive cases out of routine runs.")),
        "{p}"
    );
    assert!(!p.contains("needs `a`") && !p.contains("state 'b'") && !p.contains("../c") && !p.contains("xxx"), "{p}");

    // Nothing left to say, or nothing but the proposal: the cases are independent.
    put(&c.root, PC_MD, &pc_md(&["needs ../c", ORDER_PROPOSAL], &[(9001, ""), (9002, "")]));
    let p = cards_at(&c.root, &record(&[9001, 9002]), &none(), &none(), &[9001, 9002]).unwrap().remove(0).prompt;
    assert!(!p.contains("Order: ") && p.contains("The tests are independent (no --chain)."), "{p}");
}
