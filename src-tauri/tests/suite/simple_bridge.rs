//! App Basic mode on the AI Bridge: the assistants read test cases and write
//! Auto Test scripts, and nothing else of the writing, searching and report
//! tools. Basic mode is a view and an override - no saved switch changes.

use v2_lib::ai_bridge::{route_timed_simple, BridgeContext, SwitchGate, API_WRITES_OFF};
use v2_lib::autorun::agents::Caller;
use v2_lib::simple_view::{
    SIMPLE_API_NO_ENVIRONMENT, context_under, disabled_under, disabled_under_ctx, route_refusal, SIMPLE_HIDDEN_ROUTES, SIMPLE_HIDDEN_TOOLS, SIMPLE_HIDES_TOOL,
};

fn saved() -> BridgeContext {
    BridgeContext {
        org: "acme".into(),
        project: "Web".into(),
        disabled_tools: vec!["get_bugs".into()],
        api_writes: false,
        network_tools: false,
        gate: SwitchGate { env_id: Some("env-1".into()), ..Default::default() },
        ..BridgeContext::default()
    }
}

async fn ask(simple: bool, ctx: &BridgeContext, method: &str, target: &str, body: &str) -> (u16, String) {
    route_timed_simple(simple, &Caller::Default, ctx, None, method, target, body, "1.0.0").await
}

#[test]
fn the_hidden_list_is_the_writing_tools_the_searches_and_every_report_tool() {
    for t in [
        "begin_test_case_writing",
        "check_spec_coverage",
        "merge_case_files",
        "validate_cases",
        "get_tags",
        "optimize_cases",
        "transform_cases",
        "search_pbis",
        "search_wiki",
        "get_wiki_page",
    ] {
        assert!(SIMPLE_HIDDEN_TOOLS.contains(&t), "{t}");
    }
    for t in v2_lib::ai_tools::CRYSTAL_TOOLS {
        assert!(SIMPLE_HIDDEN_TOOLS.contains(t), "{t}");
    }
    assert_eq!(SIMPLE_HIDDEN_TOOLS.len(), 10 + v2_lib::ai_tools::CRYSTAL_TOOLS.len());
    // What stays: the reads, the Auto Test, API template, network and database tools.
    for t in ["get_guide", "get_current_pbi", "get_test_cases", "get_bugs", "search_test_suites", "get_suite_test_cases", "get_run_results"]
        .into_iter()
        .chain(v2_lib::ai_tools::module_tools().filter(|t| !v2_lib::ai_tools::CRYSTAL_TOOLS.contains(t)))
    {
        assert!(!SIMPLE_HIDDEN_TOOLS.contains(&t), "{t} must stay");
    }
}

#[test]
fn the_webview_mirror_of_the_hidden_list_matches() {
    let ts = include_str!("../../../src/lib/mcpTools.ts");
    let block = ts.split("export const SIMPLE_HIDDEN_TOOLS").nth(1).unwrap().split("] as const").next().unwrap();
    let mut from_ts: Vec<String> = block.split('"').skip(1).step_by(2).map(str::to_string).collect();
    let mut from_rs: Vec<String> = SIMPLE_HIDDEN_TOOLS.iter().map(|t| t.to_string()).collect();
    from_ts.sort();
    from_rs.sort();
    assert_eq!(from_ts, from_rs);
}

#[test]
fn every_hidden_tool_is_a_tool_the_server_offers() {
    let listed = v2_lib::mcp::all_tool_names();
    for t in SIMPLE_HIDDEN_TOOLS {
        assert!(listed.iter().any(|n| n == t), "{t} is not a tool");
    }
}

#[test]
fn simple_mode_ignores_the_saved_list_it_never_edits() {
    // Basic mode also turns the shown tools on: a saved switch-off of a shown
    // tool (get_bugs here) no longer counts, only the hidden tools are off.
    let list = vec!["get_bugs".to_string(), "search_pbis".to_string()];
    assert_eq!(disabled_under(false, &list), list);
    let under = disabled_under(true, &list);
    assert!(!under.contains(&"get_bugs".to_string()));
    assert_eq!(under.len(), SIMPLE_HIDDEN_TOOLS.len());
    for t in SIMPLE_HIDDEN_TOOLS {
        assert_eq!(under.iter().filter(|u| *u == t).count(), 1, "{t}");
    }
    assert_eq!(list.len(), 2, "the saved list is untouched");
}

