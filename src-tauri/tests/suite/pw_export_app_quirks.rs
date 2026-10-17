//! Playwright export: TCM's quirks and the sign-in recipe's page handling,
//! merged into the repo's `docs/app-quirks.md` without touching its own
//! entries.

use serde_json::json;
use v2_lib::autorun::quirks::{Quirk, QuirkSource};
use v2_lib::autorun::recipe::SignInRecipe;
use v2_lib::pw_export::app_quirks::{merge_app_quirks, merge_app_quirks_removing, quirk_entries, retired_tags, TcmQuirkEntry};

/// A quirk of 2025-10-09 that came out of `cases`.
fn quirk(id: &str, text: &str, cases: &[i32]) -> Quirk {
    let mut q = Quirk::new(text, "assistant", "autorun", 1_760_000_000_000);
    q.id = id.into();
    q.sources = cases.iter().map(|c| QuirkSource { case_id: *c, steps: vec![1], class: None }).collect();
    q
}

fn entry(tag: &str, title: &str) -> TcmQuirkEntry {
    TcmQuirkEntry {
        tag: tag.into(),
        title: title.into(),
        symptom: "a step on this page fails without this handling (from TCM)".into(),
        cause: "the cause".into(),
        fix: "Handle as TCM does: the cause".into(),
        established: "2025-10-09".into(),
        permanent: "not yet promoted".into(),
        class: "app quirk".into(),
    }
}

/// The same entry with another cause.
fn with_cause(mut e: TcmQuirkEntry, cause: &str) -> TcmQuirkEntry {
    e.cause = cause.into();
    e
}

/// What the repo's own pipeline writes: an intro, a rule, entries between rules.
const REPO_FILE: &str = "# Known application quirks\n\nIntro written by the repo.\n\n---\n\n\
### Sidebar label matches an invisible duplicate\n\n\
**Symptom in a failing run:** a 15s action timeout.\n\n**Cause:** a hidden twin.\n\n\
**Fix pattern (raw spec):** filter to the visible one.\n\n**Established live:** 2026-08-27\n\n\
**Permanent fix:** `src/components/app-shell.ts`\n\n**Class:** app quirk\n\n---\n\n\
### The repo's other entry\n\n**Symptom in a failing run:** none.\n\n**Class:** product bug\n";

#[test]
fn no_quirks_no_change() {
    assert_eq!(merge_app_quirks(Some(REPO_FILE), &[]), None);
    assert_eq!(merge_app_quirks(None, &[]), None);
    // And no recipe, no quirk of an exported case: no entries at all.
    assert!(quirk_entries(&[quirk("qaaaaaa", "Other case.", &[9])], &[1], None).is_empty());
    // Writing the same entries again changes nothing.
    let once = merge_app_quirks(Some(REPO_FILE), &[entry("tcm:q1", "One")]).unwrap();
    assert_eq!(merge_app_quirks(Some(&once), &[entry("tcm:q1", "One")]), None);
}

#[test]
fn new_entries_are_appended_after_a_rule() {
    let out = merge_app_quirks(Some(REPO_FILE), &[entry("tcm:q1", "First one"), entry("tcm:q2", "Second one")]).unwrap();
    assert!(out.starts_with(REPO_FILE), "the repo's file is untouched up to the end:\n{out}");
    assert_eq!(
        &out[REPO_FILE.len()..],
        "\n---\n\n### First one\n<!-- tcm:q1 -->\n\n\
         **Symptom in a failing run:** a step on this page fails without this handling (from TCM)\n\n\
         **Cause:** the cause\n\n**Fix pattern (raw spec):** Handle as TCM does: the cause\n\n\
         **Established live:** 2025-10-09\n\n**Permanent fix:** not yet promoted\n\n**Class:** app quirk\n\n\
         ---\n\n### Second one\n<!-- tcm:q2 -->\n\n\
         **Symptom in a failing run:** a step on this page fails without this handling (from TCM)\n\n\
         **Cause:** the cause\n\n**Fix pattern (raw spec):** Handle as TCM does: the cause\n\n\
         **Established live:** 2025-10-09\n\n**Permanent fix:** not yet promoted\n\n**Class:** app quirk\n"
    );
}

