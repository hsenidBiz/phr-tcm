//! Playwright export: the per-feature test-case markdown.

use v2_lib::pw_export::handover::{Handover, ORDER_PROPOSAL};
use v2_lib::pw_export::test_case::{
    case_ids_in, new_file, section, splice, with_assumed_marker, with_order_block, CaseDoc,
};

fn doc(id: i32, title: &str) -> CaseDoc {
    CaseDoc {
        id,
        title: title.into(),
        state: "Design".into(),
        area_path: "HRM\\Gamma".into(),
        iteration_path: "HRM\\Gamma\\S5".into(),
        project: "HRM".into(),
        module: "Performance".into(),
        tags: "A; B".into(),
        preconditions: "Logged in\n\n  Data exists  \n".into(),
        steps: vec![
            ("Open the page".into(), "Page shows".into()),
            ("Click a | b".into(), "Line1\r\nLine2".into()),
        ],
        folder: "sl/admin/x".into(),
        navigation_captured: true,
        feature_title: "Competencies".into(),
        handover: Handover::default(),
    }
}

#[test]
fn a_section_in_the_repos_layout() {
    let want = "## 7 \u{2014} My case\n\n### Metadata\n\n| Field | Value |\n|-------|-------|\n| ID | 7 |\n| Title | My case |\n| State | Design |\n| Area Path | HRM\\Gamma |\n| Iteration | HRM\\Gamma\\S5 |\n| Project | HRM |\n| Module | Performance |\n| Tags | A; B |\n\n**Folder:** sl/admin/x \u{2014} mapped in TCM.\n\n**Navigation:** captured in src/navigation.json.\n\n### Preconditions\n\n| # | Condition |\n|---|-----------|\n| 1 | Logged in |\n| 2 | Data exists |\n\n### Test Data & Validation\n\n| Name | Value / Rule | Source | Expected After Save | Notes |\n|------|--------------|--------|---------------------|-------|\n\n### Navigation Path\n\n```text\nCompetencies\n```\n\n### Test Steps\n\n| Step | Action | Expected Result |\n|------|--------|-----------------|\n| 1 | Open the page | Page shows |\n| 2 | Click a \\| b | Line1<br>Line2 |\n\n### Cleanup / Postconditions\n\n| # | Action |\n|---|--------|\n\n";
    assert_eq!(section(&doc(7, "My case")), want);
    let mut d = doc(7, "My case");
    d.navigation_captured = false;
    assert!(section(&d)
        .contains("**Navigation:** not yet captured - capture it in this repo before refactoring.\n"));
}

#[test]
fn splice_replaces_only_its_own_section() {
    let s2 = "## 2 \u{2014} Mid\n\n### Test Steps\n\n**Assertion floor:** 72 (raw sum 80)\n\n";
    let file = format!(
        "# T\r\n\r\n## 1 \u{2014} A\r\nold\r\n\r\n{s2}## 3 \u{2014} C\r\nold3\r\n"
    );
    let a = splice(&file, 1, &section(&doc(1, "A2")));
    let b = splice(&a, 3, &section(&doc(3, "C2")));
    assert!(b.contains(s2));
    assert!(b.starts_with("# T\r\n\r\n## 1 \u{2014} A2\n"));
    assert!(!b.contains("\nold"));
    assert!(b.ends_with("| # | Action |\n|---|--------|\n\n"));
    // an ### heading never ends a section
    let nested = "## 5 \u{2014} X\n### Metadata\nm\n## 6 \u{2014} Y\ny\n";
    assert_eq!(splice(nested, 5, "## 5 \u{2014} Z\n"), "## 5 \u{2014} Z\n## 6 \u{2014} Y\ny\n");
    // id prefix does not match (## 12 is not ## 1)
    assert!(splice("## 12 \u{2014} X\nk\n", 1, "## 1 \u{2014} N\n").starts_with("## 12 \u{2014} X\nk\n\n## 1"));
}

#[test]
fn splice_appends_a_new_case() {
    let out = splice("# T\n\n## 1 \u{2014} A\nbody\n", 2, "## 2 \u{2014} B\n\n");
    assert_eq!(out, "# T\n\n## 1 \u{2014} A\nbody\n\n## 2 \u{2014} B\n\n");
    let out = splice("# T\n\n", 2, "## 2 \u{2014} B\n\n");
    assert_eq!(out, "# T\n\n## 2 \u{2014} B\n\n");
    assert_eq!(splice("", 2, "S"), "S");
}

