//! How a script says WHICH element, and how that becomes handles.

mod common;

use common::ScriptedDriver;
use serde_json::json;
use v2_lib::browser::locator::{name_matches, resolve, LocatorStep, Target, CSS_JS, LEGACY_JS, VISIBLE_JS};

fn step(json: serde_json::Value) -> LocatorStep {
    serde_json::from_value(json).unwrap()
}

#[test]
fn a_plain_string_is_still_a_selector() {
    let t: Target = serde_json::from_value(json!("text=Sign in")).unwrap();
    assert_eq!(t, Target::Legacy("text=Sign in".into()));
    assert!(t.is_legacy());
    // And it goes back out as the same plain string, so a saved script
    // does not change shape by being loaded and saved.
    assert_eq!(serde_json::to_value(&t).unwrap(), json!("text=Sign in"));
}

#[test]
fn an_object_is_one_step_and_a_list_is_a_chain() {
    let one: Target = serde_json::from_value(json!({ "role": "button", "name": "Save" })).unwrap();
    assert_eq!(one, Target::One(step(json!({ "role": "button", "name": "Save" }))));
    let chain: Target = serde_json::from_value(json!([
        { "role": "dialog", "name": "Add Rating Method" },
        { "role": "button", "name": "Add Method" }
    ]))
    .unwrap();
    assert!(matches!(chain, Target::Chain(ref v) if v.len() == 2));
    // Defaults are left out when written back.
    assert_eq!(
        serde_json::to_value(&one).unwrap(),
        json!({ "role": "button", "name": "Save" })
    );
}

/// A typo must not silently become "match anything".
#[test]
fn an_unknown_field_is_refused() {
    assert!(serde_json::from_value::<Target>(json!({ "role": "button", "nme": "Save" })).is_err());
}

/// The error names the misspelt field, so a person editing the script by
/// hand can see what to fix.
#[test]
fn an_unknown_field_error_names_the_field() {
    let err = serde_json::from_value::<Target>(json!({ "role": "button", "nme": "Save" })).unwrap_err();
    assert!(err.to_string().contains("nme"), "{err}");
}

/// A struct whose fields all have defaults also deserializes from a JSON
/// array by POSITION - so without a hand-written `Deserialize`,
/// `["button", "Save"]` would quietly become
/// `One(LocatorStep { role: Some("button"), name: Some("Save"), .. })`.
/// Only a string, a locator object, or a list of locator objects is a
/// target; everything else is refused.
#[test]
fn only_a_string_an_object_or_a_list_of_objects_is_a_target() {
    let refuses = |v: serde_json::Value| assert!(serde_json::from_value::<Target>(v).is_err());
    refuses(json!(["button", "Save"]));
    refuses(json!([["button", "Save"]]));
    refuses(json!([{ "role": "button" }, "x"]));
    refuses(json!(42));
    refuses(json!(true));
    refuses(json!(null));
}

#[test]
fn validation_names_the_problem() {
    let ok = |v: serde_json::Value| serde_json::from_value::<Target>(v).unwrap().validate();
    assert!(ok(json!("#go")).is_ok());
    assert!(ok(json!({ "css": "#go", "nth": 0 })).is_ok());
    assert!(ok(json!("  ")).unwrap_err().contains("empty"));
    assert!(ok(json!({})).unwrap_err().contains("one of role, text or css"));
    assert!(ok(json!({ "role": "button", "css": "#go" })).unwrap_err().contains("only one"));
    assert!(ok(json!({ "text": "Save", "name": "Save" })).unwrap_err().contains("name only goes with role"));
    assert!(ok(json!({ "css": "#go", "nth": -1 })).unwrap_err().contains("nth"));
    assert!(ok(json!([])).unwrap_err().contains("empty"));
    assert!(ok(json!({ "role": " " })).unwrap_err().contains("empty"));
}