#[test]
fn under_basic_mode_a_saved_switch_off_of_a_shown_tool_is_ignored_and_modules_still_gate() {
    use std::collections::BTreeSet;
    use v2_lib::ai_tools::effective_disabled_under;
    use v2_lib::modules::Module;
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    let saved: Vec<String> =
        ["search_test_suites", "get_suite_test_cases", "get_run_results", "search_pbis"].map(String::from).to_vec();
    let under = effective_disabled_under(true, &saved, &all);
    for t in ["search_test_suites", "get_suite_test_cases", "get_run_results"] {
        assert!(!under.iter().any(|u| u == t), "{t} is on under Basic mode");
    }
    assert!(SIMPLE_HIDDEN_TOOLS.iter().all(|t| under.iter().any(|u| u == t)));
    // A module that is off still takes its tools, saved list or not.
    let mut no_autorun = all.clone();
    no_autorun.remove(&Module::AutoRun);
    let off = effective_disabled_under(true, &saved, &no_autorun);
    let gated: Vec<&str> = v2_lib::ai_tools::module_tools().filter(|t| v2_lib::ai_tools::module_of_tool(t) == Some(Module::AutoRun)).collect();
    assert!(!gated.is_empty());
    assert!(gated.iter().all(|t| off.iter().any(|o| o == t)));
    // Basic off: the saved list comes back as it was.
    let plain = effective_disabled_under(false, &saved, &all);
    for t in &saved {
        assert!(plain.contains(t), "{t}");
    }
}

#[test]
fn the_context_under_simple_mode_forces_the_two_switches_and_off_restores_them() {
    let ctx = saved();
    let under = context_under(true, &ctx);
    assert!(under.network_tools && under.api_writes);
    assert_eq!(under.app_simple_mode, ctx.app_simple_mode, "the webview's push is what carries it");
    assert!(SIMPLE_HIDDEN_TOOLS.iter().all(|t| under.disabled_tools.iter().any(|d| d == t)));
    // Basic mode also turns the shown tools on: the saved switch-off of get_bugs is ignored.
    assert!(!under.disabled_tools.iter().any(|d| d == "get_bugs"));
    assert_eq!(under.disabled_tools.len(), SIMPLE_HIDDEN_TOOLS.len());
    // The stored context is what was saved, and Basic mode off shows it as it was.
    assert!(!ctx.network_tools && !ctx.api_writes);
    assert_eq!(ctx.disabled_tools, vec!["get_bugs".to_string()]);
    let off = context_under(false, &ctx);
    assert_eq!(off.disabled_tools, ctx.disabled_tools);
    assert!(!off.network_tools && !off.api_writes);
}

#[test]
fn a_context_with_no_active_environment_or_a_hold_gets_no_write_forced_on() {
    let mut no_env = saved();
    no_env.gate.env_id = None;
    let under = context_under(true, &no_env);
    assert!(!under.api_writes, "no environment, no write");
    assert!(!under.network_tools, "no environment, no network tools either (as push_context_with)");
    let mut held = saved();
    held.gate.holds = 1;
    assert!(!context_under(true, &held).api_writes, "a switch in progress holds writes off");
}

#[tokio::test]
async fn the_tools_answer_names_the_hidden_tools_under_simple_mode_and_is_as_before_otherwise() {
    let ctx = saved();
    let (status, body) = ask(true, &ctx, "GET", "/tools", "").await;
    assert_eq!(status, 200);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let disabled: Vec<&str> = v["disabled"].as_array().unwrap().iter().filter_map(|d| d.as_str()).collect();
    for t in SIMPLE_HIDDEN_TOOLS {
        assert!(disabled.contains(t), "{t}");
        assert!(v["simple_hidden"].as_array().unwrap().iter().any(|h| h == t), "{t}");
    }
    // Basic mode turns the shown tools on: the saved switch-off of get_bugs no longer counts.
    assert!(!disabled.contains(&"get_bugs"));
    assert_eq!(v["network_tools"], true);

    let (_, body) = ask(false, &ctx, "GET", "/tools", "").await;
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["disabled"], serde_json::json!(["get_bugs"]));
    assert_eq!(v["network_tools"], false);
    assert!(v.get("simple_hidden").is_none());
    assert_eq!(ctx.disabled_tools, vec!["get_bugs".to_string()], "nothing was saved");
}

