//! The rules each part of the scripting work follows, carried over from the
//! Playwright pipeline's agent files (`.claude/agents/*.md` and `/gen-test`
//! in the QA automation repo) and adapted to TCM's tools: the writer
//! (playwright-test-planner and -generator), the healer
//! (playwright-test-healer and the healer bounds), the case rewriter
//! (test-refactorer and spec-editor) and the auditor (doc-drift-auditor).
//! Each role's text is in `roles/<key>.md`; an MCP session gets it ahead of
//! the first answer of the tool that starts the role (`mcp_session`), and
//! `get_guide` gives it by name. They hold whatever tools the assistant has.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    Writer,
    Healer,
    CaseRewriter,
    Auditor,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Writer, Role::Healer, Role::CaseRewriter, Role::Auditor];

    /// "writer", "healer", "case-rewriter", "auditor": the role argument and
    /// the file name.
    pub fn key(self) -> &'static str {
        match self {
            Role::Writer => "writer",
            Role::Healer => "healer",
            Role::CaseRewriter => "case-rewriter",
            Role::Auditor => "auditor",
        }
    }

    /// "writer", "healer", "case rewriter", "auditor": how a sentence names
    /// it.
    pub fn name(self) -> &'static str {
        match self {
            Role::Writer => "writer",
            Role::Healer => "healer",
            Role::CaseRewriter => "case rewriter",
            Role::Auditor => "auditor",
        }
    }

    /// What the role's agent file says it is for (its frontmatter
    /// description, which is what Claude Code shows and matches on).
    pub fn description(self) -> &'static str {
        match self {
            Role::Writer => "Choose which of a PBI's test cases to script and write their Auto Run scripts from the live app, under Test Case Manager's writer rules",
            Role::Healer => "Diagnose and repair failed Auto Run cases within the repair bounds, under Test Case Manager's healer rules",
            Role::CaseRewriter => "Change a saved Auto Run script or component, for a repair or a rewritten test case, under Test Case Manager's case rewriter rules",
            Role::Auditor => "Check saved Auto Run scripts against their test cases and report what they miss or fit, never editing, under Test Case Manager's auditor rules",
        }
    }

    /// Trimmed and lowercased, with "_" and " " read as "-". Also the
    /// Playwright agents' names: "planner" and "generator" -> Writer;
    /// "refactorer" and "spec-editor" -> CaseRewriter; "doc-drift-auditor"
    /// -> Auditor.
    pub fn from_key(key: &str) -> Option<Role> {
        let k = key.trim().to_lowercase().replace(['_', ' '], "-");
        match k.as_str() {
            "writer" | "planner" | "generator" => Some(Role::Writer),
            "healer" => Some(Role::Healer),
            "case-rewriter" | "refactorer" | "spec-editor" => Some(Role::CaseRewriter),
            "auditor" | "doc-drift-auditor" => Some(Role::Auditor),
            _ => None,
        }
    }

    /// The role's rules: its `roles/<key>.md` through include_str!, with
    /// "\r\n" read as "\n".
    pub fn text(self) -> String {
        let raw = match self {
            Role::Writer => include_str!("roles/writer.md"),
            Role::Healer => include_str!("roles/healer.md"),
            Role::CaseRewriter => include_str!("roles/case-rewriter.md"),
            Role::Auditor => include_str!("roles/auditor.md"),
        };
        raw.replace("\r\n", "\n")
    }
}

pub const ROLE_MAX_LINES: usize = 45;
pub const ROLE_MAX_CHARS: usize = 4000;

/// The line a later answer of the role's tool opens with.
pub fn pointer(role: Role) -> String {
    format!(
        "(The {n}'s rules hold for this work whatever tools you use. If they did not come with an earlier answer in this chat, get_guide with topic autorun and role {k} gives them.)",
        n = role.name(),
        k = role.key()
    )
}

/// Said when `role` names no role.
pub fn unknown_role(asked: &str) -> String {
    format!(
        "There is no role \"{}\": role is one of \"writer\", \"healer\", \"case-rewriter\" and \"auditor\".",
        asked.trim()
    )
}

/// A JSON value read as the structure it may be sent as: text holding a
/// list or an object is parsed, anything else stays as it is.
fn structured(v: &serde_json::Value) -> serde_json::Value {
    if let Some(s) = v.as_str() {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(s.trim()) {
            if parsed.is_array() || parsed.is_object() {
                return parsed;
            }
        }
    }
    v.clone()
}

/// A `case_id`: a number or a numeric string.
fn case_id_of(v: &serde_json::Value) -> Option<i32> {
    match &v["case_id"] {
        serde_json::Value::Number(n) => n.as_i64().and_then(|n| i32::try_from(n).ok()),
        serde_json::Value::String(s) => s.trim().parse::<i32>().ok(),
        _ => None,
    }
}

