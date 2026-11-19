//! How app Simple mode looks to the assistants: a VIEW over the saved
//! settings, never a change to them. While the saved Simple mode setting is on,
//! the tools an assistant gets are trimmed (the writing, search and report
//! tools are hidden and refused) and API templates and the network tools are
//! on whatever their switches say. Turning Simple mode off shows every saved
//! switch exactly as it was. Nothing here writes the setting
//! (`simple_mode` owns that), so it can be read from the bridge and the proxy.

/// What a call to a tool Simple mode hides answers.
pub const SIMPLE_HIDES_TOOL: &str = "Simple mode hides this tool. Turn Simple mode off in Settings to use it.";

/// The tools an assistant does not get while app Simple mode is on: it reads
/// test cases and writes Auto Test scripts, and nothing else of the writing,
/// searching and report tools. The ONE list: `src/lib/mcpTools.ts` mirrors it
/// (a test compares the two). Simple mode only hides them - no saved switch
/// changes - so turning it off brings them all back.
pub const SIMPLE_HIDDEN_TOOLS: &[&str] = &[
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
    "open_report",
    "read_report",
    "edit_report",
    "compare_reports",
    "check_report",
    "preview_report",
    "preview_report_live",
    "get_report_parameters",
    "find_in_report",
    "replace_in_report",
];

/// The bridge routes of the hidden tools, answered 404 with `SIMPLE_HIDES_TOOL`
/// while Simple mode is on. Every Crystal Reports route starts with
/// `CRYSTAL_ROUTE_PREFIX`.
pub const SIMPLE_HIDDEN_ROUTES: &[&str] = &[
    "/begin",
    "/check-coverage",
    "/merge-cases",
    "/validate",
    "/tags",
    "/optimize",
    "/transform",
    "/search-pbis",
    "/search-wiki",
    "/wiki-page",
];
pub const CRYSTAL_ROUTE_PREFIX: &str = "/crystal-";

/// Whether the saved app setting has Simple mode on, in this process.
pub fn is_on() -> bool {
    matches!(crate::app_settings::current().app_simple_mode, Some(true))
}

/// Whether a bridge route belongs to a tool Simple mode hides.
pub fn hides_route(path: &str) -> bool {
    SIMPLE_HIDDEN_ROUTES.contains(&path) || path.starts_with(CRYSTAL_ROUTE_PREFIX)
}

/// The route guard: the refusal for a hidden tool's route while `simple`.
pub fn route_refusal(simple: bool, path: &str) -> Option<(u16, String)> {
    (simple && hides_route(path)).then(|| (404, SIMPLE_HIDES_TOOL.to_string()))
}

/// The saved disabled list with the hidden tools added while `simple`;
/// unchanged (and not saved anywhere) otherwise.
pub fn disabled_under(simple: bool, saved: &[String]) -> Vec<String> {
    let mut out = saved.to_vec();
    if simple {
        for t in SIMPLE_HIDDEN_TOOLS {
            if !out.iter().any(|o| o == t) {
                out.push((*t).to_string());
            }
        }
    }
    out
}

/// `disabled_under` for the saved setting of this process.
pub fn disabled_now(saved: &[String]) -> Vec<String> {
    disabled_under(is_on(), saved)
}

/// The bridge's context as the routes and the tool list see it while
/// `simple`: the hidden tools disabled, the network tools on, and API
/// templates on - but only where a write could be on at all (a context for
/// the active environment, with no switch in progress), so the rule that a
/// push for another environment turns no write on still holds. A view only:
/// the stored context keeps the saved values, so turning Simple mode off
/// brings every one back.
pub fn context_under(simple: bool, ctx: &crate::ai_bridge::BridgeContext) -> crate::ai_bridge::BridgeContext {
    let mut out = ctx.clone();
    if simple {
        out.disabled_tools = disabled_under(true, &ctx.disabled_tools);
        // Like the write switch below, the network tools come on only for a
        // context that names an environment (consistent with
        // `push_context_with`, which never turns one on for another).
        out.network_tools = ctx.gate.env_id.is_some();
        if ctx.gate.env_id.is_some() && ctx.gate.holds == 0 {
            out.api_writes = true;
        }
    }
    out
}


/// Whether the SAVED settings in this machine's app data folder have Simple
/// mode on: what the `--mcp` proxy reads when the app is closed, from the
/// same folder as `modules::saved_on_this_machine`.
pub fn saved_on_this_machine() -> bool {
    crate::modules::data_dir_in(std::env::var_os("APPDATA"))
        .filter(|d| d.join("app-settings.json").exists())
        .is_some_and(|d| matches!(crate::app_settings::load(&d).app_simple_mode, Some(true)))
}

/// What an API template call answers while its switch is not on (see
/// `API_WRITES_OFF`). Under Simple mode the API templates card is hidden, so
/// the plain sentence would point at a control nobody can see: say why
/// instead. `ctx` is the context a handler was given (Simple's view of it).
pub fn api_writes_off_text(simple: bool, ctx: &crate::ai_bridge::BridgeContext) -> String {
    if !simple {
        return crate::ai_bridge::API_WRITES_OFF.to_string();
    }
    if ctx.gate.holds > 0 {
        SIMPLE_API_SWITCHING.to_string()
    } else {
        SIMPLE_API_NO_ENVIRONMENT.to_string()
    }
}

/// Simple mode, API templates, a switch of environment still in progress.
pub const SIMPLE_API_SWITCHING: &str = "The environment is being switched. Wait for it to finish, then try again.";
/// Simple mode, API templates, no environment chosen yet.
pub const SIMPLE_API_NO_ENVIRONMENT: &str =
    "API templates need an environment. Choose one in the Environment card on the AI Bridge tab, then try again.";

/// Under `simple`, an answer that is the plain API switch sentence (from any
/// of the routes that return it) becomes Simple mode's own, by `ctx`.
pub fn reword_api_switch_refusal(simple: bool, ctx: &crate::ai_bridge::BridgeContext, answer: (u16, String)) -> (u16, String) {
    if simple && answer.1 == crate::ai_bridge::API_WRITES_OFF {
        (answer.0, api_writes_off_text(true, ctx))
    } else {
        answer
    }
}

/// Under Simple mode the proxy is told which tools are hidden, by name,
/// because it drops a core tool from `disabled` (a core tool has no switch).
/// Added to the `/tools` answer only then, so with Simple mode off the answer
/// is what it always was.
pub fn name_hidden_in_tools_answer(simple: bool, method: &str, path: &str, answer: (u16, String)) -> (u16, String) {
    if !(simple && method == "GET" && path == "/tools" && answer.0 == 200) {
        return answer;
    }
    match serde_json::from_str::<serde_json::Value>(&answer.1) {
        Ok(mut v) if v.is_object() => {
            v["simple_hidden"] = serde_json::json!(SIMPLE_HIDDEN_TOOLS);
            (200, v.to_string())
        }
        _ => answer,
    }
}