#[tokio::test]
async fn a_hidden_tools_route_is_refused_with_the_simple_mode_sentence() {
    let ctx = saved();
    for path in SIMPLE_HIDDEN_ROUTES.iter().copied().chain(["/crystal-open", "/crystal-guide", "/crystal-preview-live"]) {
        for method in ["GET", "POST"] {
            assert_eq!(ask(true, &ctx, method, path, "{}").await, (404, SIMPLE_HIDES_TOOL.to_string()), "{method} {path}");
        }
        assert!(route_refusal(false, path).is_none(), "{path}");
    }
    assert!(SIMPLE_HIDES_TOOL.contains("Basic mode") && SIMPLE_HIDES_TOOL.contains("Settings"));
    assert!(!SIMPLE_HIDES_TOOL.contains("AI Bridge"), "the row is hidden, there is no switch to name");
    // The kept routes are not touched.
    for path in ["/current-pbi", "/tools", "/autorun-page", "/api-template-guide"] {
        assert!(route_refusal(true, path).is_none(), "{path}");
    }
    // With Basic mode off the same route is answered by its own handler, not the sentence.
    assert_ne!(ask(false, &ctx, "GET", "/tags", "").await.1, SIMPLE_HIDES_TOOL);
}

#[tokio::test]
async fn api_templates_run_under_simple_mode_with_the_saved_switch_off() {
    let _slot = crate::serial::api_template_run();
    let ctx = saved();
    assert!(!ctx.api_writes);
    // Off: refused by the switch's own sentence.
    let (status, out) = ask(false, &ctx, "POST", "/api-template-prove", "not json").await;
    assert_eq!((status, out.as_str()), (400, API_WRITES_OFF));
    // Under Basic mode the switch is not what stops it any more.
    let (_, out) = ask(true, &ctx, "POST", "/api-template-prove", "not json").await;
    // The next step of the call: the data folder (a test may or may not have one set by another
    // module) and then reading the body. Either way it is not the switch's sentence.
    assert!(
        out == "the app could not set up its data directory this session - restart the app" || out.starts_with("that is not readable JSON"),
        "the call got past the switch: {out}"
    );
    // A context that is not the active environment's still turns no write on.
    let mut other = saved();
    other.gate.env_id = None;
    let (status, out) = ask(true, &other, "POST", "/api-template-prove", "not json").await;
    assert_eq!(status, 400);
    assert_ne!(out, API_WRITES_OFF, "the card the plain sentence names is hidden under Basic mode");
    assert_eq!(out, SIMPLE_API_NO_ENVIRONMENT);
}

#[test]
fn the_api_switch_sentence_under_simple_mode_names_what_is_visible() {
    use v2_lib::simple_view::{api_writes_off_text, SIMPLE_API_SWITCHING};
    let mut no_env = saved();
    no_env.gate.env_id = None;
    let mut switching = saved();
    switching.gate.holds = 1;
    // Off Basic mode the sentence is the old one, whatever the context.
    assert_eq!(api_writes_off_text(false, &no_env), API_WRITES_OFF);
    assert_eq!(api_writes_off_text(false, &switching), API_WRITES_OFF);
    assert_eq!(api_writes_off_text(true, &no_env), SIMPLE_API_NO_ENVIRONMENT);
    assert_eq!(api_writes_off_text(true, &switching), SIMPLE_API_SWITCHING);
    assert!(SIMPLE_API_SWITCHING.contains("try again"));
    for said in [SIMPLE_API_NO_ENVIRONMENT, SIMPLE_API_SWITCHING] {
        assert!(!said.contains("API templates switch") && !said.contains("turn it on"), "{said}");
    }
    assert!(SIMPLE_API_NO_ENVIRONMENT.contains("Environment card"), "names a card Basic mode still shows");
}

#[test]
fn the_network_routes_are_open_under_simple_mode_with_the_saved_switch_off() {
    use v2_lib::ai_bridge::{network_guard, NETWORK_ROUTES, NETWORK_TOOLS_OFF};
    let ctx = saved();
    assert!(!ctx.network_tools);
    for path in NETWORK_ROUTES {
        assert!(network_guard(path, ctx.network_tools).is_some(), "{path}: refused by the saved switch");
        assert!(network_guard(path, context_under(true, &ctx).network_tools).is_none(), "{path}: open under Basic mode");
    }
    assert!(!NETWORK_TOOLS_OFF.is_empty());
}

