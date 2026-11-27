//! The markers on a video: how far into it each step and action began, and
//! the words that name them. Offsets are measured from the recorder's own
//! start (`browser::recording::Recording::started`), read once per step and
//! once per action, never summed from durations. A label is what the step
//! highlight says (`browser::highlight::Say`), or the label it actually
//! drew; it is never the typed value of a secret.

use super::{CaseRecord, StepRecord, VideoMark, VideoMarks};
use crate::browser::actions::Action;
use crate::browser::highlight::Say;
use serde::Serialize;
use std::time::Instant;

/// What a `fill_form` step is called: its fields are never listed.
pub const FILL_FORM_LABEL: &str = "Fill the form";
/// What a screenshot step is called.
pub const SHOT_LABEL: &str = "Take a picture";

/// Milliseconds from `start` to `at`: zero when `at` is earlier, and never
/// more than `u32::MAX`.
pub fn offset_ms(start: Instant, at: Instant) -> u32 {
    u32::try_from(at.saturating_duration_since(start).as_millis()).unwrap_or(u32::MAX)
}

/// The marker's words for `action`. `drawn` is the label the highlight drew
/// over the element, when it drew one. Without it the highlight's own label
/// is used, and where that label depends on the element being a password
/// field (which was never looked at) the hidden one, so a secret cannot leak.
pub fn mark_label(action: &Action, drawn: Option<&str>) -> String {
    if super::action_help::fill_form_ask(action).is_some() {
        return FILL_FORM_LABEL.to_string();
    }
    if super::action_help::screenshot_ask(action).is_some() {
        return SHOT_LABEL.to_string();
    }
    // An action with no element of its own runs other actions nested in it
    // (a sign-in's clicks and typing): the box drawn is the nested one's, so
    // the marker keeps the action's own name.
    if !has_no_element(action) {
        if let Some(label) = drawn {
            return label.to_string();
        }
    }
    let say = Say::of(action);
    say.if_password.unwrap_or(say.label)
}

/// Is `action` one with no element of its own, whose highlight (if any) is
/// that of an action nested in it?
fn has_no_element(action: &Action) -> bool {
    matches!(
        action,
        Action::SignIn { .. }
            | Action::ReturnToArea { .. }
            | Action::UseComponent { .. }
            | Action::WhenVisible { .. }
            | Action::WhenHidden { .. }
            | Action::RepeatWhileVisible { .. }
    )
}

/// Flag the steps of `marks` whose record has an outcome that failed after
/// it was run (a "not run:" outcome is not the step's own failure). A step
/// of the case is matched to `steps` by its number, the clean-up step to
/// `cleanup`.
pub fn mark_failures(marks: &mut VideoMarks, steps: &[StepRecord], cleanup: Option<&StepRecord>) {
    for step in &mut marks.steps {
        let record =
            if step.cleanup { cleanup } else { steps.iter().find(|s| s.step_number == step.step_number) };
        step.failed = record.is_some_and(|r| r.outcomes.iter().any(|o| !o.ok && super::replay::was_run(o)));
    }
}

/// One step of a video's timeline, named for the player and the report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimelineStep {
    pub name: String,
    pub at_ms: u32,
    pub failed: bool,
    pub actions: Vec<VideoMark>,
}

/// What the player needs to draw a video's markers: the video's title, its
/// length when known, the steps and (for a discovery recording) a flat list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VideoTimeline {
    pub title: String,
    pub length_ms: Option<u32>,
    pub steps: Vec<TimelineStep>,
    pub marks: Vec<VideoMark>,
}

/// The timeline of a case's video. A case without markers has no steps and
/// no length: the player then shows the video alone.
pub fn case_timeline(case: &CaseRecord) -> VideoTimeline {
    let title = format!("Video of #{} {}", case.case_id, case.title);
    let Some(marks) = &case.video_marks else {
        return VideoTimeline { title, length_ms: None, steps: Vec::new(), marks: Vec::new() };
    };
    let steps = marks
        .steps
        .iter()
        .map(|s| TimelineStep {
            name: if s.cleanup { super::CLEANUP_LABEL.to_string() } else { super::report::step_label(s.step_number) },
            at_ms: s.at_ms,
            failed: s.failed,
            actions: s.actions.clone(),
        })
        .collect();
    VideoTimeline { title, length_ms: Some(marks.length_ms), steps, marks: Vec::new() }
}