#[test]
fn a_missing_file_is_created_with_a_heading() {
    let out = merge_app_quirks(None, &[entry("tcm:q1", "First one")]).unwrap();
    assert!(out.starts_with("# Known application quirks\n\n---\n\n### First one\n<!-- tcm:q1 -->\n"), "{out}");
    assert!(out.ends_with("**Class:** app quirk\n"), "{out}");
}

#[test]
fn a_tagged_entry_is_replaced_in_place() {
    let first = merge_app_quirks(Some(REPO_FILE), &[entry("tcm:q1", "Old title")]).unwrap();
    // The repo adds an entry after ours, then TCM's text changes.
    let later = format!("{first}\n---\n\n### Added later by the repo\n\n**Class:** environment flake\n");
    let out = merge_app_quirks(Some(&later), &[with_cause(entry("tcm:q1", "New title"), "a better cause")]).unwrap();
    assert!(!out.contains("Old title") && out.contains("### New title\n<!-- tcm:q1 -->\n"), "{out}");
    assert_eq!(out.matches("<!-- tcm:q1 -->").count(), 1, "{out}");
    assert!(out.contains("**Cause:** a better cause"), "{out}");
    // Before and after it, byte for byte.
    let at = first.find("---\n\n### Old title").unwrap();
    assert!(out.starts_with(&first[..at]));
    assert!(out.ends_with("\n---\n\n### Added later by the repo\n\n**Class:** environment flake\n"), "{out}");
    // Replaced where it stood, so nothing was appended.
    assert_eq!(out.matches("\n---\n").count(), later.matches("\n---\n").count());
}

#[test]
fn untagged_quirks_are_never_touched() {
    // A repo entry that even shares a title with ours, and a hand-edited one
    // with a rule and a heading inside a code fence.
    let repo = format!(
        "{REPO_FILE}\n---\n\n### Same title\n\n**Cause:** hand edited\n```\n---\n### not a heading\n```\n\n\
         **Class:** app quirk\n"
    );
    let out = merge_app_quirks(Some(&repo), &[entry("tcm:q1", "Same title"), entry("tcm:q2", "Other")]).unwrap();
    assert!(out.starts_with(&repo), "everything the repo wrote is still there, unchanged:\n{out}");
    // Replacing one of ours never reaches past its own block.
    let out2 = merge_app_quirks(Some(&out), &[with_cause(entry("tcm:q1", "Same title"), "x")]).unwrap();
    assert!(out2.starts_with(&repo), "{out2}");
    assert!(out2.contains("**Cause:** x") && out2.contains("<!-- tcm:q2 -->"), "{out2}");
}

#[test]
fn a_fence_inside_our_own_entry_does_not_end_it_early() {
    let text = merge_app_quirks(Some(REPO_FILE), &[entry("tcm:q1", "One")]).unwrap();
    // The repo's engineer adds a code sample, with a rule inside it, to our entry.
    let (kept, ours) = text.split_at(REPO_FILE.len());
    let edited = format!(
        "{kept}{}",
        ours.replace("**Class:** app quirk\n", "**Class:** app quirk\n\n```\n---\n### sample\n```\n")
    );
    let out = merge_app_quirks(Some(&edited), &[with_cause(entry("tcm:q1", "One"), "changed")]).unwrap();
    assert!(!out.contains("### sample"), "the whole block is TCM's to replace:\n{out}");
    assert!(out.contains("**Cause:** changed"), "{out}");
}

#[test]
fn only_quirks_of_exported_cases_travel() {
    let mut retired = quirk("qdddddd", "Retired note.", &[1]);
    retired.status = "retired".into();
    let quirks = vec![
        quirk("qaaaaaa", "A grid pages after fifty rows. Scroll to load the rest.", &[1, 2]),
        quirk("qbbbbbb", "From a case that is not exported.", &[3]),
        quirk("qcccccc", "Filed on its own, no source case.", &[]),
        retired,
    ];
    let e = quirk_entries(&quirks, &[1, 4], None);
    assert_eq!(e.len(), 1, "{e:?}");
    assert_eq!(e[0].tag, "tcm:qaaaaaa");
    assert_eq!(e[0].title, "A grid pages after fifty rows");
    assert_eq!(e[0].symptom, "a step on this page fails without this handling (from TCM)");
    assert_eq!(e[0].cause, "A grid pages after fifty rows. Scroll to load the rest.");
    assert_eq!(e[0].fix, "Handle as TCM does: A grid pages after fifty rows. Scroll to load the rest.");
    assert_eq!(e[0].established, "2025-10-09");
    assert_eq!(e[0].permanent, "not yet promoted");
    assert_eq!(e[0].class, "app quirk");
}