#[test]
fn the_slash_commands_of_the_hidden_tools_are_not_written_under_simple_mode() {
    use v2_lib::ai_tools::{command_files_for, effective_disabled_under, COMMANDS};
    let all_on = v2_lib::modules::Module::ALL.into_iter().collect();
    let names = |simple: bool| -> Vec<String> {
        let disabled = effective_disabled_under(simple, &[], &all_on);
        command_files_for("/home/x", &disabled)
            .into_iter()
            .filter_map(|(p, _)| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .collect()
    };
    let off = names(false);
    for stem in ["begin-test-case-writing", "optimize"] {
        assert!(off.iter().any(|n| n == stem), "{stem} is written with Basic mode off");
    }
    let on = names(true);
    for c in COMMANDS.iter().filter(|c| SIMPLE_HIDDEN_TOOLS.contains(&c.tool)) {
        assert!(!on.iter().any(|n| n == c.stem), "{} is written under Basic mode", c.stem);
    }
    for stem in ["begin-test-case-writing", "optimize", "get-wiki-info", "page"] {
        assert!(!on.iter().any(|n| n == stem), "{stem}");
    }
    assert!(on.iter().any(|n| n == "setup"), "an Auto Test command stays");
}

#[test]
fn a_command_written_earlier_is_swept_when_simple_mode_hides_its_tool() {
    use v2_lib::ai_tools::effective_disabled_under;
    use v2_lib::commands::ai_tools::write_commands_with;
    let all_on = v2_lib::modules::Module::ALL.into_iter().collect();
    let dir = tempfile::tempdir().unwrap();
    let names = |d: &std::path::Path| -> Vec<String> {
        let mut n: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        n.sort();
        n
    };
    // Basic mode off: the whole set is written, begin-test-case-writing and optimize included.
    write_commands_with(dir.path(), &effective_disabled_under(false, &[], &all_on)).unwrap();
    for f in ["begin-test-case-writing.md", "optimize.md", "setup.md"] {
        assert!(names(dir.path()).iter().any(|n| n == f), "{f} written with Basic mode off");
    }
    // The same folder synced with Basic mode on: the earlier copies are removed, the Auto Test one stays.
    write_commands_with(dir.path(), &effective_disabled_under(true, &[], &all_on)).unwrap();
    let after = names(dir.path());
    for f in ["begin-test-case-writing.md", "optimize.md", "get-wiki-info.md", "page.md"] {
        assert!(!after.iter().any(|n| n == f), "{f} survived: {after:?}");
    }
    assert!(after.iter().any(|n| n == "setup.md"), "{after:?}");
    // And back: Basic mode off again writes them once more (nothing was saved).
    write_commands_with(dir.path(), &effective_disabled_under(false, &[], &all_on)).unwrap();
    assert!(names(dir.path()).iter().any(|n| n == "begin-test-case-writing.md"));
}

#[test]
fn the_proxy_hides_and_refuses_what_the_answer_names() {
    let hidden = serde_json::json!(SIMPLE_HIDDEN_TOOLS);
    let answer = serde_json::json!({ "disabled": [], "modules": ["auto_run", "api_templates", "db_access"], "simple_hidden": hidden, "network_tools": true });
    let (off, _) = v2_lib::mcp::tool_policy_from(Ok((200, answer.to_string())));
    for t in SIMPLE_HIDDEN_TOOLS {
        assert!(off.iter().any(|o| o == t), "{t}");
    }
    assert!(!off.iter().any(|o| o == "get_test_cases"));
    // A name this build does not hide is not taken on the answer's word.
    let odd = serde_json::json!({ "disabled": [], "modules": [], "simple_hidden": ["get_test_cases"] });
    let (off, _) = v2_lib::mcp::tool_policy_from(Ok((200, odd.to_string())));
    assert!(!off.iter().any(|o| o == "get_test_cases"));

    let body = answer.to_string();
    let call = |_: &str, _: &str, _: &str| Ok::<(u16, String), String>((200, body.clone()));
    let list = v2_lib::mcp::handle_message(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#, "1.0.0", &call).unwrap();
    let names: Vec<String> = serde_json::from_str::<serde_json::Value>(&list).unwrap()["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    for t in SIMPLE_HIDDEN_TOOLS {
        assert!(!names.iter().any(|n| n == t), "{t} is listed");
    }
    assert!(names.iter().any(|n| n == "get_test_cases") && names.iter().any(|n| n == "save_autorun_script"));
    for tool in ["search_pbis", "optimize_cases", "open_report"] {
        let req = format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{tool}","arguments":{{}}}}}}"#);
        let resp = v2_lib::mcp::handle_message(&req, "1.0.0", &call).unwrap();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], true, "{tool}");
        assert_eq!(v["result"]["content"][0]["text"], SIMPLE_HIDES_TOOL, "{tool}");
    }
}