#[test]
fn a_new_file_marks_the_user_as_assumed() {
    assert_eq!(
        new_file("Competencies", "competencies", "AutomationSL.pm.general"),
        "# Test Case Set: Competencies\n\nCovers the ADO test cases below. Each `##` section is one `test()` in\n`specs/competencies.spec.ts`; the ids here are what the linter's\n`assertion-count` rule resolves to raw counterparts via\n`suites/_generated/index.json`.\n\n**User:** AutomationSL.pm.general  <!-- assumed -->\n\n"
    );
}

#[test]
fn an_old_from_tcm_marker_becomes_assumed() {
    let old = "# T\n\n**User:** u  <!-- from-tcm -->\n\nHand note <!-- from-tcm -->.\n\n## 1 \u{2014} A\n**User:** x  <!-- from-tcm -->\n";
    let want = "# T\n\n**User:** u  <!-- assumed -->\n\nHand note <!-- from-tcm -->.\n\n## 1 \u{2014} A\n**User:** x  <!-- from-tcm -->\n";
    // Only the header's User line; a section's text and other lines are left alone.
    assert_eq!(with_assumed_marker(old), want);
    // Already assumed, or another marker the repo chose: untouched, byte for byte.
    for same in [
        "# T\n\n**User:** u  <!-- assumed -->\n\n## 1 \u{2014} A\n",
        "# T\n\n**User:** u  <!-- from-ticket -->\n\n",
        "# T\n\n**User:** u  <!-- from-tcm: odd -->\n\n",
        "# T\n\n**User:** u\n\n",
        "",
    ] {
        assert_eq!(with_assumed_marker(same), same);
    }
    // Spacing around the comment and CRLF line endings are kept.
    assert_eq!(
        with_assumed_marker("# T\r\n\r\n**User:** u <!-- from-tcm -->  \r\n\r\n## 1 \u{2014} A\r\n"),
        "# T\r\n\r\n**User:** u <!-- assumed -->  \r\n\r\n## 1 \u{2014} A\r\n"
    );
    // No trailing newline.
    assert_eq!(with_assumed_marker("**User:** u  <!-- from-tcm -->"), "**User:** u  <!-- assumed -->");
}

#[test]
fn a_mapped_component_writes_a_uses_line() {
    let mut d = doc(7, "My case");
    d.handover = Handover { uses: vec!["employee-search".into(), "date-picker".into()], ..Handover::default() };
    let out = section(&d);
    assert!(
        out.contains(
            "**Navigation:** captured in src/navigation.json.\n\n**Uses:** common-components/employee-search\n**Uses:** common-components/date-picker\n\n### Preconditions"
        ),
        "{out}"
    );
    // Before the From TCM block, which keeps its own paragraph.
    d.handover.from_tcm = vec!["step 1 appears to save data (\"Save\")".into()];
    let out = section(&d);
    assert!(out.contains("date-picker\n\n**From TCM:**\n\n- step 1"), "{out}");
    // A re-export keeps every line already there: TCM cannot tell the ones it wrote
    // from the repo's, so a component dropped from the map leaves its line behind.
    let mut again = doc(7, "My case");
    again.handover.uses = vec!["date-picker".into()];
    let spliced = splice(&format!("# T\n\n{}", section(&d)), 7, &section(&again));
    assert_eq!(spliced.matches("**Uses:**").count(), 2, "{spliced}");
    assert!(
        spliced.contains("**Uses:** common-components/employee-search\n**Uses:** common-components/date-picker\n\n"),
        "{spliced}"
    );
}

