//! MCP (Model Context Protocol) dispatcher, run in-process via `--mcp`.
//! `handle_message` is pure: JSON-RPC message string in, response string
//! out (None for notifications). The bridge call is injected so tests run
//! without stdio or sockets. `run_stdio_proxy` owns the transport framing
//! (newline-delimited stdio) and the HTTP client that proxies to the app's
//! localhost bridge via the handshake file.

use std::collections::BTreeSet;
use std::io::{BufRead, Write};

use crate::modules::Module;

type BridgeCall<'a> = &'a dyn Fn(&str, &str, &str) -> Result<(u16, String), String>;

/// This proxy's own version - the build it was compiled from. The release
/// script refuses to ship unless Cargo.toml and tauri.conf.json agree, so
/// it is the app version this proxy's tool list belongs to.
pub const PROXY_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The header every bridge call carries `PROXY_VERSION` in, so the app can
/// log a proxy that is not its own build.
pub const PROXY_VERSION_HEADER: &str = "x-tcm-proxy-version";

/// The header every bridge request made for a tool call carries the tool's
/// name in, so the app can log one line per call
/// (`ai_bridge::tool_call_line`). Only the name: never an argument.
pub const TOOL_HEADER: &str = "x-tcm-tool";

thread_local! {
    /// The tool `tools_call` is answering on this thread, for `TOOL_HEADER`.
    static CALLING: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Names the tool being called for as long as it lives. A name that is not
/// tool-shaped (lowercase letters, digits and underscores, at most 64) is
/// not named: it would be the assistant's own text, and an invalid header
/// value would fail the request.
struct CallingTool;

impl CallingTool {
    fn set(name: &str) -> CallingTool {
        let shaped = !name.is_empty()
            && name.len() <= 64
            && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        CALLING.with(|c| *c.borrow_mut() = shaped.then(|| name.to_string()));
        CallingTool
    }
}

impl Drop for CallingTool {
    fn drop(&mut self) {
        CALLING.with(|c| *c.borrow_mut() = None);
    }
}

/// The tool a call on this thread is being answered for, if any.
pub fn calling_tool() -> Option<String> {
    CALLING.with(|c| c.borrow().clone())
}

/// What to say when this proxy and the running app are different builds,
/// or None when they match or the app's version is not known (not running
/// yet is not a mismatch).
///
/// The tool list is compiled into the proxy, not asked of the app, so a
/// stale proxy serves a stale list. That happened for weeks on 2026-09:
/// Claude desktop is a packaged app, Windows kept a private copy of the
/// install folder under its LocalCache from 5 September, and every session
/// started that 1.22.1 proxy against a 1.25.25 app - reporting 1.25.25,
/// because the version then came from the app's handshake file.
pub fn version_warning(proxy: &str, app: &str) -> Option<String> {
    let (proxy, app) = (proxy.trim(), app.trim());
    if proxy.is_empty() || app.is_empty() || app == "unknown" || proxy == app {
        return None;
    }
    Some(format!(
        "The tcm MCP server is version {proxy}, but the running Test Case Manager is {app}. \
         The tools offered here are {proxy}'s: any tool added since is missing. The usual cause is a \
         stale copy of the app that Claude desktop, a packaged app, keeps under \
         %LOCALAPPDATA%\\Packages\\<Claude package>\\LocalCache\\Local\\AzureDevOpsTestCaseManager.V2 - \
         quit Claude, rename or delete that folder, and start a new session. If there is no such \
         folder, the app was updated after this session started: start a new session."
    ))
}

/// A JSON-RPC error object, for the cases where there is nothing else to
/// say. `id` is null when the message could not be parsed far enough to
/// find one - which is what the protocol prescribes, and is still an
/// answer: returning None left the client waiting on an id forever.
fn rpc_error(id: serde_json::Value, code: i32, message: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

pub fn handle_message(msg: &str, version: &str, call: BridgeCall) -> Option<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(msg) else {
        return Some(rpc_error(serde_json::Value::Null, -32700, "Parse error"));
    };
    let id = v.get("id").cloned();
    // Notifications (no id) never get a response - that part was right.
    id.as_ref()?;
    let id = id.unwrap();
    let Some(method) = v["method"].as_str() else {
        // Has an id, so it is a request and something must come back.
        return Some(rpc_error(id, -32600, "Invalid Request: no method"));
    };

    let result = match method {
        "initialize" => {
            // `version` is the RUNNING APP's; serverInfo reports this proxy's
            // own, so the two can be compared at all.
            let mut result = serde_json::json!({
                "protocolVersion": v["params"]["protocolVersion"].as_str().unwrap_or("2024-11-05"),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": crate::ai_tools::TCM_SERVER, "version": PROXY_VERSION },
            });
            // `instructions` is where an MCP client hands the server's own
            // words to the assistant - the one reader who can act on it.
            if let Some(warning) = version_warning(PROXY_VERSION, version) {
                result["instructions"] = serde_json::Value::String(warning);
            }
            result
        }
        "tools/list" => {
            let reply = call("GET", "/tools", "");
            let no_ask = db_no_ask_from(&reply);
            tools_list(tool_policy_from(reply).0, no_ask)
        }
        "tools/call" => tools_call(&v["params"], call),
        _ => {
            return Some(
                serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": format!("unknown method {method}") },
                })
                .to_string(),
            )
        }
    };
    Some(serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string())
}

fn schema(props: serde_json::Value, required: &[&str]) -> serde_json::Value {
    serde_json::json!({ "type": "object", "properties": props, "required": required })
}

/// The `cases` a quirk can be tied to when it is recorded on its own: the
/// cases and steps it explains, each step one that failed in its case's
/// newest run.
fn quirk_cases() -> serde_json::Value {
    serde_json::json!({
        "type": "array",
        "description": "Optional: the cases and steps this quirk explains - each step must have failed in its case's newest run on this machine. Later runs of those steps count whether the quirk helped.",
        "items": {
            "type": "object",
            "properties": {
                "case_id": { "type": "number" },
                "steps": { "type": "array", "items": { "type": "number" } },
            },
            "required": ["case_id", "steps"],
        },
    })
}

