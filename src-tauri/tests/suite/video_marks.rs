//! The timing data behind a video's markers: the types, their old-record
//! compatibility, the labels (never a secret), the failed flag and the
//! timeline the player reads.

use serde_json::{json, Value};
use std::time::{Duration, Instant};
use v2_lib::autorun::recordings::RecordingEntry;
use v2_lib::autorun::video_marks::{
    case_timeline, mark_failures, mark_label, offset_ms, FILL_FORM_LABEL, SHOT_LABEL,
};
use v2_lib::autorun::{action_help, CaseRecord, StepRecord, VideoMark, VideoMarks, VideoStep};
use v2_lib::browser::actions::Action;
use v2_lib::browser::highlight::{TYPE_HIDDEN, TYPE_PASSWORD};

fn step(step_number: i32, at_ms: u32) -> VideoStep {
    VideoStep { step_number, cleanup: false, at_ms, failed: false, actions: vec![] }
}

fn record(step_number: i32, details: &[(bool, &str)]) -> StepRecord {
    serde_json::from_value(json!({
        "step_number": step_number,
        "outcomes": details.iter().map(|(ok, d)| json!({ "ok": ok, "detail": d })).collect::<Vec<_>>(),
    }))
    .unwrap()
}

#[test]
fn old_case_records_and_recording_entries_load_without_marks() {
    let case: CaseRecord = serde_json::from_value(json!({ "case_id": 1, "title": "t", "verdict": "", "note": "", "steps": [], "video": "video-1-1.mp4" })).unwrap();
    assert_eq!(case.video_marks, None);
    assert!(!serde_json::to_string(&case).unwrap().contains("video_marks"));
    let e: RecordingEntry = serde_json::from_value(json!({ "id": "r", "title": "t", "created": "c", "duration_ms": 5, "file": "recording-1-1.mp4" })).unwrap();
    assert!(e.marks.is_empty());
    assert!(!serde_json::to_string(&e).unwrap().contains("marks"));
}

#[test]
fn marks_write_only_what_they_hold() {
    let m = VideoMarks { length_ms: 90_000, steps: vec![step(1, 0)] };
    assert_eq!(serde_json::to_value(&m).unwrap(), json!({ "length_ms": 90000, "steps": [{ "step_number": 1, "at_ms": 0 }] }));
    let full = VideoStep { cleanup: true, failed: true, actions: vec![VideoMark { at_ms: 5, label: "Click".into() }], ..step(2, 7) };
    let back: VideoStep = serde_json::from_value(serde_json::to_value(&full).unwrap()).unwrap();
    assert_eq!(back, full);
}

#[test]
fn an_offset_is_measured_from_the_recording_start_and_never_negative() {
    let t0 = Instant::now();
    assert_eq!(offset_ms(t0, t0 + Duration::from_millis(1500)), 1500);
    assert_eq!(offset_ms(t0 + Duration::from_millis(10), t0), 0);
}

#[test]
fn a_mark_label_is_the_highlight_label_and_never_a_secret() {
    let a = |v: Value| -> Action { serde_json::from_value(v).unwrap() };
    let fill = a(json!({ "kind": "fill", "selector": "#surname", "value": "Smith" }));
    assert_eq!(mark_label(&fill, Some("Type 'Smith'")), "Type 'Smith'");
    assert_eq!(mark_label(&fill, None), TYPE_PASSWORD, "never drawn, so it may have been a password field");
    assert_eq!(mark_label(&a(json!({ "kind": "fill", "selector": "#a", "value": "x", "sensitive": true })), None), TYPE_HIDDEN);
    assert_eq!(mark_label(&a(json!({ "kind": "click", "selector": "#a" })), None), "Click");
    assert_eq!(mark_label(&a(json!({ "kind": "check_text", "value": "Saved" })), None), "Check: shows 'Saved'");
    assert_eq!(mark_label(&a(json!({ "kind": "navigate", "url": "https://app.test/x?token=abc" })), None), "Open a page");
    assert_eq!(mark_label(&a(json!({ "kind": "sign_in", "account": "admin" })), None), "Sign in");
    let form = action_help::fill_form_from(&json!({ "kind": "fill_form", "fields": { "#pw": "hunter2" } })).unwrap().unwrap();
    assert_eq!(mark_label(&form, None), FILL_FORM_LABEL);
    assert_eq!(mark_label(&action_help::screenshot_action(false), None), SHOT_LABEL);
}

#[test]
fn mark_failures_marks_the_steps_that_failed() {
    let mut marks = VideoMarks {
        length_ms: 10_000,
        steps: vec![step(1, 0), step(2, 1000), step(3, 2000), VideoStep { cleanup: true, ..step(4, 3000) }],
    };
    let steps = [
        record(1, &[(true, "ok")]),
        record(2, &[(true, "ok"), (false, "the field never appeared")]),
        record(3, &[(false, "not run: an earlier step of this case failed")]),
    ];
    let cleanup = record(4, &[(false, "could not delete it")]);
    mark_failures(&mut marks, &steps, Some(&cleanup));
    let flags: Vec<bool> = marks.steps.iter().map(|s| s.failed).collect();
    assert_eq!(flags, [false, true, false, true]);
    // No clean-up record: the clean-up step is not failed.
    mark_failures(&mut marks, &steps, None);
    assert!(!marks.steps[3].failed);
}

#[test]
fn the_case_timeline_names_its_steps_like_the_review() {
    let mut case: CaseRecord = serde_json::from_value(json!({ "case_id": 7, "title": "Leave request", "verdict": "", "note": "", "steps": [] })).unwrap();
    let plain = case_timeline(&case);
    assert_eq!(plain.title, "Video of #7 Leave request");
    assert_eq!(plain.length_ms, None);
    assert!(plain.steps.is_empty() && plain.marks.is_empty());

    case.video_marks = Some(VideoMarks {
        length_ms: 20_000,
        steps: vec![
            step(v2_lib::autorun::replay::SIGN_IN_STEP, 0),
            step(v2_lib::autorun::replay::MODULE_STEP, 500),
            VideoStep { failed: true, actions: vec![VideoMark { at_ms: 900, label: "Click".into() }], ..step(1, 900) },
            VideoStep { cleanup: true, ..step(2, 5000) },
        ],
    });
    let t = case_timeline(&case);
    assert_eq!(t.length_ms, Some(20_000));
    let names: Vec<&str> = t.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["Sign in", "Module", "Step 1", "Clean-up"]);
    assert_eq!(t.steps.iter().map(|s| s.failed).collect::<Vec<_>>(), [false, false, true, false]);
    assert_eq!(t.steps[2].actions[0].label, "Click");
    assert_eq!(t.steps[2].at_ms, 900);
    assert!(t.marks.is_empty());
}
