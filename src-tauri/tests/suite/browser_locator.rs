//! How a script says WHICH element, and how that becomes handles.

use crate::common;

use common::ScriptedDriver;
use serde_json::json;
use v2_lib::browser::expect::{expect, Check};
use v2_lib::browser::snapshot::{frame_step, probe};
use v2_lib::browser::locator::{frame_unreachable, name_matches, resolve_explained, FRAME_DOC_JS, FRAME_JS, resolve, LocatorStep, Target, CSS_JS, LEGACY_JS, VISIBLE_JS};

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

/// `expect_visible` needs a second look that does NOT filter by
/// visibility, or "is there but cannot be seen" can never be true of a
/// structured target: the locator would have dropped the hidden element
/// before the check ran. Every step relaxes, the rest of each step is
/// untouched, and a legacy string (which has no filter to relax) comes
/// back exactly as it was.
#[test]
fn a_target_can_be_asked_again_including_what_cannot_be_seen() {
    let one: Target = serde_json::from_value(json!({ "css": "#ghost" })).unwrap();
    assert_eq!(one.including_hidden(), serde_json::from_value(json!({ "css": "#ghost", "visible": false })).unwrap());

    let chain: Target = serde_json::from_value(json!([
        { "role": "dialog", "name": "Add Rating Method" },
        { "role": "button", "name": "Add Method", "exact": true, "nth": 1, "visible": true }
    ]))
    .unwrap();
    let relaxed: Target = serde_json::from_value(json!([
        { "role": "dialog", "name": "Add Rating Method", "visible": false },
        { "role": "button", "name": "Add Method", "exact": true, "nth": 1, "visible": false }
    ]))
    .unwrap();
    assert_eq!(chain.including_hidden(), relaxed);

    let legacy = Target::from("text=Save");
    assert_eq!(legacy.including_hidden(), legacy);
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
        // Each match of a step that is not the last is asked whether it is
        // a frame to enter; these dialogs are plain elements.
        "Runtime.callFunctionOn" if params["functionDeclaration"] == FRAME_JS => {
            Ok(json!({ "result": { "value": "element" } }))
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
        // Each match of a step that is not the last is asked whether it is
        // a frame to enter; these dialogs are plain elements.
        "Runtime.callFunctionOn" if params["functionDeclaration"] == FRAME_JS => {
            Ok(json!({ "result": { "value": "element" } }))
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

#[test]
fn an_unreachable_frame_is_explained_in_a_sentence() {
    assert_eq!(
        frame_unreachable("iframe#locked"),
        "the frame iframe#locked holds a page from another site (or has not loaded), which Auto Run cannot reach"
    );
}

// --- frames, scripted ------------------------------------------------------

/// A page holding one frame element (`frame-el`). `FRAME_JS` answers
/// `frame_answer` for it. Entering it reads `contentDocument` (handle
/// `doc-parent`, which lives in the PARENT's context), which must be
/// re-resolved by backend id 42 into `frame-doc` before anything searches
/// inside: a search on `doc-parent` would run with the parent's globals.
fn frame_page(frame_answer: &'static str) -> ScriptedDriver {
    ScriptedDriver::new(move |method, params| {
        let on = params["objectId"].as_str().unwrap_or("");
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.releaseObjectGroup" => Ok(json!({})),
            "Runtime.callFunctionOn" if f == FRAME_JS => Ok(json!({ "result": { "value": frame_answer } })),
            "Runtime.callFunctionOn" if f == FRAME_DOC_JS => Ok(json!({ "result": { "objectId": "a-doc" } })),
            "Runtime.callFunctionOn" if f == CSS_JS => {
                let arr = match on {
                    "doc" => "a-top",
                    "frame-doc" => "a-in",
                    other => panic!("searched inside {other}: the frame document was not re-resolved"),
                };
                Ok(json!({ "result": { "objectId": arr } }))
            }
            "Runtime.callFunctionOn" if f == VISIBLE_JS => Ok(json!({ "result": { "value": true } })),
            "Runtime.callFunctionOn" => Ok(json!({ "result": { "value": { "tag": "button", "text": "Select", "rect": [0, 0, 10, 10] } } })),
            "Runtime.getProperties" => {
                let item = match on {
                    "a-top" => "frame-el",
                    "a-in" => "pick",
                    "a-doc" => "doc-parent",
                    other => panic!("unexpected array {other}"),
                };
                Ok(json!({ "result": [{ "name": "0", "value": { "objectId": item } }] }))
            }
            "DOM.describeNode" => {
                assert_eq!(on, "doc-parent");
                Ok(json!({ "node": { "backendNodeId": 42 } }))
            }
            "DOM.resolveNode" => {
                assert_eq!(params["backendNodeId"], 42);
                Ok(json!({ "object": { "objectId": "frame-doc" } }))
            }
            other => panic!("unexpected {other}"),
        }
    })
}

fn through_frame() -> Target {
    serde_json::from_value(json!([{ "css": "iframe" }, { "css": "#pick" }])).unwrap()
}

#[tokio::test]
async fn a_step_after_a_frame_searches_the_frames_own_document() {
    let mut d = frame_page("frame");
    let r = resolve_explained(&mut d, &through_frame()).await.unwrap();
    assert_eq!(r.handles, vec!["pick".to_string()]);
    assert_eq!(r.unreachable_frame, None);
}

#[tokio::test]
async fn a_frame_on_the_last_step_is_kept_as_the_element() {
    let mut d = frame_page("frame");
    let t: Target = serde_json::from_value(json!({ "css": "iframe" })).unwrap();
    assert_eq!(resolve(&mut d, &t).await.unwrap(), vec!["frame-el".to_string()]);
    assert!(d.calls_to("DOM.resolveNode").is_empty(), "the last step must not be entered");
}

#[tokio::test]
async fn an_unreachable_frame_yields_nothing_and_says_why() {
    let mut d = frame_page("unreachable");
    let r = resolve_explained(&mut d, &through_frame()).await.unwrap();
    assert!(r.handles.is_empty());
    assert_eq!(r.unreachable_frame, Some(frame_unreachable("iframe")));
}

/// Two frames match the same step: `locked` cannot be entered, `open` can
/// and holds nothing that matches the next step. `order` is the order the
/// step finds them in.
fn two_frames(order: [&'static str; 2]) -> ScriptedDriver {
    ScriptedDriver::new(move |method, params| {
        let on = params["objectId"].as_str().unwrap_or("");
        let f = params["functionDeclaration"].as_str().unwrap_or("");
        match method {
            "Runtime.evaluate" => Ok(json!({ "result": { "objectId": "doc" } })),
            "Runtime.releaseObjectGroup" => Ok(json!({})),
            "Runtime.callFunctionOn" if f == FRAME_JS => {
                Ok(json!({ "result": { "value": if on == "locked" { "unreachable" } else { "frame" } } }))
            }
            "Runtime.callFunctionOn" if f == FRAME_DOC_JS => Ok(json!({ "result": { "objectId": "a-doc" } })),
            "Runtime.callFunctionOn" if f == CSS_JS => {
                Ok(json!({ "result": { "objectId": if on == "doc" { "a-top" } else { "a-in" } } }))
            }
            "Runtime.getProperties" => {
                let items: Vec<&str> = match on {
                    "a-top" => order.to_vec(),
                    "a-doc" => vec!["doc-parent"],
                    _ => vec![],
                };
                let props: Vec<serde_json::Value> = items
                    .iter()
                    .enumerate()
                    .map(|(i, o)| json!({ "name": i.to_string(), "value": { "objectId": o } }))
                    .collect();
                Ok(json!({ "result": props }))
            }
            "DOM.describeNode" => Ok(json!({ "node": { "backendNodeId": 42 } })),
            "DOM.resolveNode" => Ok(json!({ "object": { "objectId": "frame-doc" } })),
            other => panic!("unexpected {other}"),
        }
    })
}

/// Spec 9: the "cannot reach" sentence is for a step where NO frame could be
/// entered. When another frame at that step was entered and simply holds no
/// match, the honest answer is "not found", whichever frame came first.
#[tokio::test]
async fn an_unreachable_frame_beside_an_entered_one_records_no_sentence() {
    for order in [["locked", "open"], ["open", "locked"]] {
        let mut d = two_frames(order);
        let r = resolve_explained(&mut d, &through_frame()).await.unwrap();
        assert!(r.handles.is_empty(), "{order:?}");
        assert_eq!(r.unreachable_frame, None, "{order:?}");
    }
}

#[tokio::test]
async fn no_check_passes_on_nothing_behind_an_unreachable_frame() {
    for check in [Check::Hidden, Check::Count(0)] {
        let mut d = frame_page("unreachable");
        let out = expect(&mut d, &through_frame(), check, 50, 10).await;
        assert!(!out.ok, "passed through a frame it never entered: {}", out.detail);
        assert!(out.detail.contains("holds a page from another site"), "{}", out.detail);
    }
}

#[tokio::test]
async fn the_probe_names_an_unreachable_frame() {
    let mut d = frame_page("unreachable");
    let out = probe(&mut d, &through_frame()).await.unwrap();
    assert!(out.contains("holds a page from another site"), "{out}");
}

#[test]
fn an_iframe_step_is_its_name_else_its_id_else_its_title_else_its_place() {
    let attrs = |pairs: &[&str]| json!(pairs);
    assert_eq!(frame_step("Employee Search", &attrs(&[]), 0), json!({ "role": "Iframe", "name": "Employee Search", "exact": true }));
    assert_eq!(frame_step("", &attrs(&["id", "es-frame", "title", "Search"]), 0), json!({ "css": "iframe#es-frame" }));
    assert_eq!(frame_step("", &attrs(&["title", "Search"]), 0), json!({ "css": "iframe[title='Search']" }));
    assert_eq!(frame_step("", &attrs(&["data-k", "bare"]), 2), json!({ "css": "iframe", "nth": 2 }));
}

/// The odd ids and titles a real page has: a space, a quote, a leading
/// digit, a colon, a backslash. Shared with the live check in
/// `browser_live`, which proves each printed step matches its frame.
pub const ODD_FRAME_VALUES: [&str; 6] = ["my frame", "it's", "1st-frame", "ns:frame", "back\\slash", "O'Brien \\ 'co'"];

/// A CSS quoted string read back: `\` followed by up to six hex digits
/// (and one optional space) is that code point, `\` followed by anything
/// else is that character, and an unescaped `'` ends it. `None` when the
/// text is not one whole single-quoted string.
fn css_string(s: &str) -> Option<String> {
    let mut chars = s.strip_prefix('\'')?.chars().peekable();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\'' => return chars.next().is_none().then_some(out),
            '\\' => {
                let mut hex = String::new();
                while hex.len() < 6 && chars.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    hex.push(chars.next().unwrap());
                }
                if hex.is_empty() {
                    out.push(chars.next()?);
                } else {
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                    if chars.peek() == Some(&' ') {
                        chars.next();
                    }
                }
            }
            '\n' | '\r' => return None,
            c => out.push(c),
        }
    }
    None
}