#[tokio::test]
async fn the_network_refusal_under_simple_mode_names_the_environment_not_the_hidden_switch() {
    use v2_lib::ai_bridge::{NETWORK_ROUTES, NETWORK_TOOLS_OFF};
    use v2_lib::simple_view::{reword_network_refusal, SIMPLE_API_SWITCHING, SIMPLE_NETWORK_NO_ENVIRONMENT};
    let mut no_env = saved();
    no_env.gate.env_id = None;
    // Through the route: no environment -> the environment sentence; Simple off -> the old one.
    for path in NETWORK_ROUTES {
        assert_eq!(ask(true, &no_env, "GET", path, "{}").await, (403, SIMPLE_NETWORK_NO_ENVIRONMENT.to_string()), "{path}");
        assert_eq!(ask(false, &no_env, "GET", path, "{}").await, (403, NETWORK_TOOLS_OFF.to_string()), "{path}");
    }
    assert!(SIMPLE_NETWORK_NO_ENVIRONMENT.starts_with("Network tools need an environment."));
    // While a switch is in progress the sentence is the "being switched" one.
    let mut switching = saved();
    switching.gate.holds = 1;
    let off = (403, NETWORK_TOOLS_OFF.to_string());
    assert_eq!(reword_network_refusal(true, &switching, off.clone()), (403, SIMPLE_API_SWITCHING.to_string()));
    assert_eq!(reword_network_refusal(false, &switching, off.clone()), off);
    // Another answer is never touched.
    assert_eq!(reword_network_refusal(true, &no_env, (200, "{}".into())), (200, "{}".to_string()));
}

#[test]
fn the_proxys_network_refusal_under_simple_mode_names_the_environment() {
    use v2_lib::simple_view::SIMPLE_NETWORK_NO_ENVIRONMENT;
    let call_with = |body: String| {
        move |_: &str, _: &str, _: &str| Ok::<(u16, String), String>((200, body.clone()))
    };
    let tool = |call: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String>| -> String {
        let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"autorun_har","arguments":{}}}"#;
        let v: serde_json::Value = serde_json::from_str(&v2_lib::mcp::handle_message(req, "1.0.0", call).unwrap()).unwrap();
        assert_eq!(v["result"]["isError"], true);
        v["result"]["content"][0]["text"].as_str().unwrap().to_string()
    };
    let simple = serde_json::json!({ "disabled": [], "modules": ["db_access", "auto_run"], "network_tools": false, "simple_hidden": SIMPLE_HIDDEN_TOOLS });
    assert_eq!(tool(&call_with(simple.to_string())), SIMPLE_NETWORK_NO_ENVIRONMENT);
    // Not Simple: the switch's sentence, as before.
    let plain = serde_json::json!({ "disabled": [], "modules": ["db_access", "auto_run"], "network_tools": false });
    let said = tool(&call_with(plain.to_string()));
    assert!(said.contains("Network tools switch"), "{said}");
}

#[test]
fn the_proxys_simple_network_sentence_needs_the_live_app_and_auto_test_on() {
    use v2_lib::mcp::tools_reply_or_saved_simple;
    use v2_lib::simple_view::SIMPLE_NETWORK_NO_ENVIRONMENT;
    let text_for = |call: &dyn Fn(&str, &str, &str) -> Result<(u16, String), String>| -> String {
        let req = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"autorun_har","arguments":{}}}"#;
        let v: serde_json::Value = serde_json::from_str(&v2_lib::mcp::handle_message(req, "1.0.0", call).unwrap()).unwrap();
        v["result"]["content"][0]["text"].as_str().unwrap().to_string()
    };
    let live = |modules: serde_json::Value| {
        let body = serde_json::json!({ "disabled": [], "modules": modules, "network_tools": false, "simple_hidden": SIMPLE_HIDDEN_TOOLS }).to_string();
        move |_: &str, _: &str, _: &str| Ok::<(u16, String), String>((200, body.clone()))
    };
    // Live app, Auto Test on: the environment sentence.
    assert_eq!(text_for(&live(serde_json::json!(["db_access", "auto_run"]))), SIMPLE_NETWORK_NO_ENVIRONMENT);
    // Live app, Auto Test off: the module-off sentence.
    let off = text_for(&live(serde_json::json!([])));
    assert!(off.contains("not available") && off.contains("Auto Test"), "{off}");
    assert_ne!(off, SIMPLE_NETWORK_NO_ENVIRONMENT);
    // App closed (the saved answer, Simple saved on, Auto Test on): not the environment sentence.
    let saved = Some((v2_lib::modules::Module::ALL.into_iter().collect(), false));
    let closed = move |_: &str, _: &str, _: &str| tools_reply_or_saved_simple(Err("connection refused".into()), saved.clone(), true);
    let said = text_for(&closed);
    assert_ne!(said, SIMPLE_NETWORK_NO_ENVIRONMENT, "{said}");
}