/// What the app says about its tools right now: the disabled set as it is
/// applied, and which modules are on. Asked fresh on every `tools/list`
/// and `tools/call`, so a toggle - a tool's switch or a whole module -
/// takes effect without restarting the editor.
///
/// A bridge that can't be reached disables nothing extra among the core
/// tools - losing the whole toolset because the app is closed would be
/// worse than showing tools that then say "sign in first". It never WIDENS
/// anything on a guess: a module's tools are offered only when `modules`
/// names it - an app too old to send the field or a garbled answer mean "no
/// module on". When the app does not answer at all, the proxy's `bridge_call`
/// substitutes this machine's saved modules first (`tools_reply_or_saved`),
/// so this function still only ever sees an answer. A development build
/// adds nothing: its app says what it has on like any other. The network
/// tools are offered only when the answer says `network_tools: true` - the
/// person's own switch, off by default - so a saved-modules answer, an older
/// app or a garbled one never offers them.
pub fn tool_policy_from(reply: Result<(u16, String), String>) -> (Vec<String>, BTreeSet<Module>) {
    let answer = match reply {
        Ok((200, body)) => serde_json::from_str::<serde_json::Value>(&body).ok(),
        _ => None,
    };
    // Mended like a stored choice: a module whose parent is not named is
    // off, so a confused answer can never offer a tool the tree forbids.
    let on: BTreeSet<Module> = crate::modules::normalise(
        answer
            .as_ref()
            .and_then(|v| v["modules"].as_array().cloned())
            .map(|a| a.iter().filter_map(|n| n.as_str().and_then(Module::from_key)).collect())
            .unwrap_or_default(),
    );
    let list: Vec<String> = answer
        .as_ref()
        .and_then(|v| v["disabled"].as_array().cloned())
        .map(|a| {
            a.iter()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let mut off = crate::ai_tools::effective_disabled_for(&list, &on);
    let network_on = answer.as_ref().is_some_and(|v| v["network_tools"].as_bool() == Some(true));
    if !network_on {
        for t in crate::ai_tools::NETWORK_TOOLS {
            if !off.iter().any(|o| o == t) {
                off.push(t.to_string());
            }
        }
    }
    (off, on)
}

/// Whether the person has let database changes run without being asked:
/// the app's switch for that AND writes switched on, as the bridge's
/// `/tools` answer says. Anything short of an explicit `true` - no app, an
/// older app, a failed answer - is no, so the description only ever loses
/// its "do not ask" line, never gains one by accident.
pub fn db_no_ask_from(reply: &Result<(u16, String), String>) -> bool {
    match reply {
        Ok((200, body)) => serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|v| v["db_no_ask"].as_bool())
            .unwrap_or(false),
        _ => false,
    }
}

/// Said to the assistant, at the end of `db_query`'s description, while the
/// person has switched on running database changes without being asked.
pub const DB_NO_ASK: &str = " The person has allowed database changes without confirmation: do not stop to ask before running INSERT, UPDATE, DELETE or a procedure through this tool - run the batch with dry_run first, read the row counts, then run it for real, and tell them what changed afterwards.";

/// `preview_report_live`'s own tail for the same switch: it has no batch
/// and no dry run, so `DB_NO_ASK`'s advice means nothing there.
pub const PREVIEW_LIVE_NO_ASK: &str = " The person has allowed database changes without confirmation, so this runs the report's stored procedure without a prompt: check the parameter values first, since the procedure runs with exactly those, and tell them what ran afterwards.";

fn tool_policy(call: BridgeCall) -> (Vec<String>, BTreeSet<Module>) {
    tool_policy_from(call("GET", "/tools", ""))
}

/// Every tool this server offers, by name.
pub fn all_tool_names() -> Vec<String> {
    tools_list(vec![], false)["tools"]
        .as_array()
        .map(|tools| tools.iter().filter_map(|t| t["name"].as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn tools_list(disabled: Vec<String>, db_no_ask: bool) -> serde_json::Value {
    let ops = crate::transform::SUPPORTED_OPS.join(", ");
    let transform_ops_desc = format!(
        "Ops applied in order, each with an optional `where` (title_contains, has_tag, module_is, area_is, at_index). \
         Every op the server accepts: {ops}. Which keys each op reads, and the selectors: get_guide (topic writing), \
         its \"Transform ops\" part. There is no op for `comment`: it is the developer's field and is never written \
         by an assistant. A key an op does not read is reported in `ignored`, never silently dropped."
    );
    let all = serde_json::json!({ "tools": [
        {
            "name": "begin_test_case_writing",
            "description": "START HERE for any test-case writing job, before reading specs or drafting anything. Call it with only `feature` first: it returns the questions to put to the developer in chat (what the file is called, which spec documents are authoritative, whether to check a PBI for duplicates, tags/module/status, what is out of scope) plus this org's real Module values. Ask them those questions - do not answer them yourself - then call this tool again with their answers. It checks the paths and values actually exist, and returns a plan file for them to approve. Do not write a single test case until it returns status \"ready\" and the developer has agreed to the plan.",
            "inputSchema": schema(serde_json::json!({
                "feature": { "type": "string", "description": "What you are writing cases for - the plan's title" },
                "output_path": { "type": "string", "description": "File name for the finished JSON, in the repository's .test-cases folder; a path outside it is refused" },
                "spec_paths": { "type": "array", "items": { "type": "string" }, "description": "Full paths to the spec documents" },
                "sections": { "type": "string", "description": "Which parts are in scope" },
                "authority": { "type": "string", "description": "\"spec\", \"app\" or \"spec-wins\"" },
                "ordering": { "type": "string", "description": "\"spec\" or \"tester\"" },
                "reference_cases": { "type": "string", "description": "The developer's answer on reference cases: a PBI id, a path, prose, or \"none\". Required" },
                "examples_pbi": { "type": "integer", "description": "PBI whose cases set the style and are checked for duplicates" },
                "check_examples": { "type": "boolean", "description": "Whether to read them" },
                "tags": { "type": "string", "description": "Semicolon-separated" },
                "module": { "type": "string", "description": "One of the org's allowed Module values" },
                "automation_status": { "type": "string", "description": "\"Not Automated\" or \"Planned\"" },
                "out_of_scope": { "type": "string", "description": "What not to cover" },
                "notes": { "type": "string", "description": "The developer's own conventions" },
            }), &[]),
        },
        guide_tool(&disabled),
        {
            "name": "get_test_cases",
            "description": "Test cases in the exact import JSON shape. Pass `pbi_id` for the cases a PBI is tested by, `case_ids` to read specific cases by their own ids without knowing their PBI, or both to narrow a PBI's cases to those ids (any id the PBI is not tested by is listed in `not_on_pbi`). Use them to copy the house style, to check what is already covered before writing more, or to read a case someone named by its id.",
            "inputSchema": schema(serde_json::json!({
                "pbi_id": { "type": "integer", "description": "Work item id of the PBI - optional when case_ids is given" },
                "case_ids": { "type": "array", "items": { "type": "integer" }, "description": "Test case work item ids to read (up to 200) - optional when pbi_id is given" },
                "limit": { "type": "integer", "description": "Max cases (default 5, cap 20)" },
                "offset": { "type": "integer", "description": "Skip this many cases - page through a PBI with more than the cap" },
                "titles_only": { "type": "boolean", "description": "Return only ids and titles (cap 200) - use for duplicate checking instead of pulling full step text" },
            }), &[]),
        },
        {
            "name": "search_test_suites",
            "description": "The test plans and suites in the current project, one row per suite with its plan id, suite id, type (staticTestSuite, requirementTestSuite, dynamicTestSuite), the PBI a requirement suite is bound to, and its parent suite. Filter with `query` against the plan name, the suite name, or a PBI id - a project can hold hundreds of plans, so filter rather than reading all of them. Use it to find a suite before calling get_suite_test_cases, or to read how a feature's cases are organised across plans. Served from a ten-minute cache; pass `refresh` after suites were created or moved.",
            "inputSchema": schema(serde_json::json!({
                "query": { "type": "string", "description": "Case-insensitive text matched against plan names and suite names, or a PBI id to find its requirement suite" },
                "refresh": { "type": "boolean", "description": "Read the plans again instead of the cached tree" },
            }), &[]),
        },
        {
            "name": "get_suite_test_cases",
            "description": "The test cases in one test suite, in the exact import JSON shape, in the suite's own order. Takes the plan id and suite id that search_test_suites returns. A folder suite lists its own cases only unless `include_children` is set, which reads every suite beneath it in one call. Use it to copy the house style of a suite, to check what a suite already covers, or to read a static suite that has no PBI for get_test_cases to take.",
            "inputSchema": schema(serde_json::json!({
                "plan_id": { "type": "integer", "description": "Test plan id, from search_test_suites" },
                "suite_id": { "type": "integer", "description": "Test suite id, from search_test_suites" },
                "include_children": { "type": "boolean", "description": "Also read the cases of every suite beneath this one - for a folder suite" },
                "limit": { "type": "integer", "description": "Max cases (default 5, cap 20)" },
                "offset": { "type": "integer", "description": "Skip this many cases - page through a suite with more than the cap" },
                "titles_only": { "type": "boolean", "description": "Return only ids and titles (cap 200) - use for duplicate checking instead of pulling full step text" },
            }), &["plan_id", "suite_id"]),
        },
        {
            "name": "get_run_results",
            "description": "The cases from a PBI's latest test runs with the outcomes you ask for - Failed unless you pass `outcome` - each with the tester's comment and any bugs they linked, plus `summary`: a count of every outcome in the PBI's suite (Passed, Failed, Blocked, Never run...). Use the summary to see where a PBI's testing stands; use the failures to write regression cases for what actually broke: read the failure, read the failed case itself with get_test_cases, then extend the coverage rather than restating it.",
            "inputSchema": schema(serde_json::json!({
                "pbi_id": { "type": "integer", "description": "Work item id of the PBI whose runs to read" },
                "outcome": {
                    "description": "Which outcomes to list: one, a list, or \"all\". Failed, Blocked, Paused, In progress, Not applicable, Passed, Never run (not run yet), or an automated one (Error, Timeout, Aborted, Inconclusive, Warning, Not executed, Not impacted). Default: Failed.",
                    "anyOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ]
                },
            }), &["pbi_id"]),
        },
        {
            "name": "check_spec_coverage",
            "description": "Reports coverage as findings to read and account for, not as pass/fail - a partial draft is a normal state, not an error. Joins a draft's `Spec:` citations against one or more spec documents and returns the sections with no case yet (`uncovered`), cases attributed to no section, citations pointing at a section or file that does not exist, quoted text not found in the document, citations with no quote and no exemption (`cited_without_quote` - fix these here, do not wait for validate_cases), and sections the plan's own scope excludes. A citation may stop short of a heading's trailing parenthetical, and `;` may join a second document's pointer onto the same Spec: line - both resolve. AC-level sections (e.g. \"8.2 (AC-2)\") are reported individually - a covered parent does not silence its acceptance criteria. Run before optimize_cases so gaps are found while the draft is still easy to extend.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object) - use this or `path`, never both" },
                "path": { "type": "string", "description": "Absolute path to a local draft file - use this or `json`, never both" },
                "spec_paths": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Absolute paths to the specification documents the draft cites",
                },
                "sections": { "type": "string", "description": "Which sections are in scope for this batch, if narrower than the whole document" },
                "out_of_scope": { "type": "string", "description": "Anything explicitly not covered by this batch, e.g. \"7.4 is deferred to phase 2\"" },
            }), &["spec_paths"]),
        },
        {
            "name": "merge_case_files",
            "description": "Merge slice files from a fan-out into one draft through the real importer - never merge by hand. Reads every path in `paths` with the importer the app uses, concatenates the cases in that order, and writes `output_path` (refused if that path already exists - pick a new one rather than overwriting). Returns the merged case count, a per-file breakdown and every slice's importer warnings, each prefixed with its slice file. A title in more than one slice is warned about by name: such a collision usually needs disambiguating, not deduping. Does not deduplicate - run optimize_cases or transform_cases' dedupe on the merged file if slices may overlap. The response names the slice files it consumed as superseded; remove them from .test-cases yourself - this tool deletes nothing. The merged `specs` is the union of the slices'; only .md files and Azure DevOps wiki links are kept, and each other entry is dropped and named in the warnings.",
            "inputSchema": schema(serde_json::json!({
                "paths": { "type": "array", "items": { "type": "string" }, "description": "Absolute paths to the slice files, in the order they should be concatenated" },
                "output_path": { "type": "string", "description": "File name for the merged draft (e.g. login.json) - it goes in the working repository's .test-cases folder, and must not already exist; a path outside that folder is refused" },
            }), &["paths", "output_path"]),
        },
        {
            "name": "save_autorun_script",
            "description": format!("Save action scripts so the app can drive those test cases through a real browser. Takes a LIST, one entry per case, so one call can cover a whole PBI: {{ case_id, title, account (optional: the KEY of the account the case runs as, never a username or password), area, steps: [{{ step_number, actions }}] }}. {} Every script is checked against its OWN test case before anything is written: a step whose expected result nothing asserts is refused unless that step says why in `unchecked`. Every locator must have been seen on the live app - an action needs one seen matching exactly one element; a refusal says what each use needs. CHANGING a script that already exists is a repair and needs `edits`. Steps you leave out of `edits` must stay exactly as they were (a declared step may change in any way, removing a check included), and a precondition removed or changed without a precondition edit is refused. All or nothing: one bad action, locator, case id or undeclared change rejects the whole batch. Call get_guide (topic autorun) first for the action vocabulary; its section \"Saving it\" has the seen-locator rules and the shape of `edits`.", crate::autorun::nav::SET_AREA_RULE),
            "inputSchema": schema(serde_json::json!({
                "scripts": {
                    "type": "array",
                    "description": "One entry per test case: { case_id, title, account?, area, steps, changes?, needs_unchanged?, needs_state? } (needs_state maps a setting to its value, e.g. { \"Approval workflow\": \"ON\" }). A repair may send patch_steps in place of steps: get_guide (topic autorun), section \"Saving it\".",
                    "items": { "type": "object" },
                },
                "edits": {
                    "type": "array",
                    "description": "Only when a script already exists: one entry per case you change, { case_id, steps (every step number you changed), why, area? (true when you changed the script's area, including leaving out one it had), quirk?, precondition? (at most one per case per save) }: get_guide (topic autorun), section \"Saving it\".",
                    "items": { "type": "object" },
                },
                "dry_run": { "type": "boolean", "description": DRY_RUN_SCRIPT },
            }), &["scripts"]),
        },
        {
            "name": "get_autorun_script",
            "description": "Read one test case's saved Auto Run script, whole, as the JSON save_autorun_script takes for one entry: { case_id, title, account, area, steps, ... } with its repairs, its suspected-defect mark and when it was saved. Read it before you repair or delete a script. Answers 404 with a sentence when the case has no script on this machine.",
            "inputSchema": schema(serde_json::json!({
                "case_id": { "type": "number", "description": "The test case whose script to read." },
            }), &["case_id"]),
        },
        {
            "name": "delete_autorun_script",
            "description": "Delete one test case's saved Auto Run script, when the case no longer needs one. It is not removed at once: it is moved aside on this machine, out of every list and run, and purged after 30 days; Auto Run shows the case with no script straight away. Refused while that case is being replayed, by replay_autorun_to_step or a run: wait for it to end. Refused for a case with no script. Never call it to start a script again from nothing: a repair is a save with `edits`.",
            "inputSchema": schema(serde_json::json!({
                "case_id": { "type": "number", "description": "The test case whose script to delete." },
            }), &["case_id"]),
        },
        {
            "name": "get_autorun_page",
            "description": "Read the page in the Auto Run browser (the one the person opened, or your discovery browser) as text: Chrome's accessibility tree, one element per line, with the locator that reaches it on the end of each line. This is how you see a page before writing or repairing a script; what a password field holds is never shown. Visible text that no line's name says prints as a `text \"...\"` line; a read never counts one as matching exactly one element, so probe it before an action or a one-element check uses it. It answers the first 120 lines and how many more there are: ask for more with `limit`, or list one part with `within`. During a discovery with an area, a read records what it shows as seen under that area; only a read of the whole page marks the area explored. Needs a supervised or discovery browser to be open.",
            "inputSchema": schema(serde_json::json!({
                "limit": { "type": "number", "description": "How many lines before the snapshot stops (default 120, at most 3000)." },
                "within": {
                    "type": ["object", "string", "array"],
                    "items": { "type": "object" },
                    "description": "A locator, written as a script writes one (a css string, a role-and-name object, or a chain): list only what is inside the one element it matches, such as a dialog or a form. A css string counts every visible match. Refused, naming the count and up to 3 of the matches, when it matches none or several.",
                },
                "nth": { "type": "number", "description": "Optional, with `within`: which of the elements it matches to list, counting from 0, as a locator's nth does." },
            }), &[]),
        },
        {
            "name": "probe_autorun_locator",
            "description": "Ask the open browser what a locator matches RIGHT NOW: how many elements, and for each its tag, text, whether it is visible, where it sits, and its role and name. Use it before putting a locator in a script - one that matches three things is a script that clicks the wrong one. What it matched is recorded as seen, with how many elements it matched: an action needs a locator seen matching exactly one, while a check that can pass with several matches (expect_hidden, expect_count, wait_for) may use one seen matching several. A chain can pass through a same-origin iframe. Needs a supervised or discovery browser to be open. With `show` it looks deeper at ONE element instead (refused when the locator matches nothing or more than one, and nothing is recorded as seen): `styles` (computed, all or the `properties` you name), `listeners` (where each handler was written, never its source; under `listeners.ancestors` the ones above it up to the window), `animations` and `structure`, and whether a click at its centre reaches it or what covers it - see get_guide (topic autorun), section \"Network, cookies and elements\". A password field's value, attributes named like a secret and tokens in text are hidden. Read only: nothing is clicked, typed, focused or scrolled, so it works in the person's browser too.",
            "inputSchema": schema(serde_json::json!({
                "selector": {
                    "type": ["object", "string", "array"],
                    "items": { "type": "object" },
                    "description": "A script target, exactly as the script writes it: a CSS selector string, the role-and-accessible-name object a snapshot line ends with, or a chain - a list of those objects, outermost first (a dialog, then the button inside it). A chain is recorded as seen as a whole.",
                },
                "show": {
                    "type": ["array", "boolean"],
                    "items": { "type": "string", "enum": SHOW_PARTS },
                    "description": "Optional: the parts of the one element to show, or true for all four.",
                },
                "properties": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional, with `show`: only these computed styles, e.g. [\"color\", \"pointer-events\"].",
                },
            }), &["selector"]),
        },
        {
            "name": "try_autorun_action",
            "description": "Carry out ONE script action against the open browser, so you can find out whether a repair works before you write it down. Name the case the try is for with `case_id`: a case marked no_save is tried with its saves stopped, as in a run. Answers ok or failed with what the page did, and the picture taken if it failed. Nothing is recorded - this is a rehearsal, not a run. `sign_in` is refused: the person signs in. Needs a supervised browser to be open.",
            "inputSchema": schema(serde_json::json!({
                "action": {
                    "type": "object",
                    "description": "One action in the script vocabulary, e.g. { \"kind\": \"click\", \"selector\": ... } - call get_guide (topic autorun) for all of them.",
                },
                "case_id": {
                    "type": "number",
                    "description": "The case this try is for (required). A case marked no_save is tried with its saves stopped, as in a run.",
                },
            }), &["action", "case_id"]),
        },
        {
            "name": "replay_autorun_to_step",
            "description": "Replay a case's saved script in the supervised Auto Run browser, steps 1 to `step` - 1, and stop before `step` runs, so you can heal the failing step where it failed. Needs no browser open: when none is, the app opens one; a discovery of yours holding the browser is ended first (what it mapped is kept). Like a run, it checks the preconditions, signs in as the case's account and goes to its area. A script marked must not save replays at once too, its saves stopped throughout. Answers a sentence and the page where it stopped, so you can try that step at once with try_autorun_action. Replay to the failing step and never past it to see what happens. The browser stays open on that step, signed in. Do not release it between replays: reset_autorun_browser gives a clean page with the sign-in kept, and its `release` is for a broken browser only. One replay at a time: one still going after about 45 seconds says so - call again with the same case and step to keep waiting on that same replay (never a new one); a different case or step is refused until it ends. `stop: true`, sent alone, stops your replay that is still going; with no replay going it answers 404, and the person's own replay is refused (only they can stop it). Carrying on from an earlier replay, a browser already signed in, and how a stop ends: get_guide (topic autorun), section \"Repairing a script that failed\".",
            "inputSchema": schema(serde_json::json!({
                "case_id": { "type": "number", "description": "The case whose saved script is replayed." },
                "step": { "type": "number", "description": "The step to stop before: the failing step. Steps 1 to step - 1 run." },
                "page": { "type": "boolean", "description": "Optional: false leaves the page out of the answer; get_autorun_page reads it. Default true." },
                "stop": { "type": "boolean", "description": "true, with nothing else: stop your replay that is going instead." },
            }), &[]),
        },
        {
            "name": "start_autorun_discovery",
            "description": "Open the app's own Auto Run browser and sign in as a saved account, so you can explore the live application before you write a script. The app replays the recorded sign-in and types the password; you never see it. Answers the landing page as get_autorun_page shows it, and its address path. Refused while a run, a replay or a person's browser holds the Auto Run browser. It takes over a browser your own replay left open (its other tabs closed, not signing in again when it is signed in as the account you name) and goes home, and the answer says so. A failed sign-in closes the browser and says why: report it rather than trying again. Every locator you will put in a script must be seen on the live app first: a save refuses any locator the app has not seen. Never read the application's code to write scripts. For a case marked no_save, name it with `case_id` (or pass `no_save: true`): every save the page tries is blocked, not only reported, until the discovery ends. Call end_autorun_discovery when you are done.",
            "inputSchema": schema(serde_json::json!({
                "account": { "type": "string", "description": "The KEY of the saved account to sign in as, from get_accounts. Never a username or a password." },
                "area": { "type": "string", "description": "The recorded area you are exploring, by name; what you see is filed under it (with none, under no area)." },
                "browser": { "type": "string", "enum": ["edge", "chrome"], "description": "Optional; the one last used when left out." },
                "mapping": { "type": "boolean", "description": "Optional: true starts a menu mapping run of `modules` (used by /tcm:map-menus). It is read-only: every save the page tries is blocked and counted, and each answer carries `blocked`." },
                "modules": { "type": "array", "items": { "type": "string" }, "description": "With `mapping`: the modules to map, by name (at least one)." },
                "case_id": { "type": "number", "description": "Optional: the case you explore for. When its script is marked no_save, every save the page tries is BLOCKED until the discovery ends (named in `blocked_writes`)." },
                "page": { "type": "boolean", "description": "Optional: false leaves the landing page out of the answer (it is still recorded as seen; get_autorun_page reads it)." },
                "no_save": { "type": "boolean", "description": "Optional: true blocks every save the page tries until the discovery ends, as for a case marked no_save." },
            }), &["account"]),
        },
        {
            "name": "discover_autorun_actions",
            "description": "Carry out script actions in the discovery browser and see what they did: `action` for ONE or `actions` for SEVERAL (at most 20 actions), never both. One answers the page afterwards, an address change, a dialog or message that appeared, and the requests that wrote data (method and path only). Several run in order once every action is checked - if any would be refused, none runs and the answer names it; they stop at the first action that fails unless `stop_on_failure` is false, and answer one line per action that ran, then the page once. What each page showed is recorded as seen. Any action in the script vocabulary works (a navigate is held to the environment's allowed sites); sign_in and file addresses are refused. In a discovery that must not save (a no_save case named by `case_id`, or `no_save: true`) or a mapping run, a save is blocked: the action fails with the guard's sentence and `blocked_writes` names the request. A guard this call's own `case_id` or `no_save` sets lasts for this call only; one the discovery started with lasts until it ends. Anything you create uses the environment's test name prefix, and you delete only records whose name carries that prefix. Every write is logged. A use_component action runs a component, answering its actions as `steps`; `draft` tries one not saved yet, or a change to a saved one, before save_autorun_component. A `draft` that no use_component action names is refused. Needs start_autorun_discovery first.",
            "inputSchema": schema(serde_json::json!({
                "action": {
                    "type": "object",
                    "description": "One action in the script vocabulary, e.g. { \"kind\": \"click\", \"selector\": ... } - call get_guide (topic autorun) for all of them.",
                },
                "actions": {
                    "type": "array",
                    "description": "Instead of `action`: several, in order, at most 20.",
                    "items": { "type": "object" },
                },
                "stop_on_failure": { "type": "boolean", "description": "Optional, with `actions`: false carries on past a failed action. True when left out." },
                "page": { "type": "boolean", "description": "Optional: false leaves the page out of the answer (it is still recorded as seen; get_autorun_page reads it)." },
                "area": { "type": "string", "description": "Optional: the area this exploring belongs to, by name; what is seen from now on is filed under it." },
                "case_id": { "type": "number", "description": "Optional: the case this call is for. When its script is marked no_save, every save the page tries is BLOCKED for this call only (named in `blocked_writes`), so name it on every call for such a case." },
                "no_save": { "type": "boolean", "description": "Optional: true blocks every save the page tries for this call only, as for a case marked no_save." },
                "draft": {
                    "type": "object",
                    "description": "Optional: a component to try in place of the saved one of that name, for each use_component action that names it, exactly as you will send it to save_autorun_component: { name, description, inputs, actions }.",
                },
            }), &[]),
        },
        {
            "name": "save_autorun_area",
            "description": "Save a screen you found through the menus during discovery as an area, so runs can reach it before step 1. The app checks it first: from the home page, signed in, it replays your clicks and saves the area only if they arrive where the discovery browser is now. A refusal says what the page showed. A name already taken is refused; pick another or ask the person - except a repair: when a run cannot reach an existing area (its menu moved), find the screen through the menus and send that area's name, its module, the new clicks and `repair: { reason }`. The area's saved path is tried first, from a fresh home as a run takes it, and the repair is refused while it still arrives; the area keeps its name, module and scripts, only its clicks change, and the person can put the old path back. Set each script's `area` to this name. If you cannot find the screen through the menus, ask the person to record the area in Auto Run instead.",
            "inputSchema": schema(serde_json::json!({
                "name": { "type": "string", "description": "The new area's name, unique in this project; with `repair`, the existing area's name." },
                "module": { "type": "string", "description": "The test case Module the area belongs to; with `repair`, the area's own module (a repair never moves an area)." },
                "repair": {
                    "type": "object",
                    "description": "Optional: replace the menu path of an EXISTING area a run cannot reach (whoever made it, a person included) instead of saving a new one.",
                    "properties": {
                        "reason": { "type": "string", "description": "One line, at most 200 characters, in words with no address: what changed in the menu. The person sees it beside the area." },
                    },
                    "required": ["reason"],
                },
                "clicks": {
                    "type": "array",
                    "description": "In order, the locators clicked from the home page to reach the screen, each as a snapshot line's locator.",
                    "items": { "type": ["object", "string", "array"] },
                },
            }), &["name", "module", "clicks"]),
        },
        {
            "name": "end_autorun_discovery",
            "description": "End your discovery: what it mapped is kept, and its browser stays open and signed in for your next replay or discovery. It is closed only by the person's End discovery button, a release (reset_autorun_browser), or an end whose save guard will not lift (the answer then says its browser is closed). Call it when you have finished exploring. Safe to call when no discovery is going.",
            "inputSchema": schema(serde_json::json!({}), &[]),
        },
        {
            "name": "reset_autorun_browser",
            "description": "Start clean in the Auto Run browser your replay or discovery left open, without signing out: it closes the other tabs, forgets the dialogs and the stopping point of your last replay, lifts your save guard and loads the home page with the sign-in kept, ending your discovery if one is going (what it mapped is kept). If the sign-in has gone it says so, and the next replay signs in again. Refused while a replay is running (replay_autorun_to_step with `stop: true` stops one), and for the person's own browser. `release: true` lets go of the browser instead - only when it is broken (frozen, on the wrong account, its session gone), or the app refuses with 'close the supervised browser first' or answers \"the browser did not answer\" while no Auto Run browser is really open. Never release between replays: replays, discovery and resets share one browser and its sign-in. Never ask the person to restart the app. A release frees a held browser that has gone, ends your own discovery, and closes a browser your own replay left open, freeing its account; it never closes the person's own browser while it is open and answering (it says the person must close it or press Release Auto Run browser), and never stops a replay that is going.",
            "inputSchema": schema(serde_json::json!({
                "release": { "type": "boolean", "description": "true only for a broken browser: let it go instead of resetting it. Never between replays." },
            }), &[]),
        },
        {
            "name": "save_autorun_component",
            "description": "Save a component: a widget or short flow worked out once on the live app, that any script then runs with one use_component action. Make one when a widget or flow is needed more than once; reuse a saved one rather than repeat its actions. Try it first with discover_autorun_actions, a use_component of it with this component as its `draft`, and save it unchanged once that works; anything else is refused. Every fixed locator in it must have been seen: an action needs one seen matching exactly one element, while a check that can pass with several matches may use one seen matching several; every input it declares must be used, and every one it uses declared; it cannot sign in, type a username or password, use another component, or build an address from an input. Saving a name already saved is a change: it needs `why` and is tried live again first. `remove: true` and only the `name` removes a saved one instead, refused while a script uses it (the refusal names the cases).",
            "inputSchema": schema(serde_json::json!({
                "name": { "type": "string", "description": "Its name, unique in this project, e.g. \"Pick a date\"." },
                "description": { "type": "string", "description": "One line: what it does and when to use it." },
                "inputs": {
                    "type": "array",
                    "description": "What a script passes it. A text input is written {{name}} in its actions' strings; a target input stands where a locator goes, as {\"input\": \"name\"}, alone or as one link of a chain.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "kind": { "type": "string", "enum": ["text", "target"] },
                            "description": { "type": "string" },
                        },
                        "required": ["name", "kind", "description"],
                    },
                },
                "actions": {
                    "type": "array",
                    "description": "Its actions, in the script vocabulary - call get_guide (topic autorun) for them.",
                    "items": { "type": "object" },
                },
                "why": { "type": "string", "description": "Only when changing a saved component: one sentence on why it changes." },
                "dry_run": { "type": "boolean", "description": DRY_RUN_COMPONENT },
                "remove": { "type": "boolean", "description": "true: remove the saved component `name`; send nothing else." },
            }), &["name"]),
        },
        {
            "name": "get_autorun_failures",
            "description": "What failed in an Auto Run run on THIS machine, as text you can act on: which step, which action (its own JSON), what the page said, and the picture. It also says when you must not touch the script at all - a sign-in that failed, a browser that stopped answering, a case that could not reach its module screen, or a case the person marked Blocked are not script defects (a case that could not reach its area: repair the area with save_autorun_area). A newer replay of a case that stopped on a failure counts as its newest word (the text says \"from a replay to step N\"). Asked with neither case_id nor run_id, it names the run it read and ends with the other recent runs on this machine with failed or blocked cases, each with its run_id. Read this before repairing anything.",
            "inputSchema": schema(serde_json::json!({
                "case_id": { "type": "number", "description": "The newest run that holds this test case." },
                "run_id": { "type": "string", "description": "One run by its id instead. With neither, the newest run on this machine." },
            }), &[]),
        },
        {
            "name": "record_quirk",
            "description": "Record one line (`text`, at most 300 characters) about how this application behaves, so the next script or API template does not rediscover it: one list per project, read by the Auto Run and API template guides. A line already on the list is not written twice; one you retired comes back; one a person wrote or retired is refused - ask them. Name the cases and steps it explains in `cases` (each step must have failed in its case's newest run). A project keeps 60 active quirks: past that, this is refused with the best candidates to retire. To retire one of your own that no longer helps, send `retire` with its id and a `reason` instead of `text` (`replacement` records a better line in its place). A note a person wrote is refused - ask them to remove it. How quirks are counted and retired: get_guide (topic autorun), section \"Repairing a script that failed\".",
            "inputSchema": schema(serde_json::json!({
                "text": { "type": "string", "description": "To record: one line, e.g. \"the results grid paginates at 25 rows\"." },
                "cases": quirk_cases(),
                "from": { "type": "string", "enum": ["autorun", "api"], "description": "\"api\" when you learned it building API templates; \"autorun\" (the default) for Auto Run scripts." },
                "retire": { "type": "string", "description": "To retire: the quirk's id, as the Known quirks section shows it (e.g. q1a2b3c)." },
                "reason": { "type": "string", "description": "With retire: one sentence on why the note no longer helps." },
                "replacement": { "type": "string", "description": "With retire, optional: one line to record in its place." },
            }), &[]),
        },
        {
            "name": "record_validation_rule",
            "description": "Remember (or remove) one rule the application enforced, in an area: the words its page showed after an action it refused, e.g. \"End date must be after the start date\". Use it when you judge a message the page showed to be a validation rule that Auto Run did not file by itself, or to remove one that no longer holds (`remove`). `message` is the application's own words, as the page showed them; `after` is the action that caused it, in a few words (needed unless you remove). `area` defaults to your discovery's area and `page` (when adding) to the page the browser is on; a removal with no `page` is from every page of the area; `step` is the wizard step when the page shows one. The rule is kept against the area, attributed to the assistant, and the guide shows it to whoever writes the next script there.",
            "inputSchema": schema(serde_json::json!({
                "message": { "type": "string", "description": "The application's words, as the page showed them." },
                "after": { "type": "string", "description": "The action that caused it, in a few words, e.g. \"clicked Save with the end date before the start date\". Needed unless `remove` is set." },
                "area": { "type": "string", "description": "The area it holds in. Defaults to the open discovery's area; refused when there is neither." },
                "page": { "type": "string", "description": "The page it was met on (its path). Adding defaults to the page the browser is on; removing without it removes the words from every page of the area." },
                "step": { "type": "string", "description": "The page's wizard step, when it shows one." },
                "remove": { "type": "boolean", "description": "True to take the rule out instead of remembering it." },
            }), &["message"]),
        },
        {
            "name": "mark_autorun_suspected_defect",
            "description": "Use this instead of a repair when the application, not the script, is wrong: it marks the failed step of a case as a suspected application defect with a note on what the application did, leaves the script's actions and its repair count untouched, and is refused unless that step failed in the case's newest run (or in a replay of it that stopped there, when that is newer or no run has the case) for a reason other than a STOP line.",
            "inputSchema": schema(serde_json::json!({
                "case_id": { "type": "number", "description": "The test case whose script to mark." },
                "step_number": { "type": "number", "description": "The step that failed because of the application." },
                "note": { "type": "string", "description": "What the application did, against what the case expects; at most 300 characters." },
            }), &["case_id", "step_number", "note"]),
        },
        {
            "name": "set_autorun_order",
            "description": "Set the order Auto Run runs a PBI's test cases in on this machine, for Run selected and Run unattended. Put each case that needs shared state unchanged before the cases that change it, and keep cases that need the same settings together, so a run needs as few pauses as possible; the scripts' changes, needs_unchanged and needs_state marks say which those are. It does not change Run Tests' order. Every id must be one of the PBI's test cases: one that is not is refused with `case <id> is not in PBI <pbi>`, and nothing is saved. Each call REPLACES the PBI's whole saved order (`mode: \"replace\"`, the default): cases you leave out run after the listed ones, in list order. `mode: \"append\"` keeps the saved order's other cases, in their order, after the ones you list. The answer says which it did. A listed case with no saved script is saved in the order and named back: `case <id> has no saved script`.",
            "inputSchema": schema(serde_json::json!({
                "pbi_id": { "type": "number", "description": "The PBI whose cases these are." },
                "case_ids": {
                    "type": "array",
                    "items": { "type": "number" },
                    "description": "The PBI's test case ids, in the order Auto Run should run them, each once.",
                },
                "mode": { "type": "string", "enum": ["replace", "append"], "description": "Optional: \"replace\" (the default) saves exactly this order; \"append\" keeps the saved order's other cases after these." },
            }), &["pbi_id", "case_ids"]),
        },
        {
            "name": "propose_accounts",
            "description": "Propose test logins for the app's active environment - ones you found in a seed script, a spec or the database - so a person can add them as Auto Run accounts. Each entry is { key, label, username, role?, password? }. Include each account's password from the same database lookup that found its login, because every account's password can be different - but never a hash: a value that is clearly a hash or an encrypted blob is left out, and you say so. A proposal that carries any password is refused unless the environment is marked as a test environment. The person picks which to add in the app, and can type another password over a proposed one. Keys are lowercase letters, digits, dot, underscore or hyphen, e.g. hr.supervisor - the name a script or template uses. Each call REPLACES your previous proposal for this environment; at most 100 accounts.",
            "inputSchema": schema(serde_json::json!({
                "accounts": {
                    "type": "array",
                    "description": "The accounts to propose, at most 100.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "key": { "type": "string", "description": "What a script names the account by, e.g. hr.supervisor." },
                            "label": { "type": "string", "description": "What a person reads, e.g. HR supervisor." },
                            "username": { "type": "string", "description": "The login name." },
                            "role": { "type": "string", "description": "Optional: the role or permission set the login has." },
                            "password": { "type": "string", "minLength": 1, "maxLength": 256, "description": "Optional: the login's password from the same database lookup, never a hash. Only for an environment marked as a test environment." },
                        },
                        "required": ["key", "label", "username"],
                        "additionalProperties": false,
                    },
                },
            }), &["accounts"]),
        },
        {
            "name": "get_accounts",
            "description": "The app's active environment and its accounts: key, label and username - the keys are what a script's `account` or a template run names. Passwords are included only when a person has marked the environment as a test environment; otherwise a note says so. Never write a username or password into a script or template. Also the project's `test_files` (name and size), the only files a case's upload step can send; one that is missing is for the person to add in Auto Run, Setup, Test files - you cannot.",
            "inputSchema": schema(serde_json::json!({}), &[]),
        },
        {
            "name": "get_autorun_network_wait",
            "description": "Whether an unattended Auto Run is paused because the company network is unreachable (the environment's database server has stopped answering), which case it will run next, how many seconds it has waited and how many seconds remain before it gives up. Answers { waiting: false } when no run is paused. Read only: only the network returning, the person pressing Stop or the time limit ends the wait, and nothing you call can.",
            "inputSchema": schema(serde_json::json!({}), &[]),
        },
        {
            "name": "search_autorun_requests",
            "description": "The requests the page in the Auto Run browser made - its own XHR, fetch and page loads on the application's site, polls left out - newest first: id, method, path and query, status, type, sizes, start and duration; with no filter, all of them. Every filter given must match, in any case: `path` a part of the path and query, `method` and `type` whole, `status` a code (404) or a class (4xx), `request_contains` and `response_contains` a part of a body. Read only, and redacted: passwords, tokens and session values show as [redacted], and a secret's value never matches. 404 \"No browser is open.\" with no Auto Run browser, and 409 while a replay, a step or a discovery action is using it: ask again when it has finished. Nothing shows until TCM's sign-in has confirmed the application's site. With `from`, it reads a set loaded with autorun_har instead, browser or not. Paged, with `total`: while `next_offset` is there, call again with `offset` set to it.",
            "inputSchema": schema(serde_json::json!({
                "path": { "type": "string", "description": "A part of the path and query, e.g. /api/leave." },
                "method": { "type": "string", "description": "GET, POST, PUT, PATCH..." },
                "status": { "type": ["string", "number"], "description": "A status code such as 200, or a class such as 4xx." },
                "type": { "type": "string", "description": "XHR, Fetch or Document." },
                "request_contains": { "type": "string", "description": "A part of the request body." },
                "response_contains": { "type": "string", "description": "A part of the response body." },
                "offset": { "type": "number", "description": "How many to skip: the `next_offset` of the previous answer." },
                "limit": { "type": "number", "description": "How many one answer carries: 25 by default, 100 at most." },
                "from": { "type": "string", "description": "Optional: the name of a set loaded with autorun_har. Left out, the live capture." },
            }), &[]),
        },
        {
            "name": "get_autorun_request",
            "description": "One captured exchange by its id, redacted. `part` \"request\" (the default): method, path and query, type, tab, start, its headers (an allow-list: content-type, accept, x- headers and the like - never cookies or authorization) and its body. \"response\": status, mime, headers (allow-listed, values redacted), size, start and duration, a failure, its first redirect, and whether its body was kept and how long it is. \"body\": the response body as text; only text bodies (JSON, text, HTML, XML, forms) are kept, at most 2 MB each, and one that is not there says why. A body longer than 20 000 characters comes in pages: `chars` is its whole length, and while `next_offset` is there, call again with `offset` set to it. Same browser and `from` rules as search_autorun_requests (404 with no browser, 409 while it is busy).",
            "inputSchema": schema(serde_json::json!({
                "id": { "type": "number", "description": "The request's id, from search_autorun_requests." },
                "part": { "type": "string", "enum": ["request", "response", "body"], "description": "Optional: request (the default), response, or body (the response's)." },
                "offset": { "type": "number", "description": "Optional, for a request or a body: where the page starts, in characters." },
                "from": { "type": "string", "description": "Optional: an imported set's name. Left out, the live capture." },
            }), &["id"]),
        },
        {
            "name": "autorun_capture",
            "description": "The Auto Run browser's capture filter: with no arguments, read it (`include_types`, `exclude_types`, `path_contains`), how many requests it keeps now (1000 at most, the oldest dropped first), whether the application's site is confirmed yet, and the names of the sets loaded with autorun_har. With any filter field, set what the capture keeps FROM NOW ON: only these resource types, not these, only paths containing `path_contains`. A set replaces the whole filter (send `include_types: []` alone to keep every request again); what was left out before cannot be brought back, and what is kept stays. Works in the person's browser too; 404 with no browser, 409 while a replay, a step or a discovery action is using it.",
            "inputSchema": schema(serde_json::json!({
                "include_types": { "type": "array", "items": { "type": "string", "enum": ["XHR", "Fetch", "Document"] }, "description": "Keep only these types. Empty: every type." },
                "exclude_types": { "type": "array", "items": { "type": "string", "enum": ["XHR", "Fetch", "Document"] }, "description": "Never keep these types." },
                "path_contains": { "type": "string", "description": "Keep only requests whose path and query contain this, in any case; at most 200 characters." },
            }), &[]),
        },
        {
            "name": "autorun_har",
            "description": "HAR 1.2 files in TCM's own captures folder (`<name>.har` - never anywhere else). `export`: save the live capture, redacted exactly as the reads show it, under that name (a file of that name is replaced), answering how many requests it holds; 404 with no browser, 409 while a replay, a step or a discovery action is using it. `import`: load that file (an export of yours, or a HAR file the person saved there from a browser) as a separate read-only set, redacted again on the way in, read with `from: \"<name>\"` on search_autorun_requests and get_autorun_request; it is never replayed into the browser, needs no browser, and is forgotten when the person signs in or out of TCM.",
            "inputSchema": schema(serde_json::json!({
                "export": { "type": "string", "description": "The name to save as, without .har: letters, digits, spaces, dots, dashes or underscores, at most 80 characters, no folder." },
                "import": { "type": "string", "description": "Instead: the name of a file in the captures folder, without .har." },
            }), &[]),
        },
        {
            "name": "set_autorun_headers",
            "description": "Send extra request headers with every request of the Auto Run browser you opened (your discovery or replay) - every tab, and any tab the page opens - to test an edge case such as a feature header or another language. Merged into the headers already set; `null` removes one you set. Never cookie, authorization, proxy-authorization or host, nor one that routes a request to another method or path (X-HTTP-Method-Override, X-HTTP-Method, X-Method-Override, X-Original-URL, X-Rewrite-URL). The save guard is not touched: a blocked save stays blocked. They stay until you remove them, or reset_autorun_browser, autorun_cookies' clear or a release takes them away, and every try, discovery and replay answer ends naming them (\"Changed for testing: header ...\"). Refused in the person's browser and while a replay runs.",
            "inputSchema": schema(serde_json::json!({
                "headers": { "type": "object", "additionalProperties": { "type": ["string", "null"] }, "description": "Header name to value, or to null to remove a header you set." },
            }), &["headers"]),
        },
        {
            "name": "autorun_cookies",
            "description": "The Auto Run browser's cookies on the app's own site - the one TCM's sign-in confirmed, whatever page shows (409 before it is). `action` is needed, with no default. \"list\": every cookie whatever its path - name, domain, path, expiry, flags (http_only, secure, same_site), size and value - with the sign-in's cookies (HttpOnly, the sign-in and anti-forgery cookies, names like a secret) showing their value as (hidden) and `set_for_testing` marking yours; a read, so it works in the person's browser too. \"set\": one cookie to test an edge case (culture, tenant, a feature flag), never the sign-in's: an HttpOnly one, .ASPXAUTH, .AspNetCore.*, ASP.NET_SessionId, __RequestVerificationToken*, or a name like a secret (session, auth, token, password, secret, cookie) is refused, and it never makes TCM think the sign-in changed; it stays until reset_autorun_browser (which deletes only the cookies you set), a clear or a release, and every try, discovery and replay answer ends naming it. \"clear\", with nothing else: sign out for real - delete every cookie of the app's confirmed site, including the ones set for its parent domains (the sign-in's included; an identity provider on a sibling host that shares a parent-domain cookie is signed out too), forget the signed-in account and let its lease go, and take away your test headers; the next replay signs in again, and a discovery going keeps going, signed out. Each action takes only its own fields. list answers 409 while a replay runs or a step or a discovery action is using the browser; set and clear are refused in the person's browser and while a replay runs.",
            "inputSchema": schema(serde_json::json!({
                "action": { "type": "string", "enum": ["list", "set", "clear"], "description": "list, set or clear. Needed." },
                "url": { "type": "string", "description": "list, optional: an http or https address on the app's site or one of its parent domains (anything else is refused). A parent domain lists only the cookies set for it or above it." },
                "name": { "type": "string", "description": "set: the cookie's name." },
                "value": { "type": "string", "description": "set: its value, one line, no ;." },
                "path": { "type": "string", "description": "set, optional: its path, / by default." },
                "expires": { "type": "number", "description": "set, optional: when it expires, in seconds since 1970. Left out, it lasts for the browser's session." },
                "secure": { "type": "boolean", "description": "set, optional: sent only over https." },
                "same_site": { "type": "string", "enum": ["Strict", "Lax", "None"], "description": "set, optional: its SameSite rule (None needs secure)." },
            }), &["action"]),
        },
        {
            "name": "list_api_templates",
            "description": "The API templates saved for the current project, filtered and paged. Check here before building a template - the one you need may already exist; it is also the reference for real endpoints, paths and response fields when a script checks the API (never run from a script). When at most `limit` match, each comes in full (params, outputs, flow stage, whether it is proven on this site - prove an imported one before relying on it - and its newest run) with the project's flows and Test files; when more match, a compact index: narrow it with `module`, `search` or `flow`, or pass `id` for one template in full. With `id`, the other filters and the paging are ignored. While `paging.next_offset` is there, call again with `offset` set to it. With `kind: \"fixtures\"` it lists the saved fixtures instead: `id` for one in full, `full: true` for all. What each field means: get_guide (topic api_templates). Read-only; answers with the API templates switch off.",
            "inputSchema": schema(serde_json::json!({
                "kind": { "type": "string", "enum": ["templates", "fixtures"], "description": "\"templates\" (the default) or \"fixtures\"." },
                "module": { "type": "string", "description": "Only templates whose module contains this text (case does not matter)." },
                "search": { "type": "string", "description": "Only templates whose id, title, or a param or output name contains this text (case does not matter)." },
                "flow": { "type": ["string", "boolean"], "description": "A flow id: only that flow and its templates. Or false: leave the flows and the Test files out of an answer in full." },
                "id": { "type": "string", "description": "One template's id, in full with its flow (or one fixture's, with kind fixtures)." },
                "offset": { "type": "number", "description": "How many matching templates to skip: the `next_offset` of the previous answer. 0 by default." },
                "limit": { "type": "number", "description": "How many templates one answer carries: 25 by default, 100 at most." },
                "full": { "type": "boolean", "description": "With kind fixtures: true answers every fixture in full. Long; prefer `id`." },
            }), &[]),
        },
        {
            "name": "prove_api_template",
            "description": "Run a draft API template in a fresh signed-in browser and save it only if every step passed. Every problem with the draft, the values or the account comes back together before anything runs. The first failing step stops the run and nothing is rolled back: the answer says which step failed, with the start of the response, and what had already been created. Proving an id that is already saved replaces it only with replace: true and a why. Refused while the API templates switch on the AI Bridge tab is off. Call get_guide (topic api_templates) first for the format.",
            "inputSchema": schema(serde_json::json!({
                "template": { "type": "object", "description": "The draft, in the format get_guide (topic api_templates) describes - without `proven`, which the app writes." },
                "account": { "type": "string", "description": "The KEY of the account to sign in as - one the guide lists, never a username or password." },
                "values": { "type": "object", "description": "One value per param, by name, of its declared type." },
                "replace": { "type": "boolean", "description": "Only when a template with this id is already saved: true to replace it." },
                "why": { "type": "string", "description": "With replace: why the saved template is being changed. Logged." },
                "browser": { "type": "string", "description": "\"edge\" (the default) or \"chrome\"." },
            }), &["template", "account"]),
        },
        {
            "name": "run_api_template",
            "description": "Run a saved API template and return its outputs - or, when a step fails, which step failed, the start of the response, and what had already been created (nothing is rolled back). Every run is added to the template's history on the API Templates tab. Refused while the API templates switch on the AI Bridge tab is off. While your discovery holds the account in the Auto Run browser the run is refused, and the answer says so: end the discovery first, or run as another account.",
            "inputSchema": schema(serde_json::json!({
                "id": { "type": "string", "description": "The template's id, from list_api_templates." },
                "account": { "type": "string", "description": "The KEY of the account to sign in as - one the guide lists, never a username or password." },
                "values": { "type": "object", "description": "One value per param, by name, of its declared type." },
                "browser": { "type": "string", "description": "\"edge\" (the default) or \"chrome\"." },
            }), &["id", "account"]),
        },
        {
            "name": "save_api_flow",
            "description": "Save a flow - a module's wizard mapped as stages, each with a database check that says whether that stage is done for one record - after checking it and running every stage's check once on a sample record. Map the WHOLE wizard as one large flow per kind of record, and place every template you have discovered or built for that record on a stage: call list_api_templates first, and add stages no template performs yet so the gaps show. The answer says each stage's result for the sample, and lists `not_on_a_flow` - saved templates no flow places yet; place each one on a stage (re-proving it with its stage set) until that list is empty. Needs a database chosen on the AI Bridge tab; does not need the API templates switch. Saving an id that is already saved replaces it only with replace: true and a why. Call get_guide (topic api_templates) first for the format.",
            "inputSchema": schema(serde_json::json!({
                "flow": { "type": "object", "description": "The flow, in the format get_guide (topic api_templates) describes - without `saved`, which the app writes." },
                "sample": { "description": "The subject of a real record, found with db_query - every stage's check is run on it once.", "type": ["string", "number"] },
                "replace": { "type": "boolean", "description": "Only when a flow with this id is already saved: true to replace it." },
                "why": { "type": "string", "description": "With replace: why the saved flow is being changed. Logged." },
            }), &["flow", "sample"]),
        },
        {
            "name": "get_api_flow_progress",
            "description": "Which stages of a flow are done for this record and which come next. Call it before every run of a template that belongs to a flow, and run the template of a stage marked `next`. Needs a database chosen on the AI Bridge tab; does not need the API templates switch.",
            "inputSchema": schema(serde_json::json!({
                "flow": { "type": "string", "description": "The flow's id, from list_api_templates." },
                "subject": { "description": "The record's identifier - the flow's subject, e.g. the cycle id.", "type": ["string", "number"] },
            }), &["flow", "subject"]),
        },
        {
            "name": "save_api_fixture",
            "description": "Save a fixture: an ordered list of proven API templates that makes a draft the same way every time, so a test can have a fresh one in seconds. Each step names a template and its params as text, which may hold placeholders ({{steps.<n>.<output>}}, {{now:yyyyMMdd}}, {{prefix}}); `outputs` names what a script reads, and `creates` lists what the fixture makes, recorded as test-made so Clean up can remove it - a name there must start with {{prefix}}. Every problem comes back together, one sentence each, and nothing is saved: a template that is not proven, one that deletes, a placeholder no earlier step gives, or creates without {{prefix}} in a step's params. Refused while the API templates switch on the AI Bridge tab is off. Call get_guide (topic api_templates) first: it has the placeholders and the outputs format.",
            "inputSchema": schema(serde_json::json!({
                "fixture": { "type": "object", "description": "{ id, name, account, steps: [{ template, params }], outputs?, creates?, skip_stages? }, as get_guide (topic api_templates) describes. skip_stages names flow stages (by title, by id, or as <flow id>/<stage id> when a title is in two flows) that the record this fixture makes does not include, such as [\"Competencies\"] for a cycle with Competencies off: the flow gate does not ask an earlier step to perform them, and the answer echoes them as skipped_stages." },
            }), &["fixture"]),
        },
        {
            "name": "run_api_fixture",
            "description": "Run a saved fixture - its templates in order, in one browser signed in once as the fixture's account - and return its sentence, its outputs, what it made and any warnings. The run stops at the first failed step, with that step's own sentence. Whatever it made, even when a later step failed, is recorded as test-made. A successful run's outputs become the fixture's current outputs; a failed one keeps the ones before. Use it to Rebuild a damaged shared draft, never a hand fix. Refused while the API templates switch on the AI Bridge tab is off. While your discovery holds the fixture's account in the Auto Run browser the run is refused, and the answer says so: end the discovery first.",
            "inputSchema": schema(serde_json::json!({
                "id": { "type": "string", "description": "The fixture's id, from list_api_templates (kind fixtures)." },
                "browser": { "type": "string", "description": "\"edge\" (the default) or \"chrome\"." },
            }), &["id"]),
        },
        {
            "name": "db_lookup",
            "description": "Find the tables and columns behind a topic in the company database: table and column names, types, foreign keys, ranked by how well they match the words. Use it before writing a query, and to check which table a screen reads from. A bare table name (`dbo.LeaveRequest`, or `LeaveRequest`) comes back as that table's whole column list instead of the ranked match.",
            "inputSchema": schema(serde_json::json!({
                "query": { "type": "string", "description": "The words behind the tables you are looking for, e.g. \"leave request\" - or one table's name for its full column list" },
                "limit": { "type": "number", "description": "How many tables to return (default 10, cap 30)" },
            }), &["query"]),
        },
        {
            "name": "db_query",
            "description": "Run one SQL statement on the chosen company database through sqlcmd and read the result (200 rows at most). SELECT runs on every connection, and so does EXEC of a look-up system procedure (sp_help, sp_helptext, sp_helpindex, sp_columns, sp_tables, sp_stored_procedures, sp_pkeys, sp_fkeys). INSERT, UPDATE, DELETE, and EXEC of a named stored procedure run only when the person has switched writes on in the AI Bridge tab and the connection is the Dev - dev login one. Never dynamic SQL (EXEC('...'), EXEC(@variable), sp_executesql), never any other sp_ or xp_ procedure, and never DROP. CREATE, ALTER and CREATE OR ALTER are refused too, except on the Dev - dev login with Crystal Reports and Database Access on and writes switched on: there it may create or alter one procedure, view or function per call (still never DROP). A request over 64 KiB is refused: split a longer procedure or run it by hand. To change more than one thing, send `statements` instead of `sql`: they run as ONE transaction the app wraps for you - all saved or none. Give each write an `expect_rows`: the batch stops and rolls back the moment a statement affects a different number of rows. Run it with dry_run: true first - everything runs, each statement's row count comes back, and it is all rolled back - then send the same statements without dry_run to save them. Never write BEGIN, COMMIT, ROLLBACK, DECLARE or THROW yourself: every statement is checked on its own, exactly like `sql`, and those are refused.",
            "inputSchema": schema(serde_json::json!({
                "sql": { "type": "string", "description": "ONE statement, with no GO separator and no second statement after a semicolon. Send this or `statements`, not both." },
                "statements": {
                    "type": "array",
                    "description": "Up to 50 statements run in order as one all-or-nothing transaction. Each is held to the same rules as `sql`.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "sql": { "type": "string", "description": "One statement" },
                            "expect_rows": { "type": "integer", "description": "The rows it must affect (or return); any other count stops the batch and rolls it all back" },
                        },
                        "required": ["sql"],
                    },
                },
                "dry_run": { "type": "boolean", "description": "Run everything, report each statement's row count, then roll it all back so nothing is saved" },
            }), &[]),
        },
        {
            "name": "open_report",
            "description": "Open a Crystal Reports .rpt file and read it: you get the report as an IR (format crpt-ir/1), a JSON tree of its areas, sections and objects, each with an id and its position, size, font and text. Take ids from the latest IR a report tool returned: an edit can change them. One report is open at a time; opening another replaces it, and every other report tool works on the open one. Read get_guide (topic reports) before your first edit for the units, the edit ops, the layout plan and the limits. The path must be the full path to a .rpt file on this machine. With `create`, a blank report is made at a path that has no file yet and opened - never over an existing file: choose another path, or open that one.",
            "inputSchema": schema(serde_json::json!({
                "path": { "type": "string", "description": "The full path to the .rpt file" },
                "create": {
                    "type": ["object", "boolean"],
                    "description": "Make a blank report instead: true, or its summary information as { title, author, comments }.",
                    "properties": {
                        "title": { "type": "string" },
                        "author": { "type": "string" },
                        "comments": { "type": "string" },
                    },
                },
            }), &["path"]),
        },
        {
            "name": "read_report",
            "description": "Read the open report again, or only part of it: one section by name, or one object by its IR id or designer name. The ids in the answer are the ones edit_report takes. Narrowing saves reading a large report whole; with no arguments you get all of it. Open a report first with open_report; get_guide (topic reports) describes the IR.",
            "inputSchema": schema(serde_json::json!({
                "section": { "type": "string", "description": "A section's name, e.g. \"Details\": only that section and its objects" },
                "object": { "type": "string", "description": "An object's IR id or designer name: only that object" },
            }), &[]),
        },
        {
            "name": "edit_report",
            "description": "Change the open report with one batch of edits: either `ops` (the native edit ops) or `plan` (a layout plan, which is lowered to ops), never both. Every batch the report accepts is saved over the file at once, before this answers, and the answer is the new IR - git is the undo, so work on a file that is committed. A batch is all or nothing: if any edit is refused nothing is saved, and the answer is the refusal followed by the report as it now stands. Take ids and designer names from the latest IR you were given (this call's answer, open_report or read_report), since an edit can move or renumber objects. The ops, the plan's vocabulary, the units (twips) and what cannot be edited are in get_guide (topic reports): read it before the first edit. Check the result with check_report and preview_report.",
            "inputSchema": schema(serde_json::json!({
                "ops": { "type": "array", "description": "Native edit ops, applied in order as one batch. See get_guide (topic reports) for each op and its fields.", "items": { "type": "object" } },
                "plan": { "type": "object", "description": "A layout plan ({ \"planVersion\": 1, \"operations\": [...] }), the other way to describe a batch. See get_guide (topic reports)." },
            }), &[]),
        },
        {
            "name": "compare_reports",
            "description": "Compare two .rpt files on this machine and list every difference, one line each as `path: a -> b`; an empty list means they match. Use it to see exactly what an edit changed (compare the committed copy with the edited one) or how two versions of a report differ. Neither file needs to be the open report and neither is changed.",
            "inputSchema": schema(serde_json::json!({
                "a": { "type": "string", "description": "The full path to the first .rpt file" },
                "b": { "type": "string", "description": "The full path to the second .rpt file" },
            }), &["a", "b"]),
        },
        {
            "name": "check_report",
            "description": "Run the offline load checks on a report: the problems the Crystal Reports engine would refuse it for (errors) and the ones it would only complain about (warnings). With a path it checks that file; with none, the open report. Run it after editing, before the report goes anywhere.",
            "inputSchema": schema(serde_json::json!({
                "path": { "type": "string", "description": "The full path to a .rpt file; leave out to check the open report" },
            }), &[]),
        },
        {
            "name": "preview_report",
            "description": "Draw the open report to a PDF from the data saved in it, and say how many pages that is and what the renderer noted (an object it does not draw, a formula that failed). The person sees the same PDF on the Crystal Reports screen. A page count of 0 means the drawing failed; the notes say why.",
            "inputSchema": schema(serde_json::json!({}), &[]),
        },
        {
            "name": "preview_report_live",
            "description": "Draw the open report with fresh rows instead of the data saved in it: run the report's stored procedure on the database chosen in the AI Bridge tab and render what it returns. Running a procedure counts as a database write, so this works only with Database Access on, writes switched on in the AI Bridge tab and the Dev - dev login connection chosen; anything else is refused with what to change. Only a stored procedure of the connection's own database runs: a report that names another database or a linked server, an sp_ or xp_ procedure, or a SQL command is refused before anything connects. Give the report's parameters in `params`, by name (with or without the @). Only the values passed in `params` are used, never the values saved in the report, so pass every parameter you want set. A name the report does not have is refused, and so is leaving out a required one (not optional and not allowed empty), even if the report has a value saved for it; any other parameter left out is passed as NULL. It answers as preview_report does, with the connection's name; preview_report draws the saved data and needs no database.",
            "inputSchema": schema(serde_json::json!({
                "params": { "type": "object", "description": "The report's parameters, name to value, e.g. { \"performance_cycle_id\": \"42\" }. Dates as YYYY-MM-DD, times as HH:MM:SS, true/false for yes-or-no parameters." },
            }), &[]),
        },
        {
            "name": "optimize_cases",
            "description": "Reorganise a draft into a run sheet a tester can work straight through: navigation spelled out as explicit steps, expected results reduced to the outcome, and cases ordered so the tester changes environment as few times as possible. Run with dry_run: true FIRST and read expected_rewritten - every expected result this would shorten, before and after. Every case comes back stamped with BOTH spec_order and tester_order: keep those fields as returned. Call it once on your finished draft instead of hand-tuning it. For large drafts pass `path` with `in_place: true` - NEVER shard a draft to fit it inline: tester_order is one sequence across the whole set. Only .md files and Azure DevOps wiki links stay in `specs`: any other entry is dropped and named in import_warnings. Each option, and when to turn reorder or trim_expected off: get_guide (topic writing), Workflow step 3.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object). Use `path` instead for large drafts - never both." },
                "path": { "type": "string", "description": "Absolute path to the draft file - use this instead of `json` for large drafts." },
                "in_place": { "type": "boolean", "description": "With `path`: write the result back to that file (atomic) and answer only the report." },
                "entry": { "type": "string", "description": "First step of every preamble (default \"Launch the application.\"); a non-launch entry goes after the sign-in step." },
                "dry_run": { "type": "boolean", "description": "Answer only the report of what would change." },
                "reorder": { "type": "boolean", "description": "Default true; false keeps the document order (navigation and expected results are still cleaned up)." },
                "trim_expected": { "type": "boolean", "description": "Default true; false leaves every expected result exactly as written." },
            }), &[]),
        },
        {
            "name": "transform_cases",
            "description": "Apply bulk edits to a draft without rewriting it yourself: retag, retitle, set module or automation status, find/replace inside steps, split a step in two, sort, dedupe, insert at a position. Pass `path` instead of `json` for large drafts, and `in_place: true` to write the result back to that file. Only .md files and Azure DevOps wiki links stay in `specs`: any other entry is dropped and named in import_warnings. Read the report's `applied`, `warnings` and `ignored` lists - all three.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object). Use `path` instead for large drafts - never both." },
                "path": { "type": "string", "description": "Absolute path to the draft file - use this instead of `json` for large drafts." },
                "in_place": { "type": "boolean", "description": "With `path`: write the result back to that file (atomic)." },
                "operations": {
                    "type": "array",
                    "description": transform_ops_desc,
                    "items": { "type": "object" },
                },
            }), &["operations"]),
        },
        {
            "name": "validate_cases",
            "description": "Validate draft import JSON with Test Case Manager's REAL importer. Returns case count, warnings, and errors - fix every warning before finishing. It may also return `advisories`: judgement calls (such as a case that looks like it merges a positive and its negative). Do not treat those as defects - read each one, decide, and tell the developer what you decided. For large drafts pass `path` (a local file) instead of inlining the JSON; never skip validation because the draft is too big to inline.",
            "inputSchema": schema(serde_json::json!({
                "json": { "type": "string", "description": "The draft import JSON (array or wrapper object)" },
                "path": { "type": "string", "description": "Absolute path to a local draft file - use this instead of `json` for large drafts" },
            }), &[]),
        },
        {
            "name": "get_tags",
            "description": "The tag names this project already uses. Prefer an existing tag over inventing a near-duplicate. Served from the app's cache - calling this costs no Azure DevOps request.",
            "inputSchema": schema(serde_json::json!({
                "query": { "type": "string", "description": "Case-insensitive substring filter - a project can carry thousands of tags, so filter rather than fetching all of them" },
            }), &[]),
        },
        {
            "name": "search_pbis",
            "description": "Search the current project's PBIs by title text to find the right work item id.",
            "inputSchema": schema(serde_json::json!({
                "query": { "type": "string" },
            }), &["query"]),
        },
        {
            "name": "search_wiki",
            "description": "Search the project's Azure DevOps wiki for documentation; returns page paths and snippet highlights.",
            "inputSchema": schema(serde_json::json!({
                "query": { "type": "string" },
            }), &["query"]),
        },
        {
            "name": "get_wiki_page",
            "description": "Fetch a wiki page's full markdown content. `path` takes any of three forms: the page URL from the browser (which names its own wiki, so wiki_id can be omitted), a search_wiki hit's path exactly as returned, or a real page path.",
            "inputSchema": schema(serde_json::json!({
                "wiki_id": { "type": "string", "description": "From search_wiki. Not needed when path is a URL." },
                "path": { "type": "string", "description": "A wiki page URL, a search_wiki hit's path, or a page path." },
            }), &["path"]),
        },
    ]});
    let tools: Vec<serde_json::Value> = all["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| {
            let name = t["name"].as_str().unwrap_or_default();
            !disabled.iter().any(|d| d == name)
        })
        .map(|mut t| {
            // The live report preview runs a procedure, so the same switch covers
            // it - with its own sentence, as it has no batch to dry-run.
            let tail = match t["name"].as_str() {
                Some("db_query") => Some(DB_NO_ASK),
                Some("preview_report_live") => Some(PREVIEW_LIVE_NO_ASK),
                _ => None,
            };
            if let Some(tail) = tail.filter(|_| db_no_ask) {
                let said = format!("{}{tail}", t["description"].as_str().unwrap_or_default());
                t["description"] = serde_json::Value::String(said);
            }
            t
        })
        .collect();
    serde_json::json!({ "tools": tools })
}