#[test]
fn a_repo_only_uses_line_survives_a_re_export() {
    let mut first = doc(7, "My case");
    first.handover.uses = vec!["employee-search".into()];
    // The repo's own step-2 checkpoint added a line TCM never mapped.
    let hand = format!("# T\n\n{}", section(&first)).replace(
        "**Uses:** common-components/employee-search\n",
        "**Uses:** common-components/hand-made\n**Uses:** common-components/employee-search\n",
    );
    // Nothing mapped now: both stay, once each, where the Uses lines go.
    let none = splice(&hand, 7, &section(&doc(7, "My case")));
    assert_eq!(none.matches("**Uses:**").count(), 2, "{none}");
    assert!(none.contains("captured in src/navigation.json.\n\n**Uses:** common-components/hand-made\n**Uses:** common-components/employee-search\n\n### Preconditions"), "{none}");
    // A newly mapped component comes after the ones already there; a re-run adds nothing.
    let mut now = doc(7, "My case");
    now.handover.uses = vec!["employee-search".into(), "date-picker".into()];
    let once = splice(&hand, 7, &section(&now));
    assert!(once.contains("hand-made\n**Uses:** common-components/employee-search\n**Uses:** common-components/date-picker\n\n"), "{once}");
    assert_eq!(splice(&once, 7, &section(&now)), once);
    // A section with no Uses line at all is spliced as it was.
    let plain = format!("# T\n\n{}", section(&doc(7, "My case")));
    assert_eq!(splice(&plain, 7, &section(&doc(7, "My case"))), plain);
}

#[test]
fn an_unmapped_component_writes_none() {
    let out = section(&doc(7, "My case"));
    assert!(!out.contains("**Uses:**"), "{out}");
}

#[test]
fn nothing_written_matches_a_stray_id_heading() {
    let mut d = doc(9, "T\n## 12 sneaky");
    d.preconditions = "## 12 pre\n## 13 pre2".into();
    d.steps = vec![("## 14 a\n## 15 b".into(), "## 16 c".into())];
    d.feature_title = "Feat\n## 17 x ``` y".into();
    let s = section(&d);
    let hits: Vec<&str> = s
        .lines()
        .filter(|l| {
            l.strip_prefix("##").is_some_and(|r| {
                let t = r.trim_start();
                t.len() < r.len() && t.starts_with(|c: char| c.is_ascii_digit())
            })
        })
        .collect();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(hits[0].starts_with("## 9 "));
    assert!(s.contains("````text\nFeat ## 17 x ``` y\n````\n"));
}

/// `doc(7, ..)` handing over `pre`, `cleanup` and `facts`.
fn handed(pre: &[&str], cleanup: &[&str], facts: &[&str]) -> CaseDoc {
    let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let mut d = doc(7, "My case");
    d.handover = Handover { preconditions: own(pre), cleanup: own(cleanup), from_tcm: own(facts), ..Handover::default() };
    d
}

#[test]
fn the_from_tcm_block_sits_after_navigation_and_is_omitted_when_empty() {
    let s = section(&handed(&[], &[], &["Look-only: must not save anything.", "step 2 appears to save data (\"Save\")"]));
    assert!(
        s.contains(concat!(
            "**Navigation:** captured in src/navigation.json.\n\n",
            "**From TCM:**\n\n",
            "- Look-only: must not save anything.\n",
            "- step 2 appears to save data (\"Save\")\n",
            "\n### Preconditions\n",
        )),
        "{s}"
    );
    assert_eq!(s.matches("**From TCM:**").count(), 1);
    let none = section(&handed(&["requires: x unchanged"], &[], &[]));
    assert!(!none.contains("From TCM:"), "{none}");
    assert!(none.contains("**Navigation:** captured in src/navigation.json.\n\n### Preconditions\n"), "{none}");
}

#[test]
fn tcm_rows_end_with_from_tcm_and_follow_the_ado_rows() {
    let s = section(&handed(
        &["requires: x unchanged", "requires: Approval = ON"],
        &["step 3 deletes \"A\" it created"],
        &[],
    ));
    assert!(
        s.contains(concat!(
            "| # | Condition |\n|---|-----------|\n",
            "| 1 | Logged in |\n",
            "| 2 | Data exists |\n",
            "| 3 | requires: x unchanged (from TCM) |\n",
            "| 4 | requires: Approval = ON (from TCM) |\n",
            "\n### Test Data",
        )),
        "{s}"
    );
    assert!(s.ends_with("| # | Action |\n|---|--------|\n| 1 | step 3 deletes \"A\" it created (from TCM) |\n\n"), "{s}");
}

