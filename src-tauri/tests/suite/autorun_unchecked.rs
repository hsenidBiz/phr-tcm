//! What a step leaves unchecked: the sentence still loads and keeps its
//! signature, a list of entries round-trips, any other shape is refused
//! saying the shape, and the expectations are counted as the shared
//! fixture (which the card's TypeScript mirror reads too) says.

use v2_lib::autorun::unchecked::{count_words, expectations, line};
use v2_lib::autorun::{edits, CaseScript, StepScript, Unchecked, UNCHECKED_SHAPE};

#[test]
fn a_sentence_unchecked_still_loads_and_keeps_its_signature() {
    let json = serde_json::json!({
        "step_number": 1,
        "actions": [{ "kind": "click", "selector": "#a" }],
        "unchecked": "the toast vanishes"
    });
    let step: StepScript = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(step.unchecked, Some(Unchecked::Step("the toast vanishes".to_string())));
    assert_eq!(serde_json::to_value(&step).unwrap(), json);
    // The signature built the old way: the actions' JSON, "|", the sentence.
    let by_hand = format!("{}|the toast vanishes", serde_json::to_string(&step.actions).unwrap());
    assert_eq!(edits::step_signature(&step), by_hand);
}

#[test]
fn a_list_of_entries_loads_and_round_trips() {
    let json = serde_json::json!({
        "step_number": 2,
        "actions": [{ "kind": "expect_hidden", "selector": "#denied" }],
        "unchecked": [
            { "item": "no lock icon", "why": "no check reads an icon" },
            { "item": "no denied message", "why": "no check reads words", "stand_in": "expect_hidden" }
        ]
    });
    let step: StepScript = serde_json::from_value(json.clone()).unwrap();
    let u = step.unchecked.as_ref().unwrap();
    assert_eq!(u.items().len(), 2);
    assert_eq!(u.items()[1].stand_in.as_deref(), Some("expect_hidden"));
    assert_eq!(u.reason(), None);
    assert_eq!(serde_json::to_value(&step).unwrap(), json);
    // An empty list loads as an empty list (the save refuses it).
    let empty: Unchecked = serde_json::from_value(serde_json::json!([])).unwrap();
    assert_eq!(empty, Unchecked::Items(vec![]));
    // A sentence has no entries.
    assert!(Unchecked::from("why").items().is_empty());
}

#[test]
fn an_unchecked_of_another_shape_is_refused_saying_the_shape() {
    for bad in [
        serde_json::json!(5),
        serde_json::json!(["a"]),
        serde_json::json!([{ "item": "x" }]),
        serde_json::json!([{ "item": "x", "why": "y", "reason": "z" }]),
    ] {
        let err = serde_json::from_value::<Unchecked>(bad.clone()).unwrap_err().to_string();
        assert!(err.contains(UNCHECKED_SHAPE), "{bad}: {err}");
    }
}

#[derive(serde::Deserialize)]
struct Entry {
    name: String,
    script: CaseScript,
    lines: Vec<String>,
    words: String,
}

#[test]
fn expectations_follow_the_shared_fixture() {
    let entries: Vec<Entry> = serde_json::from_str(include_str!("../fixtures/unchecked_expectations.json")).unwrap();
    assert!(entries.len() >= 5);
    for e in entries {
        let found = expectations(&e.script);
        let lines: Vec<String> = found.iter().map(line).collect();
        assert_eq!(lines, e.lines, "{}", e.name);
        if !found.is_empty() {
            assert_eq!(count_words(found.len()), e.words, "{}", e.name);
        }
    }
    assert_eq!(count_words(1), "1 expectation unchecked");
    assert_eq!(count_words(3), "3 expectations unchecked");
}