/// The call-time refusal text for a switched-off tool. A tool whose module
/// is off has no switch on the AI Bridge tab to turn back on - it names
/// the module instead; every other switched-off tool (including a module's
/// tool whose module IS on and that the person switched off) gets the
/// ordinary AI Bridge sentence - a network tool's names the switch it also
/// needs. `on` is explicit so every branch is testable.
pub fn refusal_text(name: &str, on: &BTreeSet<Module>) -> String {
    if let Some(m) = crate::ai_tools::module_of_tool(name).filter(|m| !on.contains(m)) {
        format!("The `{name}` tool is not available: {}.", m.off_sentence())
    } else if crate::ai_tools::NETWORK_TOOLS.contains(&name) {
        format!(
            "The `{name}` tool is switched off in Test Case Manager. It needs the Network tools switch \
             on the app's AI Bridge tab (each environment keeps its own, off until the person turns it on), \
             and the Auto Run scripts tools switched on."
        )
    } else {
        format!(
            "The `{name}` tool is switched off in Test Case Manager. \
             Turn it back on in the app's AI Bridge tab if you need it."
        )
    }
}

/// The topics `get_guide` answers, in the order its description gives them.
pub const GUIDE_TOPICS: [&str; 4] = ["writing", "autorun", "api_templates", "reports"];