#[test]
fn a_target_describes_itself_the_way_a_person_would() {
    let t: Target = serde_json::from_value(json!([
        { "role": "dialog", "name": "Add Rating Method" },
        { "role": "button", "name": "Add Method", "nth": 1 }
    ]))
    .unwrap();
    assert_eq!(t.describe(), r#"button "Add Method" #2 in dialog "Add Rating Method""#);
    assert_eq!(Target::from("#go").describe(), "#go");
    let text: Target = serde_json::from_value(json!({ "text": "Step 1 of 6" })).unwrap();
    assert_eq!(text.describe(), r#"text "Step 1 of 6""#);
}

/// Measured on real Edge: a label's name arrives as "Method Name * " with
/// the trailing space, and the protocol's own name filter is exact-only.
#[test]
fn names_match_loosely_unless_exact_is_asked_for() {
    assert!(name_matches("Method Name * ", "method name", false));
    assert!(name_matches("  Save   changes ", "Save changes", true));
    assert!(!name_matches("Save changes", "save changes", true));
    assert!(!name_matches("Save", "Save changes", false));
}

/// The role path: Chrome computes role and name; this app matches the
/// name, turns each node into a handle, and drops the ones nobody can see.
#[tokio::test]
async fn a_role_locator_uses_the_accessibility_tree() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Accessibility.queryAXTree" => {
            assert_eq!(params["objectId"], "doc");
            assert_eq!(params["role"], "button");
            assert!(params.get("accessibleName").is_none(), "names are matched here, not by the browser");
            Ok(json!({ "nodes": [
                { "ignored": false, "name": { "value": "Save changes " }, "backendDOMNodeId": 7 },
                { "ignored": true,  "name": { "value": "Save changes" },  "backendDOMNodeId": 8 },
                { "ignored": false, "name": { "value": "Cancel" },        "backendDOMNodeId": 9 },
                { "ignored": false, "name": { "value": "Save changes" },  "backendDOMNodeId": 10 }
            ] }))
        }
        "DOM.resolveNode" => Ok(json!({ "object": { "objectId": format!("el-{}", params["backendNodeId"]) } })),
        "Runtime.callFunctionOn" => {
            assert_eq!(params["functionDeclaration"], VISIBLE_JS);
            // Node 10 exists but cannot be seen.
            Ok(json!({ "result": { "value": params["objectId"] == "el-7" } }))
        }
        other => panic!("unexpected {other}"),
    });
    let t: Target = serde_json::from_value(json!({ "role": "button", "name": "save changes" })).unwrap();
    assert_eq!(resolve(&mut d, &t).await.unwrap(), vec!["el-7".to_string()]);
}

#[tokio::test]
async fn visible_false_keeps_hidden_matches() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Accessibility.queryAXTree" => Ok(json!({ "nodes": [
            { "ignored": false, "name": { "value": "Save" }, "backendDOMNodeId": 7 }
        ] })),
        "DOM.resolveNode" => Ok(json!({ "object": { "objectId": format!("el-{}", params["backendNodeId"]) } })),
        other => panic!("visibility must not be checked, got {other}"),
    });
    let t: Target = serde_json::from_value(json!({ "role": "button", "visible": false })).unwrap();
    assert_eq!(resolve(&mut d, &t).await.unwrap(), vec!["el-7".to_string()]);
}

/// A chain searches INSIDE the previous step's matches, and nth picks
/// from that step's matches.
#[tokio::test]
async fn a_chain_narrows_inside_the_previous_match() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Accessibility.queryAXTree" => {
            assert_eq!(params["objectId"], "doc");
            Ok(json!({ "nodes": [
                { "ignored": false, "name": { "value": "Add Rating Method" }, "backendDOMNodeId": 3 }
            ] }))
        }
        "DOM.resolveNode" => Ok(json!({ "object": { "objectId": "dialog" } })),
        "Runtime.callFunctionOn" if params["functionDeclaration"] == VISIBLE_JS => {
            Ok(json!({ "result": { "value": true } }))
        }
        "Runtime.callFunctionOn" => {
            assert_eq!(params["functionDeclaration"], CSS_JS);
            assert_eq!(params["objectId"], "dialog", "the css step must search inside the dialog");
            assert_eq!(params["arguments"][0]["value"], "tr");
            assert_eq!(params["arguments"][1]["value"], true, "visible-only by default");
            Ok(json!({ "result": { "objectId": "arr" } }))
        }
        "Runtime.getProperties" => Ok(json!({ "result": [
            { "name": "0", "value": { "objectId": "row-0" } },
            { "name": "1", "value": { "objectId": "row-1" } },
            { "name": "2", "value": { "objectId": "row-2" } }
        ] })),
        other => panic!("unexpected {other}"),
    });
    let t: Target = serde_json::from_value(json!([
        { "role": "dialog", "name": "Add Rating Method" },
        { "css": "tr", "nth": 1 }
    ]))
    .unwrap();
    assert_eq!(resolve(&mut d, &t).await.unwrap(), vec!["row-1".to_string()]);
    // Exactly one root at every step: deduplication never has anything to
    // do, so it must never spend a protocol call finding that out.
    assert!(d.calls_to("DOM.describeNode").is_empty());
}

