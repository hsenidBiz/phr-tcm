//! The few protocol calls everything else is built from.

mod common;

use common::ScriptedDriver;
use serde_json::json;
use v2_lib::browser::cdp::CdpError;
use v2_lib::browser::page::{self, GROUP};

#[tokio::test]
async fn eval_value_unwraps_the_value() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "result": { "type": "string", "value": "https://x/" } })));
    assert_eq!(page::eval_value(&mut d, "location.href").await.unwrap(), json!("https://x/"));
}

/// A thrown exception comes back as a SUCCESSFUL reply carrying
/// exceptionDetails. Reading it as a value would turn "the page threw"
/// into "the check found nothing".
#[tokio::test]
async fn a_thrown_exception_is_an_error_not_an_empty_value() {
    let mut d = ScriptedDriver::new(|_, _| {
        Ok(json!({
            "result": { "type": "object", "subtype": "error" },
            "exceptionDetails": { "text": "Uncaught", "exception": { "description": "SyntaxError: bad selector" } }
        }))
    });
    let err = page::eval_value(&mut d, "x").await.unwrap_err();
    match err {
        CdpError::Protocol { message, .. } => assert!(message.contains("bad selector"), "{message}"),
        other => panic!("expected a protocol error, got {other:?}"),
    }
}

#[tokio::test]
async fn the_document_handle_lives_in_our_object_group() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "result": { "objectId": "doc-1" } })));
    assert_eq!(page::document(&mut d).await.unwrap(), "doc-1");
    let p = &d.calls_to("Runtime.evaluate")[0];
    assert_eq!(p["expression"], "document");
    assert_eq!(p["objectGroup"], GROUP);
}

/// Script values travel as arguments. They are never pasted into source,
/// so no value can close a string and run its own code.
#[tokio::test]
async fn call_value_passes_values_as_arguments_not_source() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "result": { "value": true } })));
    let got = page::call_value(
        &mut d,
        &"el-1".to_string(),
        "function(a) { return a.length > 0; }",
        &[json!("he said \"hi\"</script>")],
    )
    .await
    .unwrap();
    assert_eq!(got, json!(true));
    let p = &d.calls_to("Runtime.callFunctionOn")[0];
    assert_eq!(p["objectId"], "el-1");
    assert_eq!(p["arguments"][0]["value"], "he said \"hi\"</script>");
    assert_eq!(p["returnByValue"], true);
    assert_eq!(p["awaitPromise"], true);
    assert!(!p["functionDeclaration"].as_str().unwrap().contains("he said"));
}

#[tokio::test]
async fn call_elements_turns_an_array_into_handles_in_index_order() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => Ok(json!({ "result": [
            { "name": "length", "value": { "type": "number", "value": 2 } },
            { "name": "1", "value": { "objectId": "el-b" } },
            { "name": "0", "value": { "objectId": "el-a" } }
        ] })),
        other => panic!("unexpected {other}"),
    });
    let got = page::call_elements(&mut d, &"doc".to_string(), "function() { return []; }", &[])
        .await
        .unwrap();
    assert_eq!(got, vec!["el-a".to_string(), "el-b".to_string()]);
    assert_eq!(d.calls_to("Runtime.callFunctionOn")[0]["objectGroup"], GROUP);
    assert_eq!(d.calls_to("Runtime.getProperties")[0]["objectId"], "arr");
}

/// `Runtime.getProperties` can throw too (a throwing getter on the array).
/// That must surface as an error, not as "the array had no properties" -
/// which would read as "nothing matched".
#[tokio::test]
async fn call_elements_reports_a_thrown_get_properties() {
    let mut d = ScriptedDriver::new(|method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => Ok(json!({
            "result": [],
            "exceptionDetails": { "text": "Uncaught", "exception": { "description": "TypeError: boom" } }
        })),
        other => panic!("unexpected {other}"),
    });
    let err = page::call_elements(&mut d, &"doc".to_string(), "function() { return []; }", &[])
        .await
        .unwrap_err();
    match err {
        CdpError::Protocol { message, .. } => assert!(message.contains("boom"), "{message}"),
        other => panic!("expected a protocol error, got {other:?}"),
    }
}

/// Property names "0".."11" sort lexically as "0","1","10","11","2",... -
/// only a numeric sort puts "10" and "11" after "9".
#[tokio::test]
async fn call_elements_sorts_numerically_not_lexically() {
    let mut props: Vec<serde_json::Value> = (0..12)
        .rev()
        .map(|i| json!({ "name": i.to_string(), "value": { "objectId": format!("el-{i}") } }))
        .collect();
    props.push(json!({ "name": "length", "value": { "type": "number", "value": 12 } }));
    let mut d = ScriptedDriver::new(move |method, _| match method {
        "Runtime.callFunctionOn" => Ok(json!({ "result": { "objectId": "arr" } })),
        "Runtime.getProperties" => Ok(json!({ "result": props.clone() })),
        other => panic!("unexpected {other}"),
    });
    let got = page::call_elements(&mut d, &"doc".to_string(), "function() { return []; }", &[])
        .await
        .unwrap();
    let want: Vec<String> = (0..12).map(|i| format!("el-{i}")).collect();
    assert_eq!(got, want);
}

#[tokio::test]
async fn resolve_backend_asks_for_a_handle_in_our_group() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "object": { "objectId": "el-7" } })));
    assert_eq!(page::resolve_backend(&mut d, 7).await.unwrap(), "el-7");
    let p = &d.calls_to("DOM.resolveNode")[0];
    assert_eq!(p["backendNodeId"], 7);
    assert_eq!(p["objectGroup"], GROUP);
}

#[tokio::test]
async fn backend_id_reads_the_id_and_sends_the_object_id() {
    let mut d = ScriptedDriver::new(|_, _| Ok(json!({ "node": { "backendNodeId": 42 } })));
    assert_eq!(page::backend_id(&mut d, &"el-7".to_string()).await.unwrap(), 42);
    let p = &d.calls_to("DOM.describeNode")[0];
    assert_eq!(p["objectId"], "el-7");
}

/// Releasing is housekeeping. A failure there must never fail the action.
#[tokio::test]
async fn release_swallows_its_own_failure() {
    let mut d = ScriptedDriver::new(|_, _| Err(CdpError::Closed));
    page::release(&mut d).await;
    assert_eq!(d.methods(), vec!["Runtime.releaseObjectGroup".to_string()]);
}