/// The module a guide topic belongs to - the module its own guide tool had
/// before the four became `get_guide` - or None for Core's writing guide.
/// `Err` for a topic there is no guide for.
pub fn guide_topic_module(topic: &str) -> Result<Option<Module>, ()> {
    match topic {
        "writing" => Ok(None),
        "autorun" => Ok(Some(Module::AutoRun)),
        "api_templates" => Ok(Some(Module::ApiTemplates)),
        "reports" => Ok(Some(Module::CrystalReports)),
        _ => Err(()),
    }
}

/// The tools whose switch decides a topic's guide: the row its old guide
/// tool rode in, less the tools another switch decides (the quirk tool,
/// which two rows share, and the network tools). Empty for Core's writing
/// guide, which nothing switches off.
fn guide_row(topic: &str) -> Vec<&'static str> {
    use crate::ai_tools::{API_TEMPLATE_TOOLS, AUTORUN_TOOLS, CRYSTAL_TOOLS, NETWORK_TOOLS};
    let own = |tools: &'static [&'static str]| -> Vec<&'static str> {
        tools.iter().copied().filter(|t| *t != "record_quirk" && !NETWORK_TOOLS.contains(t)).collect()
    };
    match topic {
        "autorun" => own(AUTORUN_TOOLS),
        "api_templates" => own(API_TEMPLATE_TOOLS),
        "reports" => own(CRYSTAL_TOOLS),
        _ => Vec::new(),
    }
}