/// The attribute value an `iframe[attr='...']` step matches, or the id an
/// `iframe#id` step matches when the id is a plain CSS identifier.
fn matched_value(css: &str, attr: &str) -> Option<String> {
    if let Some(id) = css.strip_prefix("iframe#") {
        let mut chars = id.chars();
        let first = chars.next()?;
        let plain = (first.is_ascii_alphabetic() || first == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        return (attr == "id" && plain).then(|| id.to_string());
    }
    css_string(css.strip_prefix(&format!("iframe[{attr}="))?.strip_suffix(']')?)
}

/// Ids and titles always print a valid CSS step that names the value they
/// came from: an id that is not a plain identifier uses `iframe[id='...']`,
/// and both escape `\` and `'`.
#[test]
fn an_iframe_step_escapes_odd_ids_and_titles() {
    let attrs = |pairs: &[&str]| json!(pairs);
    assert_eq!(frame_step("", &attrs(&["id", "my frame"]), 0), json!({ "css": "iframe[id='my frame']" }));
    assert_eq!(frame_step("", &attrs(&["id", "1st-frame"]), 0), json!({ "css": "iframe[id='1st-frame']" }));
    assert_eq!(frame_step("", &attrs(&["id", "back\\slash"]), 0), json!({ "css": "iframe[id='back\\\\slash']" }));
    assert_eq!(frame_step("", &attrs(&["title", "O'Brien \\ 'co'"]), 0), json!({ "css": "iframe[title='O\\'Brien \\\\ \\'co\\'']" }));
    for value in ODD_FRAME_VALUES {
        for attr in ["id", "title"] {
            let step = frame_step("", &attrs(&[attr, value]), 0);
            let css = step["css"].as_str().unwrap();
            assert_eq!(matched_value(css, attr).as_deref(), Some(value), "{attr} {value:?} printed {css}");
        }
    }
    // A line break in a title is escaped, never printed raw.
    let step = frame_step("", &attrs(&["title", "two\nlines"]), 0);
    assert_eq!(matched_value(step["css"].as_str().unwrap(), "title").as_deref(), Some("two\nlines"), "{step}");
}