/// Walks a value for the `edits` it holds. `inherited` is the case id of
/// the script an entry sits in, taken by an entry without its own.
fn collect_repairs(v: &serde_json::Value, inherited: Option<i32>, out: &mut Vec<i32>) {
    match structured(v) {
        serde_json::Value::Array(items) => {
            for item in &items {
                collect_repairs(item, inherited, out);
            }
        }
        serde_json::Value::Object(map) => {
            let obj = serde_json::Value::Object(map);
            let own = case_id_of(&obj).or(inherited);
            if let Some(edits) = obj.get("edits") {
                match structured(edits) {
                    serde_json::Value::Array(entries) => {
                        for e in &entries {
                            if let Some(id) = case_id_of(e).or(own) {
                                out.push(id);
                            }
                        }
                    }
                    entry @ serde_json::Value::Object(_) => {
                        if let Some(id) = case_id_of(&entry).or(own) {
                            out.push(id);
                        }
                    }
                    _ => {}
                }
            }
            if let Some(scripts) = obj.get("scripts") {
                collect_repairs(scripts, own, out);
            }
        }
        _ => {}
    }
}

/// The case ids a `save_autorun_script` call's arguments declare repairs
/// of: every `case_id` in its `edits` (beside `scripts`; inside a
/// `{ scripts, edits }` bundle sent as `scripts`; or nested in a script,
/// where an entry without its own `case_id` takes the script's). `scripts`
/// and `edits` may each be JSON text, as the dispatch accepts them. A
/// `case_id` is a number or a numeric string. Sorted, each once.
pub fn cases_repaired_by(args: &serde_json::Value) -> Vec<i32> {
    let mut out = vec![];
    collect_repairs(args, None, &mut out);
    out.sort_unstable();
    out.dedup();
    out
}

/// Whether an argument is true, as a boolean or as the text "true".
fn is_true(v: &serde_json::Value) -> bool {
    v.as_bool() == Some(true) || v.as_str().map(|s| s.trim().eq_ignore_ascii_case("true")) == Some(true)
}

/// The role a call starts: `start_autorun_discovery` not a mapping run (its
/// `mapping` is not true or "true") -> Writer; `get_autorun_failures` ->
/// Healer; `save_autorun_script` with `cases_repaired_by` non-empty ->
/// CaseRewriter; `save_autorun_component` -> CaseRewriter; anything else
/// None. The auditor has no tool: `get_guide` gives it.
pub fn role_for_call(tool: &str, args: &serde_json::Value) -> Option<Role> {
    match tool {
        "start_autorun_discovery" if !is_true(&args["mapping"]) => Some(Role::Writer),
        "get_autorun_failures" => Some(Role::Healer),
        "save_autorun_script" if !cases_repaired_by(args).is_empty() => Some(Role::CaseRewriter),
        "save_autorun_component" => Some(Role::CaseRewriter),
        _ => None,
    }
}

/// The rules that apply at that moment, said at the END of every answer of the route, whatever its status.
pub const RULES_DISCOVERY: &str = "Rules now:\n- Every check's value is the case's own, as written: when the app or its data differs, call report_case_divergence and keep the case's value, never fit the check to the page.\n- Carry out each step live before you write it; never guess a locator, an address or a menu path.\n- A value the case does not hold needs a because, assumed or invented, saying where it comes from.";
pub const RULES_REPAIR_SAVE: &str = "Rules now:\n- A repair keeps every check and every value the case gives; a check is given up only by naming it in gives_up with why, and the person sees it.\n- Prove the repair with a replay of the whole case: the script is not checked until one passes.\n- Repeat every assumed and invented value and every check given up in your report.";
pub const RULES_FAILURES: &str = "Rules now:\n- Classify each failure first: a missing precondition or an app defect is reported with report_case_divergence, never healed.\n- At most 3 fix-run cycles per case; stop at cycle 2 when the failure is identical to cycle 1's.\n- A case Blocked with \"needs a ruling\" waits for the person: leave it alone.";
pub const RULES_REPLAY: &str = "Rules now:\n- Diagnose a failure on this page before anything changes: a missing precondition or an app defect is reported, never healed.\n- Change only the locator, the waiting or the navigation; never a check's value, and never the number of checks without gives_up.\n- The case counts as checked only after a replay of the whole case passes.";

/// `answer` with "\n\n" + `rules` after its text (the text's trailing whitespace trimmed first); the status unchanged.
pub fn with_rules(answer: (u16, String), rules: &str) -> (u16, String) {
    let (status, text) = answer;
    (status, format!("{}\n\n{}", text.trim_end(), rules))
}

/// `body` without a trailing "\n\n" + `rules`; unchanged when it does not end with them. For tests that pin an answer.
pub fn without_rules(body: &str, rules: &str) -> String {
    match body.strip_suffix(&format!("\n\n{rules}")) {
        Some(rest) => rest.to_string(),
        None => body.to_string(),
    }
}
