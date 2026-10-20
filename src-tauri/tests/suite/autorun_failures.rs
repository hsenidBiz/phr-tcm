//! A run's failures, read back as text an assistant can act on - and the
//! three reasons it must refuse to touch the script instead.
//!
//! `describe_failures`'s exact wording matters: it is what an assistant
//! reads to decide what to change, so this pins the block byte for byte
//! for the case that has everything (account, a script, a picture, a
//! not-run step, a note) and the case that has nothing on disk to match
//! it against.

use v2_lib::autorun::failures::{describe_failures, latest_run, stop_reason};
use v2_lib::autorun::nav::no_path;
use v2_lib::autorun::replay::MODULE_STEP;
use v2_lib::autorun::store::save_run;
use v2_lib::autorun::{CaseRecord, CaseScript, LocalRun, StepRecord, StepScript};
use v2_lib::browser::actions::{Action, ActionOutcome};
use v2_lib::browser::locator::{LocatorStep, Target};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("tcm-autorun-failures-test-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A case with only what `latest_run` cares about: its id. Everything else
/// is filler that no assertion in this file looks at.
fn minimal_case(case_id: i32) -> CaseRecord {
    CaseRecord {
        case_id,
        title: "filler".to_string(),
        verdict: String::new(),
        note: String::new(),
        steps: vec![],
        proposed: String::new(),
        reason: String::new(),
        duration_ms: None,
        account: None,
        retried: None,
        notice: None,
        page_errors_seen: 0, phases: None, cleanup: None,
    }
}

/// A case with nothing set beyond the fields every test below overrides -
/// so each test only spells out what it actually cares about.
fn empty_case() -> CaseRecord {
    CaseRecord {
        case_id: 1,
        title: "A case".to_string(),
        verdict: String::new(),
        note: String::new(),
        steps: vec![],
        proposed: String::new(),
        reason: String::new(),
        duration_ms: None,
        account: None,
        retried: None,
        notice: None,
        page_errors_seen: 0, phases: None, cleanup: None,
    }
}

#[test]
fn stop_reason_is_some_when_the_sign_in_step_failed() {
    let case = CaseRecord {
        steps: vec![StepRecord {
            step_number: 0,
            outcomes: vec![ActionOutcome::failed("wrong password")],
            screenshot: None,
            downloads: vec![],
            tab: None,
            dialog: None,
            components: Vec::new(), duration_ms: None,
        }],
        verdict: "Blocked".to_string(),
        ..empty_case()
    };
    // The sign-in reason wins even though the verdict is also Blocked -
    // it is the more specific, more actionable of the two.
    assert_eq!(
        stop_reason(&case),
        Some("the sign-in failed - fix the account or the recipe in the app, not the script".to_string())
    );
}

#[test]
fn stop_reason_is_some_when_the_browser_stopped_answering() {
    let starts_with_the_browser = CaseRecord {
        reason: "the browser did not open: connection refused".to_string(),
        ..empty_case()
    };
    // What `propose` writes for any action whose browser gave up.
    let stopped_answering = CaseRecord {
        reason: "the browser stopped answering at step 3: the browser did not answer: timed out".to_string(),
        verdict: "Blocked".to_string(),
        ..empty_case()
    };
    let expected = Some("the browser stopped answering - rerun before changing anything".to_string());
    assert_eq!(stop_reason(&starts_with_the_browser), expected);
    assert_eq!(stop_reason(&stopped_answering), expected);
}

/// A Failed case's reason begins `step N:` and then quotes the page or the
/// script, which can say anything - including "did not answer". That is
/// still the page failing the test, not the browser stopping.
#[test]
fn a_failed_step_that_quotes_did_not_answer_is_not_the_browser_stopping() {
    for reason in [
        "step 2: page does NOT contain did not answer",
        "step 3: button \"Save\" not found (the page showed alert: the server did not answer and it was accepted)",
    ] {
        let case = CaseRecord { verdict: "Failed".to_string(), reason: reason.to_string(), ..empty_case() };
        assert_eq!(stop_reason(&case), None, "{reason}");
    }
}

#[test]
fn stop_reason_is_some_when_the_person_marked_the_case_blocked() {
    let case = CaseRecord {
        verdict: "Blocked".to_string(),
        reason: "a precondition was missing".to_string(),
        ..empty_case()
    };
    assert_eq!(
        stop_reason(&case),
        Some("the person marked this case Blocked - a missing precondition is not a script defect".to_string())
    );
}

#[test]
fn stop_reason_is_none_for_an_ordinary_script_failure() {
    let case = CaseRecord {
        verdict: "Failed".to_string(),
        reason: "step 2: button \"Save\" not found".to_string(),
        ..empty_case()
    };
    assert_eq!(stop_reason(&case), None);
}

fn save_button() -> Action {
    Action::Click { selector: Target::One(LocatorStep { role: Some("button".to_string()), name: Some("Save".to_string()), ..Default::default() }) }
}

#[test]
fn describe_failures_renders_the_exact_block_for_a_failed_case_with_a_script_and_skips_the_rest() {
    let run = LocalRun {
        id: "run-1700000000000".to_string(),
        pbi_id: 555,
        started_at: "1700000000000".to_string(),
        cases: vec![
            CaseRecord {
                case_id: 7,
                title: "Leave request".to_string(),
                verdict: "Failed".to_string(),
                note: "Save is missing on the new form".to_string(),
                steps: vec![
                    StepRecord {
                        step_number: 2,
                        outcomes: vec![ActionOutcome {
                            ok: false,
                            detail: "button \"Save\" not found".to_string(),
                            screenshot: Some("shot-1-000002.jpg".to_string()),
                            harness: false,
                            component: None,
                            ..Default::default()
                        }],
                        screenshot: None,
                        downloads: vec![],
                        tab: None,
                        dialog: None,
                        components: Vec::new(), duration_ms: None,
                    },
                    StepRecord {
                        step_number: 3,
                        outcomes: vec![ActionOutcome::failed("not run: an earlier step of this case failed")],
                        screenshot: None,
                        downloads: vec![],
                        tab: None,
                        dialog: None,
                        components: Vec::new(), duration_ms: None,
                    },
                ],
                proposed: "Failed".to_string(),
                reason: "step 2: button \"Save\" not found".to_string(),
                duration_ms: Some(1234),
                account: Some("hr.admin".to_string()),
                retried: None,
                notice: None,
                page_errors_seen: 0, phases: None, cleanup: None,
            },
            CaseRecord {
                case_id: 8,
                title: "Something else".to_string(),
                verdict: String::new(),
                note: String::new(),
                steps: vec![StepRecord {
                    step_number: 1,
                    outcomes: vec![ActionOutcome {
                        ok: false,
                        detail: "the page refused: no such element".to_string(),
                        screenshot: None,
                        harness: false,
                        component: None,
                        ..Default::default()
                    }],
                    screenshot: None,
                    downloads: vec![],
                    tab: None,
                    dialog: None,
                    components: Vec::new(), duration_ms: None,
                }],
                proposed: "Failed".to_string(),
                reason: "step 1: the page refused: no such element".to_string(),
                duration_ms: Some(500),
                account: None,
                retried: None,
                notice: None,
                page_errors_seen: 0, phases: None, cleanup: None,
            },
            CaseRecord {
                case_id: 9,
                title: "Passed case, never mentioned".to_string(),
                verdict: "Passed".to_string(),
                note: String::new(),
                steps: vec![],
                proposed: "Passed".to_string(),
                reason: "every action of 1 steps passed".to_string(),
                duration_ms: Some(200),
                account: None,
                retried: None,
                notice: None,
                page_errors_seen: 0, phases: None, cleanup: None,
            },
        ],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };

    let scripts = vec![CaseScript {
        case_id: 7,
        title: "Leave request".to_string(),
        account: Some("hr.admin".to_string()),
        area: None,
        steps: vec![
            StepScript { step_number: 2, actions: vec![save_button()], unchecked: None, cleanup: false },
            StepScript {
                step_number: 3,
                actions: vec![Action::Click { selector: "text=Confirm".into() }],
                unchecked: None,
                cleanup: false,
            },
        ],
        repairs: 1,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        needs_state: Default::default(),
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None, ignore_page_errors: vec![], organization: None, project: None, checked: false,
    }];

    let expected = [
        "## Case 7 \"Leave request\" (run run-1700000000000, proposed Failed, verdict Failed)",
        "account: hr.admin",
        "step 2, action 1: { \"kind\": \"click\", \"selector\": { \"role\": \"button\", \"name\": \"Save\" } }",
        "  page said: button \"Save\" not found",
        "  picture: shot-1-000002.jpg",
        "step 3: not run (an earlier step of this case failed)",
        "note from the person: Save is missing on the new form",
        "",
        "## Case 8 \"Something else\" (run run-1700000000000, proposed Failed)",
        "step 1, action 1: script: not on this machine",
        "  page said: the page refused: no such element",
    ]
    .join("\n");

    assert_eq!(describe_failures(&run, &scripts), expected);
}

#[test]
fn describe_failures_names_the_run_when_no_case_failed() {
    let run = LocalRun {
        id: "run-1".to_string(),
        pbi_id: 1,
        started_at: "1".to_string(),
        cases: vec![CaseRecord { proposed: "Passed".to_string(), verdict: "Passed".to_string(), ..empty_case() }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    assert_eq!(describe_failures(&run, &[]), "no failed case in run run-1");
}

#[test]
fn describe_failures_shows_the_stop_line_only_when_stop_reason_is_some() {
    let run = LocalRun {
        id: "run-2".to_string(),
        pbi_id: 1,
        started_at: "2".to_string(),
        cases: vec![CaseRecord {
            verdict: "Blocked".to_string(),
            proposed: "Blocked".to_string(),
            reason: "a precondition was missing".to_string(),
            steps: vec![StepRecord {
                step_number: 1,
                outcomes: vec![ActionOutcome::failed("the field never appeared")],
                screenshot: None,
                downloads: vec![],
                tab: None,
                dialog: None,
                components: Vec::new(), duration_ms: None,
            }],
            ..empty_case()
        }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let out = describe_failures(&run, &[]);
    assert!(
        out.contains("STOP: the person marked this case Blocked - a missing precondition is not a script defect")
    );
}

#[test]
fn describe_failures_masks_a_fill_value_but_never_the_other_fields() {
    let run = LocalRun {
        id: "run-3".to_string(),
        pbi_id: 1,
        started_at: "3".to_string(),
        cases: vec![CaseRecord {
            proposed: "Failed".to_string(),
            steps: vec![StepRecord {
                step_number: 1,
                outcomes: vec![ActionOutcome::failed("value shown was wrong")],
                screenshot: None,
                downloads: vec![],
                tab: None,
                dialog: None,
                components: Vec::new(), duration_ms: None,
            }],
            ..empty_case()
        }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let scripts = vec![CaseScript {
        case_id: 1,
        title: "A case".to_string(),
        account: None,
        area: None,
        steps: vec![StepScript {
            step_number: 1,
            actions: vec![Action::Fill { selector: "#password".into(), value: "correct horse battery staple".to_string() }],
            unchecked: None,
            cleanup: false,
        }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        needs_state: Default::default(),
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None, ignore_page_errors: vec![], organization: None, project: None, checked: false,
    }];

    let out = describe_failures(&run, &scripts);
    assert!(out.contains("\"kind\": \"fill\""));
    assert!(out.contains("\"selector\": \"#password\""));
    assert!(out.contains("\"value\": \"...\""));
    assert!(!out.contains("correct horse battery staple"));
}

#[test]
fn describe_failures_omits_proposed_when_proposed_is_empty() {
    // A supervised run never fills in `proposed` - only a person's own
    // verdict. The header must not print an empty "proposed " for it.
    let run = LocalRun {
        id: "run-1".to_string(),
        pbi_id: 1,
        started_at: "1".to_string(),
        cases: vec![CaseRecord { verdict: "Failed".to_string(), proposed: String::new(), ..empty_case() }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let out = describe_failures(&run, &[]);
    assert!(out.starts_with("## Case 1 \"A case\" (run run-1, verdict Failed)"));
    assert!(!out.contains("proposed"));
}

#[test]
fn describe_failures_prints_not_run_outcomes_in_a_mixed_step() {
    // A step can carry a real failure alongside a "not run:" outcome (an
    // action skipped because an earlier action in the SAME step already
    // failed) - that is not the whole-step "not run" case, so the not-run
    // outcome must still be shown, not silently dropped.
    let run = LocalRun {
        id: "run-1".to_string(),
        pbi_id: 1,
        started_at: "1".to_string(),
        cases: vec![CaseRecord {
            proposed: "Failed".to_string(),
            steps: vec![StepRecord {
                step_number: 1,
                outcomes: vec![
                    ActionOutcome::failed("button \"Save\" not found"),
                    ActionOutcome::failed("not run: an earlier action in this step failed"),
                ],
                screenshot: None,
                downloads: vec![],
                tab: None,
                dialog: None,
                components: Vec::new(), duration_ms: None,
            }],
            ..empty_case()
        }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let out = describe_failures(&run, &[]);
    assert!(out.contains("step 1, action 1: script: not on this machine"));
    assert!(out.contains("  page said: button \"Save\" not found"));
    assert!(out.contains("  action 2: not run (an earlier action in this step failed)"));
}

#[test]
fn describe_failures_says_the_script_changed_when_the_action_index_is_gone() {
    // The script for this case IS on disk, but it no longer has an action
    // at this index (a person edited it after this run happened) - that is
    // a different situation from no script at all, and must say so.
    let run = LocalRun {
        id: "run-1".to_string(),
        pbi_id: 1,
        started_at: "1".to_string(),
        cases: vec![CaseRecord {
            proposed: "Failed".to_string(),
            steps: vec![StepRecord {
                step_number: 1,
                outcomes: vec![ActionOutcome::failed("button \"Save\" not found")],
                screenshot: None,
                downloads: vec![],
                tab: None,
                dialog: None,
                components: Vec::new(), duration_ms: None,
            }],
            ..empty_case()
        }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let scripts = vec![CaseScript {
        case_id: 1,
        title: "A case".to_string(),
        account: None,
        area: None,
        steps: vec![StepScript { step_number: 1, actions: vec![], unchecked: None, cleanup: false }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        needs_state: Default::default(),
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None, ignore_page_errors: vec![], organization: None, project: None, checked: false,
    }];
    let out = describe_failures(&run, &scripts);
    assert!(out
        .contains("step 1, action 1: script: no action 1 on this machine (the script changed since the run)"));
}

#[test]
fn latest_run_picks_by_case_id_and_falls_back_to_the_newest_run_overall() {
    let dir = TempDir::new();
    let older = LocalRun {
        id: "run-1000".to_string(),
        pbi_id: 1,
        started_at: "1000".to_string(),
        cases: vec![minimal_case(5)],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let newer = LocalRun {
        id: "run-2000".to_string(),
        pbi_id: 1,
        started_at: "2000".to_string(),
        cases: vec![minimal_case(6)],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    save_run(dir.path(), &older).unwrap();
    save_run(dir.path(), &newer).unwrap();

    assert_eq!(latest_run(dir.path(), None).unwrap().id, "run-2000");
    assert_eq!(latest_run(dir.path(), Some(5)).unwrap().id, "run-1000");
    assert_eq!(latest_run(dir.path(), Some(6)).unwrap().id, "run-2000");
    assert!(latest_run(dir.path(), Some(999)).is_none());
}

#[test]
fn a_case_the_run_could_not_take_to_its_module_is_not_a_script_defect() {
    let unreached = "Could not reach module \"Leave\": click 2, link \"Apply Leave\" - waited 300ms: link \"Apply Leave\" not found.";
    let case = CaseRecord {
        proposed: "Blocked".to_string(),
        reason: unreached.to_string(),
        steps: vec![StepRecord { step_number: MODULE_STEP, outcomes: vec![ActionOutcome::failed(unreached)], screenshot: None, downloads: vec![], tab: None, dialog: None, components: Vec::new(), duration_ms: None }],
        ..empty_case()
    };
    let expected = Some(
        "the run could not take this case to its module screen - fix the module path or the case's Module in the app, not the script"
            .to_string(),
    );
    assert_eq!(stop_reason(&case), expected);
    let run = LocalRun {
        id: "run-1".into(),
        pbi_id: 1,
        started_at: "1".into(),
        cases: vec![case],
        mode: "unattended".into(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let out = describe_failures(&run, &[]);
    assert!(out.contains(&format!("module: {unreached}")), "{out}");
    assert!(!out.contains("step -1"), "{out}");

    let no_path_case = CaseRecord { proposed: "Blocked".to_string(), reason: no_path("Payroll"), ..empty_case() };
    assert_eq!(stop_reason(&no_path_case), expected);
}

/// Review I1: a page failure whose words read like the runner's sentence is
/// still the script's to look at.
#[test]
fn a_failed_check_that_quotes_the_unreached_sentence_is_not_a_setup_problem() {
    let reason = "step 1: page does NOT contain Could not reach module \"Payroll\": click 2, link \"Pay\" - gone.";
    let case = CaseRecord { proposed: "Failed".to_string(), reason: reason.to_string(), ..empty_case() };
    assert_eq!(stop_reason(&case), None);
}

/// Review of Task 8: an assistant repairing a script sees the area it
/// starts in, under the account - and a script with no area says nothing.
#[test]
fn describe_failures_names_the_scripts_area_when_it_has_one() {
    let case = CaseRecord {
        verdict: "Failed".to_string(),
        proposed: "Failed".to_string(),
        reason: "step 1: button \"Save\" not found".to_string(),
        account: Some("hr.admin".to_string()),
        steps: vec![StepRecord {
            step_number: 1,
            outcomes: vec![ActionOutcome::failed("button \"Save\" not found")],
            screenshot: None,
            downloads: vec![],
            tab: None,
            dialog: None,
            components: Vec::new(), duration_ms: None,
        }],
        ..empty_case()
    };
    let run = LocalRun {
        id: "run-1700000000000".to_string(),
        pbi_id: 555,
        started_at: "1700000000000".to_string(),
        cases: vec![case],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let mut script = CaseScript {
        case_id: 1,
        title: "A case".to_string(),
        account: Some("hr.admin".to_string()),
        area: Some("Manage Cycle".to_string()),
        steps: vec![StepScript { step_number: 1, actions: vec![save_button()], unchecked: None, cleanup: false }],
        repairs: 0,
        last_repair: None,
        suspected_defect: None,
        no_save: false,
        preconditions: vec![],
        setup: None,
        changes: vec![],
        needs_unchanged: vec![],
        needs_state: Default::default(),
        saved_at: None,
        fail_on_unexpected_dialog: false,
        page_errors: None, ignore_page_errors: vec![], organization: None, project: None, checked: false,
    };
    let text = describe_failures(&run, std::slice::from_ref(&script));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1..3], ["account: hr.admin", "area: Manage Cycle"], "{text}");
    // Repairs have no limit, so the count is not something to act on.
    assert!(!text.contains("repairs so far"), "{text}");

    for none in [None, Some("  ".to_string())] {
        script.area = none;
        let text = describe_failures(&run, std::slice::from_ref(&script));
        assert!(!text.contains("area:"), "{text}");
    }
}

#[test]
fn describe_failures_names_each_steps_downloads_and_nothing_more() {
    let run = LocalRun {
        id: "run-dl".to_string(),
        pbi_id: 1,
        started_at: "4".to_string(),
        cases: vec![CaseRecord {
            proposed: "Failed".to_string(),
            steps: vec![
                StepRecord {
                    step_number: 1,
                    outcomes: vec![ActionOutcome::passed("clicked button \"Export\"")],
                    screenshot: None,
                    downloads: vec!["Template.xlsx".to_string()],
                    tab: None,
                    dialog: None,
                    components: Vec::new(), duration_ms: None,
                },
                StepRecord {
                    step_number: 2,
                    outcomes: vec![ActionOutcome::failed("the download Template.xlsx has no header Email")],
                    screenshot: None,
                    downloads: vec!["errors.csv".to_string(), "errors (2).csv".to_string()],
                    tab: None,
                    dialog: None,
                    components: Vec::new(), duration_ms: None,
                },
                StepRecord {
                    step_number: 3,
                    outcomes: vec![ActionOutcome::passed("ok")],
                    screenshot: None,
                    downloads: vec![],
                    tab: None,
                    dialog: None,
                    components: Vec::new(), duration_ms: None,
                },
            ],
            ..empty_case()
        }],
        mode: "unattended".to_string(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let out = describe_failures(&run, &[]);
    assert!(out.contains("step 1 downloads: Template.xlsx"), "{out}");
    assert!(out.contains("step 2 downloads: errors.csv, errors (2).csv"), "{out}");
    // The failed action's lines come first, the step's files after them.
    let failed_at = out.find("  page said: the download Template.xlsx").unwrap();
    assert!(failed_at < out.find("step 2 downloads:").unwrap(), "{out}");
    assert!(!out.contains("step 3 downloads"), "{out}");
    // Names only: never the folder they are in.
    assert!(!out.contains("downloads\\") && !out.contains("downloads/"), "{out}");
}

// ---- components ----

mod component_steps {
    use crate::common::{component_case, component_run, component_script, pick_a_date_file, COMPONENT_TYPED};
    use v2_lib::autorun::components::ComponentFile;
    use v2_lib::autorun::failures::{describe_failures, describe_failures_with};

    const TYPING: &str = "#start cannot be typed into";

    #[test]
    fn a_failure_inside_a_component_names_the_component_and_its_action() {
        let run = component_run(vec![component_case(7, TYPING)]);
        let text = describe_failures_with(&run, &[component_script(7)], &pick_a_date_file());
        assert!(
            text.contains(
                "step 3, Pick a date, action 3: { \"kind\": \"fill\", \"selector\": { \"css\": \"#start\" }, \"value\": \"...\" }\n  page said: #start cannot be typed into"
            ),
            "{text}"
        );
        // The script's own action after the component lines up with it.
        assert!(text.contains("step 3, action 5: { \"kind\": \"check_text\", \"value\": \"Saved\" }"), "{text}");
        assert!(!text.contains("script changed"), "{text}");
        assert!(!text.contains(COMPONENT_TYPED), "{text}");

        // Without the components (gone, or not read), the action is named
        // by its component, and the script is still not said to have
        // changed.
        for text in [
            describe_failures(&run, &[component_script(7)]),
            describe_failures_with(&run, &[component_script(7)], &ComponentFile::default()),
        ] {
            assert!(text.contains("step 3, Pick a date, action 3: component:"), "{text}");
            assert!(text.contains("the component changed since the run"), "{text}");
            assert!(text.contains("step 3, action 5: { \"kind\": \"check_text\", \"value\": \"Saved\" }"), "{text}");
            assert!(!text.contains("script changed"), "{text}");
        }
    }

    #[test]
    fn a_component_text_input_is_masked_in_the_failure_report() {
        // A step whose component could not be used: the use itself is the
        // failed action, so its JSON is shown.
        let script: v2_lib::autorun::CaseScript = serde_json::from_value(serde_json::json!({
            "case_id": 8, "title": "case 8",
            "steps": [{ "step_number": 1, "actions": [
                { "kind": "use_component", "component": "Pick a date",
                  "inputs": { "field": { "css": "#start" }, "day": COMPONENT_TYPED, "count": 42 } },
                { "kind": "when_visible", "selector": { "css": "#popup" }, "then": [
                    { "kind": "use_component", "component": "Pick a date",
                      "inputs": { "field": [{ "css": "#grid" }, { "text": "Row" }], "day": COMPONENT_TYPED } }
                ] }
            ] }]
        }))
        .unwrap();
        let mut case = component_case(8, "x");
        case.steps = serde_json::from_value(serde_json::json!([{ "step_number": 1, "outcomes": [
            { "ok": false, "detail": "Pick a date is not saved in this project" },
            { "ok": false, "detail": "#popup showed: Pick a date is not saved in this project" }
        ] }]))
        .unwrap();
        let text = describe_failures_with(&component_run(vec![case]), &[script], &ComponentFile::default());
        assert!(!text.contains(COMPONENT_TYPED), "{text}");
        assert!(!text.contains("42"), "{text}");
        assert!(
            text.contains("\"inputs\": { \"field\": { \"css\": \"#start\" }, \"day\": \"...\", \"count\": \"...\" }"),
            "{text}"
        );
        assert!(text.contains("\"field\": [{ \"css\": \"#grid\" }, { \"text\": \"Row\" }]"), "{text}");
    }
}

/// With the store known, a failure line names the picture by the full path
/// of a file that is there; the run record itself keeps the bare name.
#[test]
fn a_failure_line_names_the_picture_by_a_path_that_exists() {
    let dir = TempDir::new();
    let name = v2_lib::autorun::store::save_shot(dir.path(), b"jpeg bytes").unwrap();
    let run = LocalRun {
        id: "run-1".to_string(),
        pbi_id: 1,
        started_at: "1".to_string(),
        cases: vec![CaseRecord {
            case_id: 7,
            title: "Leave request".to_string(),
            verdict: "Failed".to_string(),
            steps: vec![StepRecord {
                step_number: 1,
                outcomes: vec![ActionOutcome {
                    ok: false,
                    detail: "button \"Save\" not found".to_string(),
                    screenshot: Some(name.clone()),
                    harness: false,
                    component: None,
                    ..Default::default()
                }],
                screenshot: None,
                downloads: vec![],
                tab: None,
                dialog: None,
                components: Vec::new(),
                duration_ms: None,
            }],
            ..empty_case()
        }],
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    };
    let out = v2_lib::autorun::failures::describe_failures_in(
        Some(dir.path()),
        &run,
        &[],
        &v2_lib::autorun::components::ComponentFile::default(),
    );
    let line = out.lines().find_map(|l| l.strip_prefix("  picture: ")).unwrap_or_else(|| panic!("no picture line in {out}"));
    let path = std::path::Path::new(line);
    assert!(path.is_absolute(), "{line}");
    assert!(path.is_file(), "{line}");
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), name);
    assert_eq!(run.cases[0].steps[0].outcomes[0].screenshot.as_deref(), Some(name.as_str()));
}

const HOUR: u64 = 60 * 60 * 1000;
/// "Now" for the recent-runs tests: 1970-01-05T04:00:00Z.
const NOW: u64 = 100 * HOUR;

/// Run `id` started `ago` before `NOW`, its cases as (id, verdict,
/// proposed).
fn run_ago(id: &str, ago: u64, cases: &[(i32, &str, &str)]) -> LocalRun {
    LocalRun {
        id: id.to_string(),
        pbi_id: 5,
        started_at: (NOW - ago).to_string(),
        cases: cases
            .iter()
            .map(|(case_id, verdict, proposed)| CaseRecord {
                verdict: verdict.to_string(),
                proposed: proposed.to_string(),
                ..minimal_case(*case_id)
            })
            .collect(),
        mode: String::new(),
        published: None,
        environment: None,
        resets: vec![],
        network_pauses: vec![],
    }
}

/// The other recent runs with failed or blocked cases: of the 5 newest
/// that are not the one read, those from the last 24 hours. A person's
/// verdict outranks the machine's proposal.
#[test]
fn recent_failing_takes_the_smaller_of_five_runs_and_a_day() {
    use v2_lib::autorun::failures::{failed_and_blocked, recent_failing};
    let runs = vec![
        run_ago("run-read", 0, &[(1, "", "Failed")]),
        run_ago("run-a", HOUR, &[(1, "Failed", ""), (2, "Passed", "")]),
        run_ago("run-b", 2 * HOUR, &[(1, "Passed", "Failed")]),
        run_ago("run-c", 3 * HOUR, &[(1, "", "Blocked")]),
        run_ago("run-d", 25 * HOUR, &[(1, "Failed", "")]),
    ];
    let ids: Vec<&str> = recent_failing(&runs, "run-read", NOW).iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["run-a", "run-c"]);
    assert_eq!(failed_and_blocked(&runs[1]), (1, 0));
    assert_eq!(failed_and_blocked(&runs[2]), (0, 0));
    assert_eq!(failed_and_blocked(&runs[3]), (0, 1));

    let mut many = vec![run_ago("run-read", 0, &[])];
    for n in 1..=7u64 {
        many.push(run_ago(&format!("run-{n}"), n * HOUR, &[(1, "Failed", "")]));
    }
    let ids: Vec<&str> = recent_failing(&many, "run-read", NOW).iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, vec!["run-1", "run-2", "run-3", "run-4", "run-5"]);
}

/// Asked of no case and no run, the answer names the run it read and ends
/// with the other recent runs that failed or blocked, each readable by id.
#[test]
fn newest_run_context_names_the_run_read_and_the_others() {
    use v2_lib::autorun::failures::newest_run_context;
    let leave: CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": 1, "title": "t", "area": "Leave", "steps": []
    }))
    .unwrap();
    let runs = vec![
        run_ago("run-read", 0, &[(1, "Passed", ""), (2, "Passed", "")]),
        run_ago("run-a", HOUR, &[(1, "Failed", ""), (3, "Blocked", "")]),
    ];
    let (head, others) = newest_run_context(&runs[0], std::slice::from_ref(&leave), &runs, NOW);
    assert_eq!(
        head,
        "Read run run-read (started 1970-01-05T04:00:00Z, PBI #5, 2 cases, area Leave): the newest on this machine. Ask with run_id to read another."
    );
    assert_eq!(
        others.as_deref(),
        Some(
            "Other recent runs on this machine with failed or blocked cases (read one with run_id):\n- run run-a (started 1970-01-05T03:00:00Z, PBI #5, 2 cases, area Leave): 1 failed, 1 blocked - run_id=run-a"
        )
    );
    let (_, none) = newest_run_context(&runs[1], &[], &runs[1..], NOW);
    assert_eq!(none, None);
}

/// #46: a failed case whose clean-up failed too says so, after its steps.
#[test]
fn a_failed_clean_up_is_said_after_the_failed_steps() {
    let run: LocalRun = serde_json::from_value(serde_json::json!({
        "id": "run-1", "pbi_id": 1, "started_at": "1", "cases": [{
            "case_id": 5, "title": "Add a rating", "verdict": "", "note": "",
            "proposed": "Failed", "reason": "step 1: button \"Save\" not found",
            "steps": [{ "step_number": 1, "outcomes": [{ "ok": false, "detail": "button \"Save\" not found" }] }],
            "cleanup": { "step_number": 2, "outcomes": [{ "ok": false, "detail": "button \"Delete\" not found" }] }
        }]
    }))
    .unwrap();
    let out = describe_failures(&run, &[]);
    assert!(
        out.ends_with("clean-up (it never fails the case), action 1: script: not on this machine\n  page said: button \"Delete\" not found"),
        "{out}"
    );
}

/// #46 review: a case that passed but whose clean-up failed is listed too,
/// for its clean-up only, with the action and its picture as a failed
/// step's are.
#[test]
fn a_passed_case_whose_clean_up_failed_is_listed_for_its_clean_up() {
    let run: LocalRun = serde_json::from_value(serde_json::json!({
        "id": "run-1", "pbi_id": 1, "started_at": "1", "cases": [{
            "case_id": 5, "title": "Add a rating", "verdict": "", "note": "",
            "proposed": "Passed", "reason": "every action of 1 steps passed, but its clean-up failed",
            "steps": [{ "step_number": 1, "outcomes": [{ "ok": true, "detail": "ok" }] }],
            "cleanup": { "step_number": 2, "outcomes": [{ "ok": false, "detail": "button \"Delete\" not found", "screenshot": "shot-1.jpg" }] }
        }]
    }))
    .unwrap();
    let script: CaseScript = serde_json::from_value(serde_json::json!({
        "case_id": 5, "title": "Add a rating", "steps": [
            { "step_number": 1, "actions": [{ "kind": "check_text", "value": "ok" }] },
            { "step_number": 2, "actions": [{ "kind": "click", "selector": "#delete" }], "cleanup": true, "unchecked": "tidy" }
        ]
    }))
    .unwrap();
    assert_eq!(
        describe_failures(&run, &[script]),
        [
            "## Case 5 \"Add a rating\" (run run-1, proposed Passed)",
            "the case passed; only its clean-up failed",
            "clean-up (it never fails the case), action 1: { \"kind\": \"click\", \"selector\": \"#delete\" }",
            "  page said: button \"Delete\" not found",
            "  picture: shot-1.jpg",
        ]
        .join("\n")
    );
}