#[test]
fn a_re_export_keeps_every_row_the_repo_added() {
    let first = section(&handed(&["old tcm"], &["old cleanup"], &["old fact"]));
    let engineer =
        "**Added by engineer (not from ADO):**\n\n| # | Step | Expected |\n|---|------|----------|\n| 11 | Delete it | Gone |\n\n";
    // What the repo did to it: a Preconditions row step 4 found, a Cleanup
    // row, and an engineer's step before Cleanup. The file is CRLF now.
    let repo = first
        .replace(
            "| 3 | old tcm (from TCM) |\n",
            "| 3 | old tcm (from TCM) |\n| 4 | Goal group \\| X exists (found by step 4) |\n",
        )
        .replace("| 1 | old cleanup (from TCM) |\n", "| 1 | old cleanup (from TCM) |\n| 2 | Log out all actors |\n")
        .replace("### Cleanup", &format!("{engineer}### Cleanup"))
        .replace('\n', "\r\n");
    let file = format!("# Set\r\n\r\n{repo}## 8 \u{2014} Eight\r\n\r\nkept\r\n");

    let again = section(&handed(&["new tcm"], &[], &["new fact"]));
    let out = splice(&file, 7, &again);
    let want = again
        .replace(
            "| 3 | new tcm (from TCM) |\n",
            "| 3 | Goal group \\| X exists (found by step 4) |\n| 4 | new tcm (from TCM) |\n",
        )
        .replace("| # | Action |\n|---|--------|\n", "| # | Action |\n|---|--------|\n| 1 | Log out all actors |\n")
        .replace("### Cleanup", &format!("{}### Cleanup", engineer.replace('\n', "\r\n")));
    assert_eq!(out, format!("# Set\r\n\r\n{want}## 8 \u{2014} Eight\r\n\r\nkept\r\n"));
    for gone in ["old tcm", "old cleanup", "old fact"] {
        assert!(!out.contains(gone), "{gone}: {out}");
    }
    // A second re-export changes nothing: the kept rows are not doubled.
    assert_eq!(splice(&out, 7, &again), out);
}

#[test]
fn a_re_export_keeps_the_repos_test_data_rows_whole() {
    let header = "|------|--------------|--------|---------------------|-------|\n";
    let row = "| Group name | QA Group \\| 7 | step 2 | Listed | made by the test |\n";
    let first = section(&doc(7, "My case"));
    let file = first.replacen(header, &format!("{header}{row}"), 1);
    let out = splice(&file, 7, &section(&doc(7, "My case")));
    assert_eq!(out, file);
}

#[test]
fn a_pipe_in_a_value_stays_in_its_cell() {
    let mut d = handed(&["a | b\nc"], &["x|y\r\nz"], &["p\nq | r"]);
    d.title = "One | two\nthree".into();
    let s = section(&d);
    assert!(s.contains("| 3 | a \\| b c (from TCM) |\n"), "{s}");
    assert!(s.contains("| 1 | x\\|y z (from TCM) |\n"), "{s}");
    assert!(s.contains("\n- p q | r\n"), "{s}");
    assert!(s.contains("| Title | One \\| two<br>three |\n"), "{s}");
    // Every table row is one line holding only its own cells.
    for line in s.lines().filter(|l| l.starts_with('|')) {
        let bars = line.replace("\\|", "").matches('|').count();
        assert!([3, 4, 6].contains(&bars), "{line}");
    }
}

/// The note lines for some facts, as `order_note` ends them.
fn lines(list: &[&str]) -> Vec<String> {
    list.iter().map(|l| l.to_string()).chain([ORDER_PROPOSAL.to_string()]).collect()
}

#[test]
fn the_order_block_goes_right_after_the_user_line_of_a_new_file() {
    let head = new_file("Competencies", "competencies", "AutomationSL.pm.general");
    let out = with_order_block(&head, &lines(&["run 1 before 2", "run 2 alone and last: it publishes"]));
    let user = "**User:** AutomationSL.pm.general  <!-- assumed -->\n\n";
    let block = format!(
        "<!-- tcm:order -->\n**Order (from TCM):**\n\n- run 1 before 2\n- run 2 alone and last: it publishes\n- {ORDER_PROPOSAL}\n<!-- /tcm:order -->\n\n"
    );
    assert_eq!(out, head.replace(user, &format!("{user}{block}")));
}