/// Whether `get_guide` offers `topic` while `off` is the disabled set as
/// applied (`tool_policy_from`): the writing guide always; another only
/// while its row's own tools are not all off - which a module that is off
/// makes them. Exactly when the topic's old guide tool was offered.
pub fn guide_offered(topic: &str, off: &[String]) -> bool {
    match topic {
        "writing" => true,
        _ => guide_row(topic).iter().any(|t| !off.iter().any(|o| o == t)),
    }
}

/// The topics `get_guide` offers while `off` is disabled, in order.
pub fn offered_topics(off: &[String]) -> Vec<&'static str> {
    GUIDE_TOPICS.into_iter().filter(|t| guide_offered(t, off)).collect()
}

/// What `get_guide`'s description says about each topic.
fn guide_sentence(topic: &str) -> &'static str {
    match topic {
        "writing" => "\"writing\": Test Case Manager import JSON - format rules, the org's allowed Module values, the workflow and the transform ops; call it first.",
        "autorun" => "\"autorun\": writing an Auto Run action script - the actions and checks, and which source decides what: locators come from the live app, every assertion from the case's own expected result, never from what the code does; read it before writing a script. Alone it answers the Quick rules, this project's live sections and every section title; `section` adds one section, `quick` only the Quick rules and live sections, `full` the whole guide.",
        "api_templates" => "\"api_templates\": building an API template - the format, placeholders and captures, the workflow, this project's account keys and sign-in origin; read it before your first prove_api_template.",
        "reports" => "\"reports\": working with Crystal Reports - the IR, units (twips), edit ops, the layout plan and the limits; read it before your first edit_report.",
        _ => "",
    }
}

/// `get_guide` as `tools/list` offers it: its `topic` enum and its
/// sentences name only the topics offered, and the autorun guide's own
/// arguments come only with that topic - a guide that is not there is not
/// described, as its old tool was not listed.
fn guide_tool(off: &[String]) -> serde_json::Value {
    let topics = offered_topics(off);
    let mut description = String::from("The live guide for one job, by `topic`.");
    for t in &topics {
        description.push(' ');
        description.push_str(guide_sentence(t));
    }
    let mut props = serde_json::json!({
        "topic": { "type": "string", "enum": topics, "description": "Which guide." },
    });
    if topics.contains(&"autorun") {
        props["quick"] = serde_json::json!({ "type": "boolean", "description": "autorun only. true: only the Quick rules and the live sections - the active environment, whether get_accounts gives passwords, this project's areas, areas to explore, components and quirks." });
        props["section"] = serde_json::json!({ "type": "string", "description": "autorun only. One section's title, e.g. \"Selectors\" or \"Saving it\" (case ignored): the Quick rules and that section. An unknown title answers the list of titles." });
        props["full"] = serde_json::json!({ "type": "boolean", "description": "autorun only. true: every section - over 100,000 characters; prefer `section`." });
    }
    serde_json::json!({ "name": "get_guide", "description": description, "inputSchema": schema(props, &["topic"]) })
}

/// Why `get_guide` cannot answer `topic`, or None when it can. No topic or
/// an unknown one names the topics offered. A topic not offered is refused
/// in the words its old guide tool was (`refusal_text`): its module off
/// names the module; its row switched off says so.
pub fn guide_refusal(topic: Option<&str>, off: &[String], on: &BTreeSet<Module>) -> Option<String> {
    let topics = offered_topics(off).iter().map(|t| format!("\"{t}\"")).collect::<Vec<_>>().join(", ");
    let Some(topic) = topic.map(str::trim).filter(|t| !t.is_empty()) else {
        return Some(format!("get_guide needs a `topic`: one of {topics}."));
    };
    match guide_topic_module(topic) {
        Err(()) => Some(format!("There is no `{topic}` guide: `topic` is one of {topics}.")),
        Ok(Some(m)) if !on.contains(&m) => Some(format!("The `{topic}` guide is not available: {}.", m.off_sentence())),
        Ok(_) if !guide_offered(topic, off) => Some(format!(
            "The `{topic}` guide is switched off in Test Case Manager. \
             Turn it back on in the app's AI Bridge tab if you need it."
        )),
        Ok(_) => None,
    }
}

/// Said when `section`, `quick` or `full` comes with a topic other than
/// autorun: only that guide is long enough to be read in parts.
pub const GUIDE_PARTS_AUTORUN_ONLY: &str = "`section`, `quick` and `full` are for topic \"autorun\" only - send the other topics with `topic` alone";

/// Whether an optional argument says anything: absent, null, false and ""
/// all say nothing.
fn says(v: &serde_json::Value) -> bool {
    !matches!(v, serde_json::Value::Null | serde_json::Value::Bool(false)) && v.as_str() != Some("")
}

/// Why a `{ scripts, edits }` bundle sent as `scripts` cannot take the
/// `edits` sent beside it: its own `edits` is neither a list nor one entry.
pub const BUNDLE_EDITS_NOT_A_LIST: &str = "the bundle sent as \"scripts\" has an \"edits\" that is not a list, and more \"edits\" were sent beside it - send one \"edits\" list beside \"scripts\", one entry per case";

/// Why the `edits` sent beside a `{ scripts, edits }` bundle cannot be
/// added to the bundle's own: it is neither a list nor one entry.
pub const BESIDE_EDITS_NOT_A_LIST: &str = "the \"edits\" sent beside the bundle in \"scripts\" is not a list - send one \"edits\" list beside \"scripts\", one entry per case";

