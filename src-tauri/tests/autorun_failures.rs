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
    }
}

#[test]
fn stop_reason_is_some_when_the_sign_in_step_failed() {
    let case = CaseRecord {
        steps: vec![StepRecord {
            step_number: 0,
            outcomes: vec![ActionOutcome::failed("wrong password")],
            screenshot: None,
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
    let contains_did_not_answer = CaseRecord {
        reason: "step 3: waited 5000ms but the browser did not answer".to_string(),
        ..empty_case()
    };
    let expected = Some("the browser stopped answering - rerun before changing anything".to_string());
    assert_eq!(stop_reason(&starts_with_the_browser), expected);
    assert_eq!(stop_reason(&contains_did_not_answer), expected);
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
                        }],
                        screenshot: None,
                    },
                    StepRecord {
                        step_number: 3,
                        outcomes: vec![ActionOutcome::failed("not run: an earlier step of this case failed")],
                        screenshot: None,
                    },
                ],
                proposed: "Failed".to_string(),
                reason: "step 2: button \"Save\" not found".to_string(),
                duration_ms: Some(1234),
                account: Some("hr.admin".to_string()),
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
                    }],
                    screenshot: None,
                }],
                proposed: "Failed".to_string(),
                reason: "step 1: the page refused: no such element".to_string(),
                duration_ms: Some(500),
                account: None,
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
            },
        ],
        mode: String::new(),
        published: None,
    };

    let scripts = vec![CaseScript {
        case_id: 7,
        title: "Leave request".to_string(),
        account: Some("hr.admin".to_string()),
        steps: vec![
            StepScript { step_number: 2, actions: vec![save_button()], unchecked: None },
            StepScript {
                step_number: 3,
                actions: vec![Action::Click { selector: "text=Confirm".into() }],
                unchecked: None,
            },
        ],
        repairs: 1,
        last_repair: None,
    }];

    let expected = [
        "## Case 7 \"Leave request\" (run run-1700000000000, proposed Failed, verdict Failed)",
        "account: hr.admin",
        "repairs so far: 1 of 3",
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
            }],
            ..empty_case()
        }],
        mode: String::new(),
        published: None,
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
            }],
            ..empty_case()
        }],
        mode: String::new(),
        published: None,
    };
    let scripts = vec![CaseScript {
        case_id: 1,
        title: "A case".to_string(),
        account: None,
        steps: vec![StepScript {
            step_number: 1,
            actions: vec![Action::Fill { selector: "#password".into(), value: "correct horse battery staple".to_string() }],
            unchecked: None,
        }],
        repairs: 0,
        last_repair: None,
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
            }],
            ..empty_case()
        }],
        mode: String::new(),
        published: None,
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
            }],
            ..empty_case()
        }],
        mode: String::new(),
        published: None,
    };
    let scripts = vec![CaseScript {
        case_id: 1,
        title: "A case".to_string(),
        account: None,
        steps: vec![StepScript { step_number: 1, actions: vec![], unchecked: None }],
        repairs: 0,
        last_repair: None,
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
    };
    let newer = LocalRun {
        id: "run-2000".to_string(),
        pbi_id: 1,
        started_at: "2000".to_string(),
        cases: vec![minimal_case(6)],
        mode: String::new(),
        published: None,
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
        steps: vec![StepRecord { step_number: MODULE_STEP, outcomes: vec![ActionOutcome::failed(unreached)], screenshot: None }],
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