#[test]
fn a_long_first_sentence_is_cut_for_the_title() {
    let text = format!("{} and on.", "word ".repeat(30));
    let e = quirk_entries(&[quirk("qaaaaaa", &text, &[1])], &[1], None);
    assert!(e[0].title.chars().count() <= 80 && e[0].title.ends_with('…'), "{}", e[0].title);
    // A note that runs over several lines is still one line in the file.
    let e = quirk_entries(&[quirk("qbbbbbb", "Line one\n---\n### sneaky", &[1])], &[1], None);
    assert_eq!(e[0].cause, "Line one --- ### sneaky");
}

#[test]
fn recipe_popups_and_loaders_become_entries() {
    let recipe: SignInRecipe = serde_json::from_value(json!({
        "start_url": "https://hr.example.internal/",
        "steps": [ { "kind": "click", "selector": { "css": "#login" } } ],
        "signed_in": { "css": "#marker" },
        "close_when_shown": [
            { "name": "cookie banner", "shown": { "css": "#cookieBanner.show" },
              "close": { "role": "button", "name": "Close" } }
        ],
        "wait_while_shown": [
            { "name": "page spinner", "shown": { "css": ".spinner" }, "max_ms": 45000 },
            { "name": "grid spinner", "shown": { "css": ".grid-spin" } },
            { "name": "quick spinner", "shown": { "css": ".q" }, "max_ms": 500 }
        ]
    }))
    .unwrap();
    let e = quirk_entries(&[], &[1], Some(&recipe));
    assert_eq!(e.len(), 4, "{e:?}");

    assert_eq!(e[0].tag, "tcm:popup:cookie banner");
    assert_eq!(e[0].title, "cookie banner pops up and blocks the page");
    assert_eq!(e[0].fix, "close it with button \"Close\" whenever #cookieBanner.show is visible");

    assert_eq!(e[1].tag, "tcm:loading:page spinner");
    assert_eq!(e[1].title, "page spinner loading indicator");
    assert_eq!(e[1].fix, "wait while .spinner is visible, up to 45 s");
    // No limit given: the runner's own default.
    assert_eq!(e[2].fix, "wait while .grid-spin is visible, up to 90 s");
    // Under a second is not "0 s".
    assert_eq!(e[3].fix, "wait while .q is visible, up to 0.5 s");

    // The cause is one clean sentence: single spaces, nothing lost between its halves.
    assert_eq!(
        e[0].cause,
        "the cookie banner can be up over the page's controls, so an action behind it fails (TCM's sign-in recipe closes it)"
    );
    assert_eq!(
        e[1].cause,
        "while the page spinner is up the page is still loading, so an action started then times out (TCM's sign-in recipe waits it out)"
    );

    for x in &e {
        assert_eq!(x.symptom, "a step on this page fails without this handling (from TCM)");
        assert_eq!(x.permanent, "not yet promoted");
        assert_eq!(x.class, "app quirk");
    }
    // They travel with any export, whichever cases it holds.
    assert_eq!(quirk_entries(&[], &[], Some(&recipe)).len(), 4);
}

#[test]
fn a_crlf_file_stays_crlf() {
    let crlf = REPO_FILE.replace('\n', "\r\n");
    let out = merge_app_quirks(Some(&crlf), &[entry("tcm:q1", "One")]).unwrap();
    assert!(!out.replace("\r\n", "").contains('\n'), "a bare line feed crept in:\n{out:?}");
    let again = merge_app_quirks(Some(&out), &[with_cause(entry("tcm:q1", "One"), "y")]).unwrap();
    assert!(!again.replace("\r\n", "").contains('\n') && again.contains("**Cause:** y"));
}