/// What `dry_run` does on `save_autorun_script`.
const DRY_RUN_SCRIPT: &str = "true: run every check a save makes, the seen check included, and answer \"Dry run: nothing was saved.\" with what it would save, or the refusal a save would give (every locator never seen, listed at once), writing nothing and recording nothing.";

/// What `dry_run` does on `save_autorun_component`.
const DRY_RUN_COMPONENT: &str = "true: run every rule a save makes and answer would_save or the refusal, writing nothing and recording nothing. It is not a try of the component.";

/// Said when `save_autorun_component` is sent `remove: true` with anything
/// but the name: a removal saves nothing, so a component beside it is a
/// mistake to name, not to drop without a word.
pub const REMOVE_TAKES_ONLY_THE_NAME: &str = "`remove: true` takes only the component's `name` - send the rest without `remove` to save it";

/// Said when `save_autorun_component` is sent a `remove` that is not true or
/// false.
pub const REMOVE_IS_TRUE_OR_FALSE: &str = "`remove` is true or false";

/// `save_autorun_component`: a save, or with `remove: true` (and only the
/// name) the removal of a saved one through its own route.
fn component_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    match args.get("remove") {
        Some(serde_json::Value::Bool(true)) => {
            let others = args
                .as_object()
                .map(|o| o.iter().any(|(k, v)| k != "name" && k != "remove" && !v.is_null()))
                .unwrap_or(false);
            if others {
                return Ok((400, REMOVE_TAKES_ONLY_THE_NAME.to_string()));
            }
            call("POST", "/autorun-component-retire", &serde_json::json!({ "name": args["name"] }).to_string())
        }
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => {
            let mut body = args.clone();
            if let Some(o) = body.as_object_mut() {
                o.remove("remove");
            }
            call("POST", "/autorun-component-save", &body.to_string())
        }
        Some(_) => Ok((400, REMOVE_IS_TRUE_OR_FALSE.to_string())),
    }
}

/// Whether the arguments carry `key` with a value: a `null` is the same as
/// leaving it out.
fn given(args: &serde_json::Value, key: &str) -> bool {
    args.get(key).is_some_and(|v| !v.is_null())
}

/// The arguments' keys with a value, other than `allowed`.
fn others<'a>(args: &'a serde_json::Value, allowed: &[&str]) -> Vec<&'a str> {
    args.as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, v)| !v.is_null() && !allowed.contains(&k.as_str()))
                .map(|(k, _)| k.as_str())
                .collect()
        })
        .unwrap_or_default()
}

/// A boolean switch argument: absent, null or false is no; anything but a
/// boolean is refused by name.
fn flag(args: &serde_json::Value, key: &str) -> Result<bool, String> {
    match args.get(key) {
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => Ok(false),
        Some(serde_json::Value::Bool(true)) => Ok(true),
        Some(_) => Err(format!("`{key}` is true or false")),
    }
}

/// Said when `discover_autorun_actions` is sent both `action` and
/// `actions`, or neither.
pub const DISCOVER_ACTION_OR_ACTIONS: &str =
    "send `action` (one action) or `actions` (a list of at most 20) - one of them, not both";

/// Said when `discover_autorun_actions` is sent `stop_on_failure` with one
/// `action`: there is nothing after it to stop.
pub const STOP_ON_FAILURE_GOES_WITH_ACTIONS: &str =
    "`stop_on_failure` goes with `actions`: one `action` has nothing after it to stop";

/// `discover_autorun_actions`: one `action` through the one-action route,
/// whose answer is the richer one (dialogs, address change, writes), or
/// `actions` through the batch route. The arguments travel whole either
/// way: each route reads its own named fields.
fn discover_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    match (given(args, "action"), given(args, "actions")) {
        (true, false) if given(args, "stop_on_failure") => Ok((400, STOP_ON_FAILURE_GOES_WITH_ACTIONS.to_string())),
        (true, false) => call("POST", "/autorun-discover-action", &args.to_string()),
        (false, true) => call("POST", "/autorun-discover-actions", &args.to_string()),
        _ => Ok((400, DISCOVER_ACTION_OR_ACTIONS.to_string())),
    }
}

/// The parts of one element `probe_autorun_locator` shows with `show`.
pub const SHOW_PARTS: [&str; 4] = ["styles", "listeners", "animations", "structure"];

/// Said when `probe_autorun_locator` is sent a `show` that is neither true
/// nor a list of parts.
pub const SHOW_IS_TRUE_OR_PARTS: &str =
    "`show` is true (every part) or a list of the parts to show: styles, listeners, animations, structure";

/// Said when `probe_autorun_locator` is sent `properties` without `show`:
/// a plain probe reads no styles.
pub const PROPERTIES_GO_WITH_SHOW: &str =
    "`properties` goes with `show`: a plain probe reads no styles - send `show: [\"styles\"]` (or true) with them";

/// `probe_autorun_locator`: a probe through its own route, or with `show`
/// the read-only look at one element (the inspect route, which takes the
/// locator as `locator`; `show: true` is every part, the route's default).
fn probe_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    let show = match args.get("show") {
        None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => None,
        Some(serde_json::Value::Bool(true)) => Some(None),
        Some(parts @ serde_json::Value::Array(_)) => Some(Some(parts.clone())),
        Some(_) => return Ok((400, SHOW_IS_TRUE_OR_PARTS.to_string())),
    };
    let Some(parts) = show else {
        if given(args, "properties") {
            return Ok((400, PROPERTIES_GO_WITH_SHOW.to_string()));
        }
        return call("POST", "/autorun-probe", &args.to_string());
    };
    let mut body = serde_json::Map::new();
    body.insert("locator".to_string(), args["selector"].clone());
    if let Some(parts) = parts {
        body.insert("show".to_string(), parts);
    }
    if given(args, "properties") {
        body.insert("properties".to_string(), args["properties"].clone());
    }
    call("POST", "/autorun-inspect", &serde_json::Value::Object(body).to_string())
}

/// Said when `replay_autorun_to_step` is sent `stop: true` with anything
/// else: a stop is of the replay going, whichever it is.
pub const STOP_TAKES_NOTHING_ELSE: &str =
    "`stop: true` takes nothing else - it stops your replay that is going, whichever case and step it is at";

/// `replay_autorun_to_step`: a replay, or with `stop: true` (alone) the stop
/// of the assistant's own replay that is going.
fn replay_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    match flag(args, "stop") {
        Err(refused) => Ok((400, refused)),
        Ok(true) if !others(args, &["stop"]).is_empty() => Ok((400, STOP_TAKES_NOTHING_ELSE.to_string())),
        Ok(true) => call("POST", "/autorun-replay-stop", "{}"),
        Ok(false) => {
            let mut body = args.clone();
            if let Some(o) = body.as_object_mut() {
                o.remove("stop");
            }
            call("POST", "/autorun-replay", &body.to_string())
        }
    }
}

/// `reset_autorun_browser`: a reset, or with `release: true` the release of
/// a broken browser.
fn reset_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    match flag(args, "release") {
        Err(refused) => Ok((400, refused)),
        Ok(true) => call("POST", "/autorun-release", "{}"),
        Ok(false) => call("POST", "/autorun-reset", "{}"),
    }
}

/// The search filters of `search_autorun_requests`: with none of them it
/// is the plain list.
pub const REQUEST_FILTERS: [&str; 6] = ["path", "method", "status", "type", "request_contains", "response_contains"];

/// `search_autorun_requests`: the plain list (the cheaper read: it takes
/// only the page) when no filter is given, the search otherwise. The
/// arguments travel whole.
fn requests_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    if REQUEST_FILTERS.iter().any(|k| given(args, k)) {
        call("POST", "/autorun-requests-search", &args.to_string())
    } else {
        call("POST", "/autorun-requests", &args.to_string())
    }
}

/// Said when `get_autorun_request` is sent a `part` it does not read.
pub const PART_REFUSED: &str = "`part` is \"request\" (the default), \"response\" or \"body\" (the response's body)";

/// Said when `get_autorun_request` is sent an `offset` for the response's
/// part, which has no text to page.
pub const RESPONSE_HAS_NO_OFFSET: &str =
    "`offset` pages a body: it goes with part \"request\" or \"body\", not \"response\"";

/// `get_autorun_request`: one captured exchange - the request, its
/// response or the response's body - through that part's own route, the
/// arguments whole but for `part`.
fn request_part_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    let route = match args.get("part") {
        None | Some(serde_json::Value::Null) => "/autorun-request",
        Some(p) => match p.as_str() {
            Some("request") => "/autorun-request",
            Some("response") => "/autorun-response",
            Some("body") => "/autorun-response-body",
            _ => return Ok((400, PART_REFUSED.to_string())),
        },
    };
    if route == "/autorun-response" && given(args, "offset") {
        return Ok((400, RESPONSE_HAS_NO_OFFSET.to_string()));
    }
    let mut body = args.clone();
    if let Some(o) = body.as_object_mut() {
        o.remove("part");
    }
    call("POST", route, &body.to_string())
}

/// The fields `autorun_capture` sets; with none of them it reads.
pub const CAPTURE_FILTER_FIELDS: [&str; 3] = ["include_types", "exclude_types", "path_contains"];

/// `autorun_capture`: the filter read, or - with any filter field, even an
/// empty one - the whole filter set from now on.
fn capture_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    if CAPTURE_FILTER_FIELDS.iter().any(|k| given(args, k)) {
        call("POST", "/autorun-capture", &args.to_string())
    } else {
        call("GET", "/autorun-capture", "")
    }
}

/// Said when `autorun_har` is sent both `export` and `import`, or neither.
pub const HAR_EXPORT_OR_IMPORT: &str =
    "send `export` (a name to save the live capture as) or `import` (the name of a file in TCM's captures folder) - one of them, not both";

/// `autorun_har`: the export or the import route, each with the file's
/// name as its own `name`.
fn har_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    let route = match (given(args, "export"), given(args, "import")) {
        (true, false) => ("/autorun-requests-export", &args["export"]),
        (false, true) => ("/autorun-requests-import", &args["import"]),
        _ => return Ok((400, HAR_EXPORT_OR_IMPORT.to_string())),
    };
    call("POST", route.0, &serde_json::json!({ "name": route.1 }).to_string())
}

/// What `autorun_cookies` takes with each `action`.
pub const COOKIE_LIST_FIELDS: [&str; 1] = ["url"];
pub const COOKIE_SET_FIELDS: [&str; 6] = ["name", "value", "path", "expires", "secure", "same_site"];

/// Said when `autorun_cookies` is sent no `action`, or one it does not take.
/// There is no default: `clear` signs out.
pub const COOKIE_ACTION_REFUSED: &str =
    "`action` is needed: \"list\" (read the cookies), \"set\" (one test cookie) or \"clear\" (a real sign-out) - there is no default";

/// `autorun_cookies`: one route per action, each sent only the fields its
/// action takes - anything else is refused by name, so a `clear` meant for
/// one cookie never signs out the whole site.
fn cookies_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    let (route, fields): (&str, &[&str]) = match args.get("action").and_then(|a| a.as_str()) {
        Some("list") => ("/autorun-cookies", &COOKIE_LIST_FIELDS[..]),
        Some("set") => ("/autorun-cookie", &COOKIE_SET_FIELDS[..]),
        Some("clear") => ("/autorun-cookies-clear", &[]),
        _ => return Ok((400, COOKIE_ACTION_REFUSED.to_string())),
    };
    let mut allowed = vec!["action"];
    allowed.extend_from_slice(fields);
    let wrong = others(args, &allowed);
    if !wrong.is_empty() {
        let action = args["action"].as_str().unwrap_or_default();
        let takes = if fields.is_empty() { "nothing else".to_string() } else { fields.join(", ") };
        return Ok((400, format!("`action: \"{action}\"` takes {takes} - not {}", wrong.join(", "))));
    }
    let mut body = args.clone();
    if let Some(o) = body.as_object_mut() {
        o.remove("action");
    }
    call("POST", route, &body.to_string())
}

/// Said when `open_report` is sent a new report's summary fields beside
/// `create` rather than in it - the old `new_report`'s shape.
pub const SUMMARY_GOES_IN_CREATE: &str = "title, author and comments go inside `create`: { \"path\": ..., \"create\": { \"title\", \"author\", \"comments\" } }";

/// Said when `open_report` is sent a `create` that is neither a yes nor the
/// new report's summary information.
pub const CREATE_REFUSED: &str = "`create` is true, or the new report's { title, author, comments }";

/// Said when the fixtures list is sent a template filter.
pub const FIXTURES_TAKE_ID_OR_FULL: &str = "with kind \"fixtures\" only `id` and `full` apply - module, search, flow, offset and limit filter templates";

/// Said when the template list is sent `full`, which only the fixtures list
/// reads.
pub const FULL_IS_FOR_FIXTURES: &str = "`full` is for kind \"fixtures\" - a template list pages with offset and limit, or answers one template in full by `id`";

/// Said when `list_api_templates` is sent a `kind` it does not list.
pub const LIST_KIND_REFUSED: &str = "`kind` is \"templates\" (the default) or \"fixtures\"";

/// Said when `record_quirk` is sent a line to record and a quirk to retire
/// at once: one call does one of them.
pub const QUIRK_RECORD_OR_RETIRE: &str = "send `text` to record a quirk, or `retire` with a quirk's id to retire one - not both (a better line goes in `replacement`)";

/// Said when `record_quirk` is sent an `id` without `retire`: the old retire
/// tool's shape.
pub const QUIRK_ID_WITHOUT_RETIRE: &str = "to retire a quirk, send `retire: <its id>` with a `reason` - `id` is not an argument here; to record one, send `text`";

/// `record_quirk`'s arguments with `from: "api"` filled in when the caller
/// left `from` out and the Auto Run row's own tools are all off: then the
/// API templates row is the only reason the tool is offered, so the note is
/// an API templates assistant's - what the old `record_app_quirk` said for
/// it. A `from` the caller gave is never changed.
fn with_inferred_from(args: &serde_json::Value, off: &[String]) -> serde_json::Value {
    let mut args = args.clone();
    if !says(&args["from"]) && args.is_object() && !guide_offered("autorun", off) {
        args["from"] = serde_json::json!(crate::autorun::quirks::FROM_API);
    }
    args
}

/// Said when `record_quirk` retires a quirk without saying why.
pub const QUIRK_RETIRE_NEEDS_REASON: &str = "retiring a quirk needs a `reason`: one sentence on why the note no longer helps";

/// `record_quirk`: with `retire`, the retire route, the id carried as the
/// route's own `id`; otherwise the record route. Either way the arguments
/// travel whole, `from` included, so the bridge still refuses a quirk sent
/// `from: "api"` while API Templates is off (`ai_bridge::module_guard`).
fn quirk_call(args: &serde_json::Value, call: BridgeCall) -> Result<(u16, String), String> {
    let Some(id) = args.get("retire").filter(|v| !v.is_null()) else {
        if says(&args["id"]) {
            return Ok((400, QUIRK_ID_WITHOUT_RETIRE.to_string()));
        }
        return call("POST", "/autorun-quirk", &args.to_string());
    };
    if args.get("text").is_some_and(|t| !t.is_null()) {
        return Ok((400, QUIRK_RECORD_OR_RETIRE.to_string()));
    }
    if args.get("reason").is_none_or(|r| r.is_null()) {
        return Ok((400, QUIRK_RETIRE_NEEDS_REASON.to_string()));
    }
    let mut body = args.as_object().cloned().unwrap_or_default();
    body.remove("retire");
    body.insert("id".to_string(), id.clone());
    call("POST", "/autorun-quirk-retire", &serde_json::Value::Object(body).to_string())
}

/// The script save's body with the tool's `dry_run` carried into it: the
/// bare array becomes `{ "scripts": [...], "dry_run": ... }`, and an object
/// body gets the key. Without one, or with a body that is not JSON (the
/// route names what is wrong with it), the body is unchanged.
fn with_dry_run(body: String, dry_run: Option<&serde_json::Value>) -> String {
    let Some(flag) = dry_run.filter(|v| !v.is_null()) else { return body };
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(scripts @ serde_json::Value::Array(_)) => {
            serde_json::json!({ "scripts": scripts, "dry_run": flag }).to_string()
        }
        Ok(serde_json::Value::Object(mut bundle)) => {
            bundle.insert("dry_run".to_string(), flag.clone());
            serde_json::Value::Object(bundle).to_string()
        }
        _ => body,
    }
}