#[test]
fn the_order_block_is_inserted_in_an_existing_file_and_replaced_not_duplicated() {
    let existing = "# T\n\n**User:** u  <!-- from-tcm -->\n\nHand note.\n\n## 1 \u{2014} A\nbody\n";
    let first = with_order_block(existing, &lines(&["run 1 before 2"]));
    assert_eq!(first.matches("<!-- tcm:order -->").count(), 1, "{first}");
    assert!(first.starts_with("# T\n\n**User:** u  <!-- from-tcm -->\n\n<!-- tcm:order -->\n**Order (from TCM):**\n\n- run 1 before 2\n"), "{first}");
    assert!(first.ends_with("<!-- /tcm:order -->\n\nHand note.\n\n## 1 \u{2014} A\nbody\n"), "{first}");
    // The same lines again: byte for byte the same. Other lines: replaced.
    assert_eq!(with_order_block(&first, &lines(&["run 1 before 2"])), first);
    let second = with_order_block(&first, &lines(&["run 3 alone and last: it publishes"]));
    assert_eq!(second.matches("<!-- tcm:order -->").count(), 1, "{second}");
    assert!(second.contains("- run 3 alone and last: it publishes\n") && !second.contains("run 1 before 2"), "{second}");
    assert!(second.ends_with("Hand note.\n\n## 1 \u{2014} A\nbody\n"), "{second}");
    // A section spliced in afterwards leaves the block alone.
    let spliced = splice(&second, 1, &section(&doc(1, "A2")));
    assert_eq!(spliced.matches("<!-- tcm:order -->").count(), 1);
}

#[test]
fn no_order_block_for_independent_cases() {
    let existing = "# T\n\n**User:** u  <!-- from-tcm -->\n\n## 1 \u{2014} A\nbody\n";
    // Only the proposal line, or nothing at all: no block, file untouched.
    assert_eq!(with_order_block(existing, &lines(&[])), existing);
    assert_eq!(with_order_block(existing, &[]), existing);
    // A block from an earlier export goes when the cases no longer need one.
    let with = with_order_block(existing, &lines(&["run 1 before 2"]));
    assert_eq!(with_order_block(&with, &lines(&[])), existing);
}

#[test]
fn the_order_block_keeps_the_files_line_endings_and_finds_a_place_without_a_user_line() {
    let crlf = "# T\r\n\r\n**User:** u\r\n\r\n## 1 \u{2014} A\r\nbody\r\n";
    let out = with_order_block(crlf, &lines(&["run 1 before 2"]));
    assert!(out.starts_with("# T\r\n\r\n**User:** u\r\n\r\n<!-- tcm:order -->\r\n**Order (from TCM):**\r\n\r\n- run 1 before 2\r\n"), "{out:?}");
    assert!(out.ends_with("<!-- /tcm:order -->\r\n\r\n## 1 \u{2014} A\r\nbody\r\n"), "{out:?}");
    assert_eq!(with_order_block(&out, &lines(&["run 1 before 2"])), out);
    // No User line: before the first case heading.
    let bare = "# T\n\nintro\n\n## 1 \u{2014} A\nbody\n";
    let out = with_order_block(bare, &lines(&["run 1 before 2"]));
    assert!(out.starts_with("# T\n\nintro\n\n<!-- tcm:order -->\n"), "{out}");
    assert!(out.ends_with("<!-- /tcm:order -->\n\n## 1 \u{2014} A\nbody\n"), "{out}");
}

#[test]
fn the_case_ids_a_file_holds_come_in_file_order() {
    let file = "# T\n\n## 12 \u{2014} B\nx\n### 99 not a case\n## 3 \u{2014} A\n## 12 \u{2014} dup\n## 7x no\n## Amendment\n## 5\n";
    assert_eq!(case_ids_in(file), vec![12, 3, 5]);
}

#[test]
fn a_hand_broken_start_marker_never_costs_the_repo_its_text() {
    // A start with no end, ahead of the User line, then text the repo wrote.
    let broken = "# T\n\n<!-- tcm:order -->\nRepo wrote this.\nAnd this.\n\n**User:** u  <!-- from-tcm -->\n\n## 1 \u{2014} A\nbody\n";
    let once = with_order_block(broken, &lines(&["run 1 before 2"]));
    let twice = with_order_block(&once, &lines(&["run 3 alone and last: it publishes"]));
    let thrice = with_order_block(&twice, &lines(&[]));
    for out in [&once, &twice, &thrice] {
        assert!(out.contains("Repo wrote this.\nAnd this.\n"), "{out}");
        assert!(out.contains("<!-- tcm:order -->\nRepo wrote this.\n"), "{out}");
    }
    assert!(twice.contains("- run 3 alone and last: it publishes\n") && !twice.contains("run 1 before 2"), "{twice}");
    // Only the orphan start is left once TCM has nothing to say.
    assert_eq!(thrice.matches("tcm:order").count(), 1, "{thrice}");
}