#[test]
fn no_environment_wins_over_a_switch_in_progress() {
    use v2_lib::ai_bridge::NETWORK_TOOLS_OFF;
    use v2_lib::simple_view::{api_writes_off_text, reword_network_refusal, SIMPLE_API_SWITCHING, SIMPLE_NETWORK_NO_ENVIRONMENT};
    let mut ctx = saved();
    ctx.gate.env_id = None;
    ctx.gate.holds = 1;
    assert_eq!(api_writes_off_text(true, &ctx), SIMPLE_API_NO_ENVIRONMENT);
    assert_eq!(
        reword_network_refusal(true, &ctx, (403, NETWORK_TOOLS_OFF.to_string())).1,
        SIMPLE_NETWORK_NO_ENVIRONMENT
    );
    ctx.gate.env_id = Some("env-1".into());
    assert_eq!(api_writes_off_text(true, &ctx), SIMPLE_API_SWITCHING);
}

/// Basic mode (or a module) switched while no bridge ran leaves the files as they were;
/// the first push after start must write them for the view as it is now.
#[test]
fn the_first_push_after_a_switch_made_with_no_bridge_catches_the_files_up() {
    use std::collections::BTreeSet;
    use v2_lib::ai_tools::effective_disabled_under;
    use v2_lib::commands::ai_bridge::sync_needed;
    use v2_lib::commands::ai_tools::write_commands_with;
    use v2_lib::modules::Module;
    let all: BTreeSet<Module> = Module::ALL.into_iter().collect();
    let dir = tempfile::tempdir().unwrap();
    let has = |f: &str| dir.path().join(f).exists();
    let key = |simple: bool, on: &BTreeSet<Module>| {
        (disabled_under(simple, &effective_disabled_under(false, &[], on)), None, on.clone())
    };
    // An earlier run wrote the files with Basic mode off.
    write_commands_with(dir.path(), &effective_disabled_under(false, &[], &all)).unwrap();
    assert!(has("begin-test-case-writing.md") && has("optimize.md"));
    // Basic mode was switched on while the bridge was not running. This process remembers nothing,
    // so the first push syncs - and the files then follow the Simple view.
    let mut last = None;
    assert!(v2_lib::commands::ai_bridge::sync_needed(&mut last, key(true, &all)));
    write_commands_with(dir.path(), &effective_disabled_under(true, &[], &all)).unwrap();
    assert!(!has("begin-test-case-writing.md") && !has("optimize.md") && has("setup.md"));
    // The same view again writes nothing; a Simple switch or a module switch does.
    assert!(!sync_needed(&mut last, key(true, &all)));
    assert!(sync_needed(&mut last, key(false, &all)));
    let mut no_playwright = all.clone();
    no_playwright.remove(&Module::Playwright);
    assert!(sync_needed(&mut last, key(false, &no_playwright)));
}

#[test]
fn a_switch_in_progress_keeps_the_placeholder_list_under_basic_mode() {
    // The placeholder a switch leaves: a shown tool off until the push.
    let mut held = saved();
    held.gate.holds = 1;
    held.disabled_tools = vec!["get_bugs".into(), "get_test_cases".into()];
    let under = disabled_under_ctx(true, &held);
    assert!(under.iter().any(|d| d == "get_bugs"), "kept while switching, as with Basic mode off");
    assert!(under.iter().any(|d| d == "get_test_cases"));
    assert!(SIMPLE_HIDDEN_TOOLS.iter().all(|t| under.iter().any(|d| d == t)), "the hidden tools are added");
    assert_eq!(context_under(true, &held).disabled_tools, under);
    assert_eq!(disabled_under_ctx(false, &held), held.disabled_tools, "Basic mode off: unchanged");
    // Outside a switch the saved list is ignored.
    let mut idle = held.clone();
    idle.gate.holds = 0;
    let under = disabled_under_ctx(true, &idle);
    assert!(!under.iter().any(|d| d == "get_bugs"));
    assert_eq!(under.len(), SIMPLE_HIDDEN_TOOLS.len());
}