/// The bundle's own `edits` with the ones sent beside it added, exact
/// duplicates dropped. Either side may be a list, one entry, or a JSON
/// string of either. When both are there and one of them is anything else,
/// the call is refused rather than one side being dropped without a word.
/// With only one side present it is forwarded as it is, and the route
/// says what is wrong with it.
fn merge_bundle_edits(
    mut bundle: serde_json::Map<String, serde_json::Value>,
    beside: Option<serde_json::Value>,
) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let parsed = |v: serde_json::Value| match v {
        serde_json::Value::String(s) => match serde_json::from_str::<serde_json::Value>(&s) {
            Ok(inner @ (serde_json::Value::Array(_) | serde_json::Value::Object(_))) => inner,
            _ => serde_json::Value::String(s),
        },
        other => other,
    };
    let as_list = |v: serde_json::Value| match v {
        serde_json::Value::Array(list) => Some(list),
        one @ serde_json::Value::Object(_) => Some(vec![one]),
        _ => None,
    };
    let own = bundle.remove("edits").filter(|e| !e.is_null()).map(parsed);
    let merged = match (own, beside.map(parsed)) {
        (None, None) => None,
        (Some(one_side), None) | (None, Some(one_side)) => Some(one_side),
        (Some(own), Some(beside)) => {
            let mut list = as_list(own).ok_or_else(|| BUNDLE_EDITS_NOT_A_LIST.to_string())?;
            for e in as_list(beside).ok_or_else(|| BESIDE_EDITS_NOT_A_LIST.to_string())? {
                if !list.contains(&e) {
                    list.push(e);
                }
            }
            Some(serde_json::Value::Array(list))
        }
    };
    if let Some(edits) = merged {
        bundle.insert("edits".to_string(), edits);
    }
    Ok(bundle)
}

/// Tools that were merged or renamed, as `(old name, the new tool and the
/// shape to send)`. A client working from a tool list it cached before an
/// update may still call the old name; it is told the new one rather than
/// "unknown tool" - or, for a name with "remove" in it, the Azure DevOps
/// write refusal. The names mirror `RENAMED_TOOLS` in `src/lib/mcpTools.ts`
/// (a test pins that).
pub const RETIRED_TOOLS: &[(&str, &str)] = &[
    ("get_run_failures", "`get_run_results`."),
    ("record_autorun_quirk", "`record_quirk` with `text` (and `cases`)."),
    ("record_app_quirk", "`record_quirk` with `text` (and `cases`, `from`)."),
    ("retire_autorun_quirk", "`record_quirk` with `retire` (the quirk's id) and `reason`."),
    ("retire_app_quirk", "`record_quirk` with `retire` (the quirk's id) and `reason`."),
    ("get_writing_guide", "`get_guide` with `topic` \"writing\"."),
    ("get_autorun_guide", "`get_guide` with `topic` \"autorun\" (and `section`, `quick` or `full`)."),
    ("get_api_template_guide", "`get_guide` with `topic` \"api_templates\"."),
    ("get_report_guide", "`get_guide` with `topic` \"reports\"."),
    ("list_test_files", "`get_accounts`, whose answer carries the project's `test_files`."),
    ("list_api_fixtures", "`list_api_templates` with `kind` \"fixtures\"."),
    ("new_report", "`open_report` with `path` and `create` (true, or { title, author, comments })."),
    ("remove_autorun_component", "`save_autorun_component` with `name` and `remove` true, nothing else."),
    ("discover_autorun_action", "`discover_autorun_actions` with `action` (one) or `actions` (a list)."),
    ("inspect_autorun_element", "`probe_autorun_locator` with `selector` and `show` (true, or a list of parts)."),
    ("stop_autorun_replay", "`replay_autorun_to_step` with `stop` true, nothing else."),
    ("release_autorun_browser", "`reset_autorun_browser` with `release` true."),
    ("list_autorun_requests", "`search_autorun_requests` (no filter lists them all)."),
    ("get_autorun_response", "`get_autorun_request` with `id` and `part` \"response\"."),
    ("get_autorun_response_body", "`get_autorun_request` with `id` and `part` \"body\"."),
    ("set_autorun_capture", "`autorun_capture` with the filter fields to set."),
    ("get_autorun_capture", "`autorun_capture` with no filter fields."),
    ("export_autorun_requests", "`autorun_har` with `export` (the name to save as)."),
    ("import_autorun_requests", "`autorun_har` with `import` (the name of a file in the captures folder)."),
    ("get_autorun_cookies", "`autorun_cookies` with `action` \"list\"."),
    ("set_autorun_cookie", "`autorun_cookies` with `action` \"set\", `name` and `value`."),
    ("clear_autorun_cookies", "`autorun_cookies` with `action` \"clear\"."),
];

/// What a call to a retired tool is told, or `None` for any other name.
pub fn retired_tool_answer(name: &str) -> Option<String> {
    let (_, now) = RETIRED_TOOLS.iter().find(|(old, _)| *old == name)?;
    Some(format!("`{name}` is now {now}"))
}