/// Two roots that both contain the same element (nested dialogs, nested
/// rows of the same role) must not double-count it or throw `nth` off.
/// Root 0's css search finds backend ids [5, 6]; root 1's finds [6, 7].
/// Backend id 6 is the same element reached two ways, so it is kept only
/// once, in the order it was first seen.
#[tokio::test]
async fn a_chain_deduplicates_elements_reached_through_more_than_one_root() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Accessibility.queryAXTree" => Ok(json!({ "nodes": [
            { "ignored": false, "name": { "value": "A" }, "backendDOMNodeId": 100 },
            { "ignored": false, "name": { "value": "B" }, "backendDOMNodeId": 101 }
        ] })),
        "DOM.resolveNode" => {
            Ok(json!({ "object": { "objectId": format!("root-{}", params["backendNodeId"]) } }))
        }
        "Runtime.callFunctionOn" if params["functionDeclaration"] == VISIBLE_JS => {
            Ok(json!({ "result": { "value": true } }))
        }
        "Runtime.callFunctionOn" => {
            assert_eq!(params["functionDeclaration"], CSS_JS);
            let arr = if params["objectId"] == "root-100" { "arr-0" } else { "arr-1" };
            Ok(json!({ "result": { "objectId": arr } }))
        }
        "Runtime.getProperties" => {
            let props = if params["objectId"] == "arr-0" {
                vec![json!({ "name": "0", "value": { "objectId": "h5" } }), json!({ "name": "1", "value": { "objectId": "h6" } })]
            } else {
                vec![json!({ "name": "0", "value": { "objectId": "h6b" } }), json!({ "name": "1", "value": { "objectId": "h7" } })]
            };
            Ok(json!({ "result": props }))
        }
        "DOM.describeNode" => {
            let id = match params["objectId"].as_str().unwrap() {
                "h5" => 5,
                "h6" | "h6b" => 6,
                "h7" => 7,
                other => panic!("unexpected handle {other}"),
            };
            Ok(json!({ "node": { "backendNodeId": id } }))
        }
        other => panic!("unexpected {other}"),
    });
    let t: Target = serde_json::from_value(json!([{ "role": "dialog" }, { "css": "div" }])).unwrap();
    assert_eq!(
        resolve(&mut d, &t).await.unwrap(),
        vec!["h5".to_string(), "h6".to_string(), "h7".to_string()]
    );
}

#[tokio::test]
async fn a_legacy_string_keeps_its_old_meaning() {
    let mut d = ScriptedDriver::new(|method, params| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Runtime.callFunctionOn" => {
            assert_eq!(params["functionDeclaration"], LEGACY_JS);
            assert_eq!(params["arguments"][0]["value"], "text=Sign in");
            Ok(json!({ "result": { "objectId": "arr" } }))
        }
        "Runtime.getProperties" => Ok(json!({ "result": [ { "name": "0", "value": { "objectId": "el" } } ] })),
        other => panic!("unexpected {other}"),
    });
    assert_eq!(resolve(&mut d, &Target::from("text=Sign in")).await.unwrap(), vec!["el".to_string()]);
}

#[tokio::test]
async fn nothing_matching_is_an_empty_list_not_an_error() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
        "Accessibility.queryAXTree" => Ok(json!({ "nodes": [] })),
        other => panic!("unexpected {other}"),
    });
    let t: Target = serde_json::from_value(json!([{ "role": "dialog" }, { "css": "button" }])).unwrap();
    assert!(resolve(&mut d, &t).await.unwrap().is_empty());
}
