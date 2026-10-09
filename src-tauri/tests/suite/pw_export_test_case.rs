//! Playwright export: the per-feature test-case markdown.

use v2_lib::pw_export::test_case::{new_file, section, splice, CaseDoc};

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
        side: "admin".into(),
        navigation_captured: true,
        feature_title: "Competencies".into(),
    }
}

#[test]
fn a_section_in_the_repos_layout() {
    let want = "## 7 \u{2014} My case\n\n### Metadata\n\n| Field | Value |\n|-------|-------|\n| ID | 7 |\n| Title | My case |\n| State | Design |\n| Area Path | HRM\\Gamma |\n| Iteration | HRM\\Gamma\\S5 |\n| Project | HRM |\n| Module | Performance |\n| Tags | A; B |\n\n**Side:** admin \u{2014} mapped in TCM.\n\n**Navigation:** captured in src/navigation.json.\n\n### Preconditions\n\n| # | Condition |\n|---|-----------|\n| 1 | Logged in |\n| 2 | Data exists |\n\n### Test Data & Validation\n\n| Name | Value / Rule | Source | Expected After Save | Notes |\n|------|--------------|--------|---------------------|-------|\n\n### Navigation Path\n\n```text\nCompetencies\n```\n\n### Test Steps\n\n| Step | Action | Expected Result |\n|------|--------|-----------------|\n| 1 | Open the page | Page shows |\n| 2 | Click a \\| b | Line1<br>Line2 |\n\n### Cleanup / Postconditions\n\n| # | Action |\n|---|--------|\n\n";
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
    assert!(!b.contains("old"));
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
fn a_new_file_header() {
    assert_eq!(
        new_file("Competencies", "competencies", "AutomationSL.pm.general"),
        "# Test Case Set: Competencies\n\nCovers the ADO test cases below. Each `##` section is one `test()` in\n`specs/competencies.spec.ts`; the ids here are what the linter's\n`assertion-count` rule resolves to raw counterparts via\n`suites/_generated/index.json`.\n\n**User:** AutomationSL.pm.general  <!-- from-tcm -->\n\n"
    );
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