fn tools_call(params: &serde_json::Value, call: BridgeCall) -> serde_json::Value {
    let name = params["name"].as_str().unwrap_or_default();
    let args = &params["arguments"];
    // Every bridge request from here on names the tool, so the app logs the
    // call once: a refusal on the `/tools` read, otherwise the route's answer.
    let _naming = CallingTool::set(name);
    // A tool that was merged away answers with where it went, before any
    // name-based refusal can misread it.
    if let Some(text) = retired_tool_answer(name) {
        return serde_json::json!({ "content": [{ "type": "text", "text": text }], "isError": true });
    }
    // Checked again here, not just in tools/list: a client may be working
    // from a list it cached before the tool was switched off.
    let (off, on) = tool_policy(call);
    let refused = if off.iter().any(|d| d == name) {
        Some(refusal_text(name, &on))
    } else if name == "get_guide" {
        guide_refusal(args["topic"].as_str(), &off, &on)
    } else {
        None
    };
    if let Some(text) = refused {
        return serde_json::json!({
            "content": [{
                "type": "text",
                "text": text,
            }],
            "isError": true,
        });
    }
    let outcome = match name {
        "begin_test_case_writing" => {
            let feature = args["feature"].as_str().unwrap_or("");
            let target = if feature.is_empty() {
                "/begin".to_string()
            } else {
                format!("/begin?feature={}", urlencoding::encode(feature))
            };
            // Everything except `feature` is an answer. With none of them
            // present this is phase 1 (the questions); with any of them it
            // is phase 2 (check and plan).
            let answers: serde_json::Map<String, serde_json::Value> = args
                .as_object()
                .map(|o| {
                    o.iter()
                        .filter(|(k, _)| k.as_str() != "feature")
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default();
            let body = if answers.is_empty() {
                String::new()
            } else {
                serde_json::Value::Object(answers).to_string()
            };
            call("POST", &target, &body)
        }
        "get_guide"
            if args["topic"].as_str().map(str::trim) != Some("autorun")
                && ["section", "quick", "full"].iter().any(|k| says(&args[*k])) =>
        {
            Ok((400, GUIDE_PARTS_AUTORUN_ONLY.to_string()))
        }
        "get_guide" => match args["topic"].as_str().map(str::trim).unwrap_or_default() {
            "writing" => call("GET", "/guide", ""),
            "autorun" => match args["section"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                Some(section) => call("GET", &format!("/autorun-guide?section={}", urlencoding::encode(section)), ""),
                None if args["quick"].as_bool().unwrap_or(false) => call("GET", "/autorun-guide?quick=true", ""),
                None if args["full"].as_bool().unwrap_or(false) => call("GET", "/autorun-guide", ""),
                // The whole guide is too long for one answer: the Quick rules,
                // the live sections and the section titles.
                None => call("GET", "/autorun-guide?overview=true", ""),
            },
            "api_templates" => call("GET", "/api-template-guide", ""),
            "reports" => call("GET", "/crystal-guide", ""),
            // Refused before the match by `guide_refusal`; said again here
            // rather than trusted.
            _ => Ok((400, guide_refusal(args["topic"].as_str(), &off, &on).unwrap_or_default())),
        },
        "get_test_cases" => {
            let limit = args["limit"].as_i64().unwrap_or(5);
            let offset = args["offset"].as_i64().unwrap_or(0);
            let mut params: Vec<String> = vec![];
            if let Some(pbi) = args["pbi_id"].as_i64() {
                params.push(format!("pbi={pbi}"));
            }
            // A bare number is taken as a one-id list. Strings pass through
            // as written, so a malformed id reaches the bridge and is refused
            // by name instead of vanishing here.
            let ids: Vec<String> = match &args["case_ids"] {
                serde_json::Value::Array(items) => items
                    .iter()
                    .map(|i| i.as_i64().map(|n| n.to_string()).unwrap_or_else(|| i.as_str().unwrap_or_default().trim().to_string()))
                    .collect(),
                serde_json::Value::Number(n) => vec![n.to_string()],
                _ => vec![],
            };
            if !ids.is_empty() {
                params.push(format!("ids={}", ids.iter().map(|i| urlencoding::encode(i)).collect::<Vec<_>>().join(",")));
            }
            params.push(format!("limit={limit}&offset={offset}"));
            let mut target = format!("/test-cases?{}", params.join("&"));
            if args["titles_only"].as_bool().unwrap_or(false) {
                target.push_str("&titles_only=true");
            }
            call("GET", &target, "")
        }
        "search_test_suites" => {
            let mut params: Vec<String> = vec![];
            if let Some(f) = args["query"].as_str().filter(|f| !f.trim().is_empty()) {
                params.push(format!("q={}", urlencoding::encode(f)));
            }
            if args["refresh"].as_bool().unwrap_or(false) {
                params.push("refresh=true".to_string());
            }
            let target = if params.is_empty() {
                "/suites".to_string()
            } else {
                format!("/suites?{}", params.join("&"))
            };
            call("GET", &target, "")
        }
        "get_suite_test_cases" => {
            let plan = args["plan_id"].as_i64().unwrap_or(0);
            let suite = args["suite_id"].as_i64().unwrap_or(0);
            let limit = args["limit"].as_i64().unwrap_or(5);
            let offset = args["offset"].as_i64().unwrap_or(0);
            let mut target = format!("/suite-cases?plan={plan}&suite={suite}&limit={limit}&offset={offset}");
            if args["titles_only"].as_bool().unwrap_or(false) {
                target.push_str("&titles_only=true");
            }
            if args["include_children"].as_bool().unwrap_or(false) {
                target.push_str("&children=true");
            }
            call("GET", &target, "")
        }
        "get_run_results" => {
            let pbi = args["pbi_id"].as_i64().unwrap_or(0);
            // One word, or a list: both reach the bridge as one
            // comma-separated `outcome`, which it parses and validates.
            let outcome = match &args["outcome"] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Array(list) => {
                    list.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(",")
                }
                _ => String::new(),
            };
            let mut target = format!("/run-results?pbi={pbi}");
            if !outcome.trim().is_empty() {
                target.push_str(&format!("&outcome={}", urlencoding::encode(&outcome)));
            }
            call("GET", &target, "")
        }
        "check_spec_coverage" => call("POST", "/check-coverage", &args.to_string()),
        // The bridge takes one body, so forwarding the raw arguments object
        // is structurally unable to drop `paths` or `output_path` - the
        // same pattern as `check_spec_coverage` above.
        "merge_case_files" => call("POST", "/merge-cases", &args.to_string()),
        "save_autorun_script" => {
            // The bridge takes the bare array, so a caller that wrapped it
            // in {scripts: [...]} and one that sent the list directly both
            // work - the wrapper is a JSON-schema convenience, not a shape
            // the assistant should have to get right twice. Every sibling
            // tool on this server (transform_cases, validate_cases, ...)
            // takes its JSON payload as a STRING, so a caller that follows
            // that pattern here sends {"scripts": "[...]"} - a
            // Value::String, not a Value::Array. `.to_string()` on that
            // would re-quote and escape it into a JSON string literal
            // instead of forwarding the array text, so it is unwrapped
            // first. `edits` travels the same way, and when it is present
            // the body has to be the object shape that carries both.
            //
            // Built with `json!` rather than by pasting text together: a
            // `scripts` string that is not valid JSON would otherwise be
            // spliced in raw and turn the whole body into something the
            // route cannot even parse, so the real complaint ("this is
            // not a list of scripts") would be replaced by a syntax
            // error pointing at nothing.
            let value_of = |key: &str| match args.get(key) {
                Some(serde_json::Value::String(s)) => Some(
                    serde_json::from_str::<serde_json::Value>(s)
                        .unwrap_or_else(|_| serde_json::Value::String(s.clone())),
                ),
                Some(v) => Some(v.clone()),
                None => None,
            };
            // `"edits": null` declares nothing: the same as leaving it out.
            let edits = value_of("edits").filter(|e| !e.is_null());
            let body: Result<String, String> = match (value_of("scripts"), edits) {
                // The guide's whole body, { scripts, edits }, sent AS
                // `scripts` (an object or its text). Wrapping it again
                // would bury its scripts one level down, so it is the
                // body, with any `edits` sent beside it added to its own.
                // Its other keys stay, for the route to name.
                (Some(serde_json::Value::Object(bundle)), beside) if bundle.contains_key("scripts") => {
                    merge_bundle_edits(bundle, beside).map(|b| serde_json::Value::Object(b).to_string())
                }
                // Nothing to declare: the bare array, exactly as before.
                // An unparseable string is forwarded as it was written so
                // the route's own message names what is wrong with it.
                (Some(serde_json::Value::String(s)), None) => Ok(s),
                (Some(scripts), None) => Ok(scripts.to_string()),
                (Some(scripts), Some(edits)) => {
                    Ok(serde_json::json!({ "scripts": scripts, "edits": edits }).to_string())
                }
                (None, _) => Ok(args.to_string()),
            };
            match body {
                Ok(body) => call("POST", "/autorun-script", &with_dry_run(body, args.get("dry_run"))),
                // Refused here, before anything reaches the app, in the
                // same shape as a refusal from the route.
                Err(refused) => Ok((400, refused)),
            }
        }
        "get_autorun_script" => {
            // A number, or a numeric string: anything else is forwarded as
            // written, so the route names what is wrong with it.
            let id = match &args["case_id"] {
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s.trim().to_string(),
                _ => String::new(),
            };
            call("GET", &format!("/autorun-script?case_id={}", urlencoding::encode(&id)), "")
        }
        "delete_autorun_script" => call("POST", "/autorun-script-delete", &args.to_string()),
        "get_autorun_network_wait" => call("GET", "/autorun-network-wait", ""),
        // The network tools: each route reads its own named fields out of
        // the body; a tool that chooses between routes says why in its
        // own function.
        "search_autorun_requests" => requests_call(args, call),
        "get_autorun_request" => request_part_call(args, call),
        "autorun_capture" => capture_call(args, call),
        "autorun_har" => har_call(args, call),
        "set_autorun_headers" => call("POST", "/autorun-headers", &args.to_string()),
        "autorun_cookies" => cookies_call(args, call),
        "get_autorun_page" => {
            let mut params: Vec<String> = Vec::new();
            if let Some(n) = args["limit"].as_u64() {
                params.push(format!("limit={n}"));
            }
            // A locator travels as its JSON text; a css string as itself.
            match &args["within"] {
                serde_json::Value::Null => {}
                serde_json::Value::String(s) => params.push(format!("within={}", urlencoding::encode(s))),
                other => params.push(format!("within={}", urlencoding::encode(&other.to_string()))),
            }
            // Forwarded as written, so the route names what is wrong with it.
            match &args["nth"] {
                serde_json::Value::Null => {}
                serde_json::Value::Number(n) => params.push(format!("nth={n}")),
                serde_json::Value::String(s) => params.push(format!("nth={}", urlencoding::encode(s))),
                other => params.push(format!("nth={}", urlencoding::encode(&other.to_string()))),
            }
            let target = if params.is_empty() { "/autorun-page".to_string() } else { format!("/autorun-page?{}", params.join("&")) };
            call("GET", &target, "")
        }
        // The bridge reads one named field out of the body, so forwarding
        // the raw arguments object is structurally unable to drop it -
        // the same pattern as `check_spec_coverage`.
        "probe_autorun_locator" => probe_call(args, call),
        "try_autorun_action" => call("POST", "/autorun-try", &args.to_string()),
        "replay_autorun_to_step" => replay_call(args, call),
        // Discovery: each route reads its own named fields out of the body,
        // so the arguments object travels whole.
        "start_autorun_discovery" => call("POST", "/autorun-discover-start", &args.to_string()),
        "discover_autorun_actions" => discover_call(args, call),
        "save_autorun_area" => call("POST", "/autorun-discover-area", &args.to_string()),
        "end_autorun_discovery" => call("POST", "/autorun-discover-end", &args.to_string()),
        "reset_autorun_browser" => reset_call(args, call),
        // Components: each route reads its own named fields out of the body.
        "save_autorun_component" => component_call(args, call),
        "get_autorun_failures" => {
            let mut params: Vec<String> = vec![];
            if let Some(id) = args["case_id"].as_i64() {
                params.push(format!("case_id={id}"));
            }
            if let Some(id) = args["run_id"].as_str().filter(|s| !s.trim().is_empty()) {
                params.push(format!("run_id={}", urlencoding::encode(id.trim())));
            }
            let target = if params.is_empty() {
                "/autorun-failures".to_string()
            } else {
                format!("/autorun-failures?{}", params.join("&"))
            };
            call("GET", &target, "")
        }
        "record_quirk" => quirk_call(&with_inferred_from(args, &off), call),
        "record_validation_rule" => call("POST", "/autorun-validation-rule", &args.to_string()),
        "mark_autorun_suspected_defect" => call("POST", "/autorun-defect", &args.to_string()),
        "set_autorun_order" => call("POST", "/autorun-order", &args.to_string()),
        "propose_accounts" => call("POST", "/accounts-propose", &args.to_string()),
        "get_accounts" => call("GET", "/accounts", ""),
        "list_api_templates"
            if args["kind"].as_str() == Some("fixtures")
                && ["module", "search", "flow", "offset", "limit"].iter().any(|k| says(&args[*k])) =>
        {
            Ok((400, FIXTURES_TAKE_ID_OR_FULL.to_string()))
        }
        "list_api_templates" if args["kind"].as_str() != Some("fixtures") && says(&args["full"]) => {
            Ok((400, FULL_IS_FOR_FIXTURES.to_string()))
        }
        "list_api_templates" if args["kind"].as_str() == Some("fixtures") => {
            match args["id"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                Some(id) => call("GET", &format!("/api-template-fixtures?id={}", urlencoding::encode(id)), ""),
                None if args["full"].as_bool().unwrap_or(false) => call("GET", "/api-template-fixtures?full=true", ""),
                None => call("GET", "/api-template-fixtures", ""),
            }
        }
        // Any other `kind` is refused by name rather than read as templates.
        "list_api_templates" if !matches!(args.get("kind"), None | Some(serde_json::Value::Null)) && args["kind"].as_str() != Some("templates") => {
            Ok((400, LIST_KIND_REFUSED.to_string()))
        }
        "list_api_templates" => {
            // Text filters pass through percent-encoded. Offset and limit
            // may arrive as numbers or as numeric strings; anything else
            // given for them (a boolean, a list, a negative or fractional
            // number) is forwarded as written, so the route refuses it by
            // name rather than this dispatch quietly dropping it.
            let mut params: Vec<String> = vec![];
            // `flow: false` is not a flow id: it leaves the flows and the
            // Test files out of an answer in full.
            if args["flow"] == serde_json::json!(false) {
                params.push("with_flow=false".to_string());
            }
            for key in ["module", "search", "flow", "id", "offset", "limit"] {
                let paging = matches!(key, "offset" | "limit");
                let value = match &args[key] {
                    serde_json::Value::String(s) if !s.trim().is_empty() => s.trim().to_string(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Null | serde_json::Value::String(_) => continue,
                    other if paging => other.to_string(),
                    _ => continue,
                };
                params.push(format!("{key}={}", urlencoding::encode(&value)));
            }
            let target = if params.is_empty() {
                "/api-templates".to_string()
            } else {
                format!("/api-templates?{}", params.join("&"))
            };
            call("GET", &target, "")
        }
        // The bridge reads its fields out of the body (a template or
        // values sent as a JSON string included), so the arguments object
        // travels whole - the same pattern as `check_spec_coverage`.
        "prove_api_template" => call("POST", "/api-template-prove", &args.to_string()),
        "run_api_template" => call("POST", "/api-template-run", &args.to_string()),
        "save_api_flow" => call("POST", "/api-template-flow-save", &args.to_string()),
        "get_api_flow_progress" => call("POST", "/api-template-flow-progress", &args.to_string()),
        // The bridge reads the fixture (or the id) out of the body, so the
        // arguments object travels whole.
        "save_api_fixture" => call("POST", "/api-template-fixture-save", &args.to_string()),
        "run_api_fixture" => call("POST", "/api-template-fixture-run", &args.to_string()),
        // Both bridge routes read their fields out of the body, so
        // forwarding the raw arguments object is structurally unable to
        // drop one - the same pattern as `check_spec_coverage`.
        "db_lookup" => call("POST", "/db-lookup", &args.to_string()),
        "db_query" => call("POST", "/db-query", &args.to_string()),
        // The Crystal Reports routes read their fields out of the body, so
        // the arguments object travels whole, like the database tools'.
        // `create` makes a blank report first: the new route takes the path
        // and the summary fields side by side.
        "open_report" if ["title", "author", "comments"].iter().any(|k| says(&args[*k])) => {
            Ok((400, SUMMARY_GOES_IN_CREATE.to_string()))
        }
        "open_report" => match args.get("create") {
            None | Some(serde_json::Value::Null) | Some(serde_json::Value::Bool(false)) => {
                call("POST", "/crystal-open", &args.to_string())
            }
            Some(serde_json::Value::Bool(true)) => {
                call("POST", "/crystal-new", &serde_json::json!({ "path": args["path"] }).to_string())
            }
            Some(serde_json::Value::Object(summary)) => {
                let mut body = summary.clone();
                body.insert("path".to_string(), args["path"].clone());
                call("POST", "/crystal-new", &serde_json::Value::Object(body).to_string())
            }
            Some(_) => Ok((400, CREATE_REFUSED.to_string())),
        },
        "read_report" => call("POST", "/crystal-read", &args.to_string()),
        "edit_report" => call("POST", "/crystal-change", &args.to_string()),
        "compare_reports" => call("POST", "/crystal-compare", &args.to_string()),
        "check_report" => call("POST", "/crystal-check", &args.to_string()),
        "preview_report" => call("POST", "/crystal-preview", &args.to_string()),
        "preview_report_live" => call("POST", "/crystal-preview-live", &args.to_string()),
        "optimize_cases" => {
            let entry = args["entry"].as_str().unwrap_or("");
            let mut params: Vec<String> = vec![];
            if !entry.is_empty() {
                params.push(format!("entry={}", urlencoding::encode(entry)));
            }
            if args["dry_run"].as_bool().unwrap_or(false) {
                params.push("dry_run=true".to_string());
            }
            // Only sent when explicitly false - the bridge defaults to
            // reordering, and an absent flag has to mean the same thing.
            if args["reorder"].as_bool() == Some(false) {
                params.push("reorder=false".to_string());
            }
            if args["trim_expected"].as_bool() == Some(false) {
                params.push("trim_expected=false".to_string());
            }
            // Round 7 §1: a large draft travels as a file path, like every
            // other tool in the family. Forwarded only when present, so
            // the route can refuse path+json as two sources rather than
            // silently preferring one.
            if let Some(p) = args["path"].as_str().filter(|p| !p.trim().is_empty()) {
                params.push(format!("path={}", urlencoding::encode(p)));
            }
            if args["in_place"].as_bool().unwrap_or(false) {
                params.push("in_place=true".to_string());
            }
            let target = if params.is_empty() {
                "/optimize".to_string()
            } else {
                format!("/optimize?{}", params.join("&"))
            };
            call("POST", &target, args["json"].as_str().unwrap_or(""))
        }
        "validate_cases" => {
            let target = match args["path"].as_str().filter(|p| !p.trim().is_empty()) {
                Some(p) => format!("/validate?path={}", urlencoding::encode(p)),
                None => "/validate".to_string(),
            };
            call("POST", &target, args["json"].as_str().unwrap_or(""))
        }
        "transform_cases" => {
            // The bridge takes one body, so the draft and the ops travel
            // together rather than as a query string. Keys are forwarded
            // only when PRESENT - the old shape hardcoded json+operations,
            // which is precisely how a passed `path` was silently dropped
            // and the result built from an empty inline draft (round 5
            // §10). Absent stays absent so the route can tell "not given"
            // from "given empty".
            let mut body = serde_json::Map::new();
            if let Some(j) = args["json"].as_str() {
                body.insert("test_cases".into(), serde_json::json!(j));
            }
            if let Some(p) = args["path"].as_str() {
                body.insert("path".into(), serde_json::json!(p));
            }
            if let Some(w) = args["in_place"].as_bool() {
                body.insert("in_place".into(), serde_json::json!(w));
            }
            body.insert("operations".into(), args["operations"].clone());
            call("POST", "/transform", &serde_json::Value::Object(body).to_string())
        }
        "get_tags" => {
            let target = match args["query"].as_str().filter(|f| !f.trim().is_empty()) {
                Some(f) => format!("/tags?query={}", urlencoding::encode(f)),
                None => "/tags".to_string(),
            };
            call("GET", &target, "")
        }
        "search_pbis" => {
            let q = args["query"].as_str().unwrap_or("");
            call("GET", &format!("/search-pbis?q={}", urlencoding::encode(q)), "")
        }
        "search_wiki" => {
            let q = args["query"].as_str().unwrap_or("");
            call("GET", &format!("/search-wiki?q={}", urlencoding::encode(q)), "")
        }
        "get_wiki_page" => {
            let wiki_id = args["wiki_id"].as_str().unwrap_or("");
            let path = args["path"].as_str().unwrap_or("");
            call(
                "GET",
                &format!(
                    "/wiki-page?wiki={}&path={}",
                    urlencoding::encode(wiki_id),
                    urlencoding::encode(path)
                ),
                "",
            )
        }
        // An unknown tool whose NAME says the assistant wanted to mutate
        // Azure DevOps gets the refusal sentence, not a bare "unknown" -
        // "unknown tool update_test_case" reads as a spelling problem and
        // invites another guess; the refusal ends the attempt and points
        // at the path that is allowed.
        other
            if ["create", "update", "delete", "edit", "remove", "submit"]
                .iter()
                .any(|w| other.to_ascii_lowercase().contains(w)) =>
        {
            Err(ToolError::WRITE_REFUSED_TAG.to_string())
        }
        other => Err(format!("{}{other}", ToolError::UNKNOWN_TOOL_TAG)),
    };
    match outcome {
        Ok((status, body)) if status < 400 => serde_json::json!({
            "content": [{ "type": "text", "text": body }],
        }),
        Ok((status, body)) => serde_json::json!({
            "isError": true,
            "content": [{ "type": "text", "text": format!("bridge returned {status}: {body}") }],
        }),
        // Each failure says what actually went wrong. Until the 2026-08
        // audit (P-8) every one of these - including a plain typo in the
        // tool name - was reported as "Could not reach Test Case Manager.
        // Start the app and sign in", sending an assistant off to debug a
        // healthy bridge instead of fixing its own call.
        Err(e) => serde_json::json!({
            "isError": true,
            "content": [{ "type": "text", "text": ToolError::message(&e) }],
        }),
    }
}

/// Why a tool call could not be answered.
///
/// The dispatch arms above all yield `Result<_, String>` (the bridge's own
/// error type), so the KIND travels as a short internal prefix that this
/// classifier strips. Until the 2026-08 audit (P-8) there was no kind at
/// all: every failure - including a plain typo in the tool name - was
/// reported as "Could not reach Test Case Manager. Start the app and sign
/// in", which sent assistants off to debug a healthy bridge.
struct ToolError;

impl ToolError {
    // U+0001 cannot occur in a tool name or a bridge error message, so a
    // tag can never be produced by accident.
    const UNKNOWN_TOOL_TAG: &'static str = "\u{1}unknown-tool\u{1}";
    const WRITE_REFUSED_TAG: &'static str = "\u{1}write-refused\u{1}";

    /// The user-facing sentence for one failed tool call.
    fn message(raw: &str) -> String {
        if let Some(name) = raw.strip_prefix(Self::UNKNOWN_TOOL_TAG) {
            return format!(
                "Unknown tool: {name}. Call tools/list for the tools this server offers."
            );
        }
        if raw == Self::WRITE_REFUSED_TAG {
            return crate::ai_bridge::WRITE_REFUSAL.to_string();
        }
        format!("Could not reach Test Case Manager ({raw}). Start the app and sign in, then retry.")
    }
}

/// `/tools` as the proxy uses it: the app's answer, or - when the app cannot
/// be reached or does not answer 200 - this machine's saved modules in the
/// same shape. An editor keeps the tool list it got when it connected, so
/// one started before the app must still see the modules the person has on
/// (the app writes them to `modules.json`; "Updates take nothing away").
/// With no saved file (`None`) the original reply stands and no module's
/// tools are offered. The saved set is mended by `tool_policy_from`.
pub fn tools_reply_or_saved(
    reply: Result<(u16, String), String>,
    saved: Option<BTreeSet<Module>>,
) -> Result<(u16, String), String> {
    match (&reply, saved) {
        (Ok((200, _)), _) | (_, None) => reply,
        (_, Some(on)) => {
            let names: Vec<&str> = Module::ALL.into_iter().filter(|m| on.contains(m)).map(Module::key).collect();
            Ok((200, serde_json::json!({ "disabled": [], "modules": names }).to_string()))
        }
    }
}

fn bridge_call(method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    let reply = bridge_request(method, path, body);
    if method == "GET" && path == "/tools" {
        return tools_reply_or_saved(reply, crate::modules::saved_on_this_machine());
    }
    reply
}

fn bridge_request(method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    let hs: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(crate::ai_bridge::handshake_path())
            .map_err(|_| "handshake file missing - is the app running?".to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let port = hs["port"].as_u64().ok_or("bad handshake file")? as u16;
    let token = hs["token"].as_str().ok_or("bad handshake file")?;
    send_to_bridge(port, token, method, path, body)
}

/// One request to the app's bridge on `port`: the transport under
/// `bridge_request`, public so a test can drive a listener of its own
/// (the handshake file belongs to a live app). A request made during a
/// tool call carries the tool's name in `TOOL_HEADER`.
pub fn send_to_bridge(port: u16, token: &str, method: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    let client = reqwest::blocking::Client::new();
    let url = format!("http://127.0.0.1:{port}{path}");
    let mut req = match method {
        "POST" => client.post(&url).body(body.to_string()),
        _ => client.get(&url),
    };
    if let Some(tool) = calling_tool() {
        req = req.header(TOOL_HEADER, tool);
    }
    let resp = req
        .header("x-bridge-token", token)
        .header(PROXY_VERSION_HEADER, PROXY_VERSION)
        // 300s, not 30: get_run_results resolves a PBI's suite by
        // scanning every test plan in the project, throttle-paced - ~60s
        // against a large org on a cold cache. At 30s the proxy gave up
        // mid-scan and reported the app as unreachable, which it wasn't.
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp.text().map_err(|e| e.to_string())?;
    Ok((status, text))
}

/// The running app's version from the handshake file it already writes for
/// port/token - compared against `PROXY_VERSION`, never reported as this
/// proxy's own. "unknown" when the app isn't running yet; the proxy must
/// still answer `initialize`.
fn read_version() -> String {
    std::fs::read_to_string(crate::ai_bridge::handshake_path())
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["version"].as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

/// Entry point for `v2.exe --mcp`: MCP stdio bridge to a RUNNING Test Case
/// Manager. Reads the handshake file for the port + token, proxies each
/// tool call over localhost, and never sees credentials. Register in an AI
/// tool as: `claude mcp add tcm -- "<install dir>\v2.exe" --mcp`
pub fn run_stdio_proxy() {
    let version = read_version();
    // stderr is the MCP client's log: the same sentence the assistant gets
    // in `instructions`, where a person reading the client's logs finds it.
    if let Some(warning) = version_warning(PROXY_VERSION, &version) {
        eprintln!("{warning}");
    }
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_message(&line, &version, &bridge_call) {
            let _ = writeln!(stdout, "{resp}");
            let _ = stdout.flush();
        }
    }
}
