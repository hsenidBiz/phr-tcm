//! Bug reports go to a PUBLIC repo, so the log that travels with one must
//! not carry the company in it. These tests are that promise.

use v2_lib::bugreport::{body, excerpt, issue_url, scrub, title};

const LOG: &str = "\
2026-07-28 09:00:01 [INFO] Test Case Manager 1.13.1 started
2026-07-28 09:00:04 [DEBUG] GET dev.azure.com/AcmeCorp/Patient Portal/_apis/wit/workitems/42 -> 200 in 143 ms
2026-07-28 09:00:09 [INFO] Submitting 3 test case(s) to AcmeCorp/Patient Portal PBI #144714
2026-07-28 09:00:11 [ERROR] Submit failed for 'Sign in as the ward administrator': 400
2026-07-28 09:00:12 [INFO] Signed in as nurse.jones@acmehealth.co.uk";

#[test]
fn the_organization_and_project_never_survive_scrubbing() {
    let out = scrub(LOG, "AcmeCorp", "Patient Portal");
    assert!(!out.contains("AcmeCorp"), "org leaked:\n{out}");
    assert!(!out.contains("Patient Portal"), "project leaked:\n{out}");
    assert!(out.contains("<org>"));
    assert!(out.contains("<project>"));
}

#[test]
fn work_item_titles_and_email_addresses_are_removed() {
    let out = scrub(LOG, "AcmeCorp", "Patient Portal");
    assert!(!out.contains("ward administrator"), "a title leaked:\n{out}");
    assert!(!out.contains("nurse.jones"), "an address leaked:\n{out}");
    assert!(out.contains("'<redacted>'"));
    assert!(out.contains("<email>"));
}

/// Ids stay. A bare number discloses nothing outside the organization, and
/// without it the log cannot be tied to the run it describes.
#[test]
fn ids_timestamps_and_endpoints_survive_so_the_log_is_still_useful() {
    let out = scrub(LOG, "AcmeCorp", "Patient Portal");
    assert!(out.contains("#144714"));
    assert!(out.contains("workitems/42"));
    assert!(out.contains("-> 200 in 143 ms"));
    assert!(out.contains("2026-07-28 09:00:01"));
    assert!(out.contains("[ERROR]"));
    // Line structure is preserved - a log squashed to one line is unreadable.
    assert_eq!(out.lines().count(), LOG.lines().count());
}

/// A project called "Web" inside an org called "WebPortal" must not carve
/// the longer name up. Longest name is replaced first.
#[test]
fn a_short_name_does_not_chew_holes_in_a_longer_one() {
    let out = scrub("hitting WebPortal/Web/_apis", "WebPortal", "Web");
    assert_eq!(out, "hitting <org>/<project>/_apis");
}

#[test]
fn an_empty_or_one_character_name_is_not_used_as_a_pattern() {
    // Replacing every "a" would destroy the log.
    let out = scrub("a normal line about a thing", "a", "");
    assert_eq!(out, "a normal line about a thing");
}

#[test]
fn a_long_log_is_cut_on_a_line_boundary_keeping_the_newest() {
    let big = (0..900)
        .map(|i| format!("2026-07-28 09:00:00 [DEBUG] line number {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let (text, truncated) = excerpt(&big);
    assert!(truncated);
    assert!(text.len() < big.len());
    // The END of the log is what matters - it is where the failure is.
    assert!(text.contains("line number 899"));
    assert!(!text.contains("line number 0\n"));
    // No half lines.
    assert!(text.starts_with("2026-07-28"), "cut mid-line: {:?}", &text[..40]);
}

#[test]
fn a_short_log_is_carried_whole() {
    let (text, truncated) = excerpt("one line");
    assert!(!truncated);
    assert_eq!(text, "one line");
}

#[test]
fn the_title_is_the_reporters_own_first_line() {
    assert_eq!(title("Steps vanish when I reorder them\n\nmore detail"), "Steps vanish when I reorder them");
    assert_eq!(title("   "), "Bug report");
    assert!(title(&"x".repeat(200)).chars().count() <= 80);
}

#[test]
fn the_body_names_the_file_to_attach_and_the_build_it_came_from() {
    let out = body("It broke", "1.13.1", "windows x86_64", "some log", false, "tcm-bug-report-9.log");
    assert!(out.contains("It broke"));
    assert!(out.contains("1.13.1"));
    assert!(out.contains("windows x86_64"));
    assert!(out.contains("tcm-bug-report-9.log"));
    assert!(out.contains("drag"), "the attachment step must be spelled out");
    // The reporter is told the log was already cleaned.
    assert!(out.contains("removed"));
}

#[test]
fn an_empty_description_asks_for_one_rather_than_filing_a_blank() {
    let out = body("", "1.0.0", "windows", "log", false, "f.log");
    assert!(out.contains("please say what happened"));
}

#[test]
fn a_truncated_body_says_the_file_has_the_rest() {
    let out = body("x", "1.0.0", "windows", "log", true, "f.log");
    assert!(out.contains("Older lines were dropped"));
}

/// The URL goes to the PUBLIC releases repo - the private source repo would
/// shut out every colleague who installed the app rather than cloning it.
#[test]
fn the_issue_url_targets_the_public_releases_repo_and_encodes_its_payload() {
    let url = issue_url("Broke & stayed broken", "line one\nline two #1");
    assert!(url.starts_with(
        "https://github.com/AvinAlwis/azure-devops-test-case-manager-v2-releases/issues/new?"
    ));
    // Characters that would otherwise end the query or start a fragment.
    assert!(!url.contains("Broke & stayed"));
    assert!(!url.contains('\n'));
    assert!(url.contains("%26") && url.contains("%23"));
    assert!(url.contains("labels=bug"));
}

/// The reporter's own header wins; a blank one falls back to the derived
/// first line, so the quick type-and-go path keeps working unchanged.
#[test]
fn an_explicit_title_wins_and_a_blank_one_derives() {
    use v2_lib::bugreport::effective_title;
    assert_eq!(
        effective_title("Import loses tags", "Long description\nwith lines"),
        "Import loses tags"
    );
    assert_eq!(
        effective_title("   ", "First line becomes the title\nrest"),
        "First line becomes the title"
    );
    assert_eq!(effective_title("", ""), "Bug report");
    // The 80-char cap applies to explicit titles too - the issue list is
    // the whole reason the title exists.
    let long = "x".repeat(120);
    assert_eq!(effective_title(&long, "d").chars().count(), 80);
}