#[test]
fn a_repo_entry_after_ours_with_no_rule_is_not_swallowed() {
    let ours = merge_app_quirks(Some(REPO_FILE), &[entry("tcm:q1", "One")]).unwrap();
    for heading in ["## repo entry", "#### sub-heading", "# top", "### plain repo entry"] {
        // The repo adds an entry right after ours, with no rule between.
        let tail = format!("
{heading}

**Class:** app quirk
");
        let later = format!("{ours}{tail}");
        let out = merge_app_quirks(Some(&later), &[with_cause(entry("tcm:q1", "One"), "changed")]).unwrap();
        assert!(out.ends_with(&tail), "{heading} was swallowed:
{out}");
        assert!(out.contains("**Cause:** changed") && out.starts_with(&ours[..REPO_FILE.len()]), "{out}");
    }
}

#[test]
fn a_heading_inside_a_fence_does_not_end_our_entry() {
    let ours = merge_app_quirks(Some(REPO_FILE), &[entry("tcm:q1", "One")]).unwrap();
    let edited = format!("{ours}
```
## in a fence
```
");
    let out = merge_app_quirks(Some(&edited), &[with_cause(entry("tcm:q1", "One"), "c2")]).unwrap();
    assert!(!out.contains("in a fence"), "{out}");
}

#[test]
fn two_entries_with_one_tag_are_written_once() {
    let out = merge_app_quirks(None, &[entry("tcm:q1", "First"), entry("tcm:q1", "Second")]).unwrap();
    assert_eq!(out.matches("<!-- tcm:q1 -->").count(), 1, "{out}");
    assert!(out.contains("### First") && !out.contains("### Second"), "{out}");
}

#[test]
fn a_retired_quirk_leaves_the_repo_file() {
    let mut gone = quirk("qgone01", "This one is retired now.", &[1]);
    gone.status = "retired".into();
    let mut other_case = quirk("qother1", "Retired, but from a case not exported.", &[7]);
    other_case.status = "retired".into();
    let live = quirk("qlive01", "Still true.", &[1]);
    let quirks = vec![gone, other_case, live];
    assert_eq!(retired_tags(&quirks, &[1]), vec!["tcm:qgone01".to_string()]);

    // Three TCM entries between repo ones: the retired one is in the middle.
    let file = merge_app_quirks(
        Some(REPO_FILE),
        &[entry("tcm:qgone01", "Gone"), entry("tcm:qother1", "Other"), entry("tcm:qlive01", "Live")],
    )
    .unwrap();
    let out = merge_app_quirks_removing(Some(&file), &[entry("tcm:qlive01", "Live")], &["tcm:qgone01".to_string()]).unwrap();
    assert!(!out.contains("tcm:qgone01") && !out.contains("### Gone"), "{out}");
    assert!(out.contains("tcm:qother1") && out.contains("tcm:qlive01") && out.starts_with(REPO_FILE), "{out}");
    // No doubled rule or run of blank lines where it stood.
    assert!(!out.contains("---

---") && !out.contains("


"), "{out:?}");
    // Exactly the file TCM would have written without that entry.
    let expected = merge_app_quirks(
        Some(REPO_FILE),
        &[entry("tcm:qother1", "Other"), entry("tcm:qlive01", "Live")],
    )
    .unwrap();
    assert_eq!(out, expected);

    // The first and the last of TCM's entries, too, and then nothing left to remove.
    let out = merge_app_quirks_removing(Some(&file), &[], &["tcm:qlive01".to_string()]).unwrap();
    assert!(!out.contains("qlive01") && out.ends_with("**Class:** app quirk
"), "{out:?}");
    let all = ["tcm:qgone01", "tcm:qother1", "tcm:qlive01"].map(String::from);
    assert_eq!(merge_app_quirks_removing(Some(&file), &[], &all).unwrap(), REPO_FILE);
    assert_eq!(merge_app_quirks_removing(Some(REPO_FILE), &[], &all), None);
    assert_eq!(merge_app_quirks_removing(None, &[], &all), None);
}
