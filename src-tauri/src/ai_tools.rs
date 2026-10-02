//! AI-tool detection + registration, pure logic (testable with injected
//! paths). `commands/ai_tools.rs` supplies the real home/appdata dirs and
//! PATH probe, and does the actual file/process I/O.

use std::path::PathBuf;

pub const TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec {
        id: "claude-code",
        name: "Claude Code",
        path_cmd: Some("claude"),
        install_dir: Some(".claude"),
        install_appdata_dir: None,
        // claude-code registers via its own CLI, but detection still reads
        // ~/.claude.json to see whether our server is already there.
        config_path: |home, _appdata| PathBuf::from(home).join(".claude.json"),
        entry_key: "mcpServers",
        project_config: Some(|root| PathBuf::from(root).join(".mcp.json")),
    },
    ToolSpec {
        id: "claude-desktop",
        name: "Claude Desktop",
        path_cmd: None,
        install_dir: None,
        install_appdata_dir: Some("Claude"),
        config_path: |_home, appdata| {
            PathBuf::from(appdata).join("Claude").join("claude_desktop_config.json")
        },
        entry_key: "mcpServers",
        project_config: None,
    },
    ToolSpec {
        id: "vscode",
        name: "VS Code",
        path_cmd: Some("code"),
        install_dir: None,
        install_appdata_dir: None,
        config_path: |_home, appdata| {
            PathBuf::from(appdata).join("Code").join("User").join("mcp.json")
        },
        entry_key: "servers",
        project_config: Some(|root| PathBuf::from(root).join(".vscode").join("mcp.json")),
    },
    ToolSpec {
        id: "cursor",
        name: "Cursor",
        path_cmd: None,
        install_dir: Some(".cursor"),
        install_appdata_dir: None,
        config_path: |home, _appdata| PathBuf::from(home).join(".cursor").join("mcp.json"),
        entry_key: "mcpServers",
        project_config: Some(|root| PathBuf::from(root).join(".cursor").join("mcp.json")),
    },
    ToolSpec {
        id: "windsurf",
        name: "Windsurf",
        path_cmd: None,
        install_dir: Some(".codeium/windsurf"),
        install_appdata_dir: None,
        config_path: |home, _appdata| {
            PathBuf::from(home).join(".codeium").join("windsurf").join("mcp_config.json")
        },
        entry_key: "mcpServers",
        project_config: None,
    },
];

/// Our own MCP server's key in every tool's config. Assistants show a tool
/// as `<server>: <tool>`, so this is the prefix a person reads on every
/// call - short, and the same `tcm` the slash commands are grouped under.
pub const TCM_SERVER: &str = "tcm";

/// The key our server was registered under until it was renamed to
/// `TCM_SERVER`. Nothing writes it any more; it is named here so detection
/// still reports a registration an earlier version made (the AI Bridge tab
/// shows it as needing an update), and so registering `TCM_SERVER` can take
/// it out of the same config - one server under two names would offer
/// every tool twice. Also the name of the old single-file slash command
/// (`legacy_command_path`) and the prefix of the old permission rules
/// (`migrate_claude_permissions`).
pub const LEGACY_TCM_SERVER: &str = "tcm-testcases";

/// A separate database MCP server that earlier versions could register
/// beside ours. The app's own database tools replaced it and nothing
/// registers it any more; it is named here only so detection still reports
/// an entry an earlier version left behind, and the AI Bridge tab can
/// remove it (`commands::ai_tools::remove_legacy_db_server`).
pub const LEGACY_DB_SERVER: &str = "phr-db-mcp";

/// Every server this app manages, including the one it only ever removes
/// now. Anything else in a config is somebody else's and is never touched.
pub const MANAGED_SERVERS: &[&str] = &[TCM_SERVER, LEGACY_TCM_SERVER, LEGACY_DB_SERVER];

/// The names a registration of `server` replaces: registering it takes
/// these out of the same config and scope. Only our own server replaces
/// anything.
pub fn superseded_by(server: &str) -> &'static [&'static str] {
    if server == TCM_SERVER {
        &[LEGACY_TCM_SERVER]
    } else {
        &[]
    }
}

/// The Claude Code slash commands written alongside the MCP registration.
///
/// # Why commands at all
///
/// Without them the tools are only reachable by name: somebody has to know
/// `begin_test_case_writing` exists and type it out. One command per thing
/// a person reaches for by name puts that set in the picker, under a
/// `tcm:` prefix so they group together the way `sc:` does rather than
/// scattering through it. Every other tool is called by the assistant
/// when the guide says so.
///
/// # Why they are this thin
///
/// None of them restate the format, the workflow, the allowed modules or
/// the tag list. All of that comes from `get_writing_guide`, live and
/// per-project; a copy on disk would be stale the first time the guide
/// changed, and this codebase has already paid for one hand-copied list
/// that drifted silently. Each command says what its tool is for and gets
/// out of the way.
pub const COMMAND_MARKER: &str = "<!-- generated by Test Case Manager -->";

/// Tools an assistant can always call - the AI Bridge tab shows them
/// without a switch, and a saved disabled-list naming one is ignored.
pub const CORE_TOOLS: &[&str] = &[
    "begin_test_case_writing",
    "get_writing_guide",
    "get_test_cases",
    "check_spec_coverage",
    "transform_cases",
    // Finishing a draft is part of writing one. A set that cannot be
    // checked, ordered into a run sheet, or merged back from its slices is
    // a set nobody can ship - switching these off left an assistant able to
    // write cases and unable to hand over anything usable.
    "validate_cases",
    "optimize_cases",
    "merge_case_files",
];

/// The Auto Run tools. Offered only where Auto Run itself is: in a
/// development build, and in a release build whose optional extras are
/// unlocked (`autorun_offered`). Where they are offered they are listed,
/// switchable and callable like any other tool; where they are not, they
/// are not present at all - not listed, no switch, no skill file, and a
/// direct call is refused. (The name predates the unlock; the TS mirror
/// and its sync test read it, so it stays.)
///
/// The API template tools ride here too: a template runs in the same
/// signed-in browser Auto Run drives, so it is offered exactly where Auto
/// Run is. Proving and running one also need the person's own switch
/// (`BridgeContext::api_writes`), which the bridge checks.
pub const DEV_ONLY_TOOLS: &[&str] = &[
    "get_autorun_guide",
    "save_autorun_script",
    "get_autorun_page",
    "probe_autorun_locator",
    "try_autorun_action",
    "get_autorun_failures",
    "record_autorun_quirk",
    "retire_autorun_quirk",
    "mark_autorun_suspected_defect",
    "propose_accounts",
    "get_accounts",
    "get_api_template_guide",
    "list_api_templates",
    "prove_api_template",
    "run_api_template",
    "save_api_flow",
    "get_api_flow_progress",
    "record_app_quirk",
    "retire_app_quirk",
];

/// Whether this process is a development build: `cargo test` and
/// `tauri dev` compile with debug assertions on, `tauri build` does not.
/// The one place that reads `cfg!(debug_assertions)`, so every other spot
/// that needs the distinction takes a `dev: bool` instead and stays
/// testable for both values without a release build.
pub fn dev_build() -> bool {
    cfg!(debug_assertions)
}

/// Whether the Auto Run tools are offered: always in a development build,
/// and in a release build once this machine's optional extras are
/// unlocked. Both inputs explicit, so every combination is testable from
/// this (development) test binary.
pub fn autorun_offered_for(dev: bool, unlocked: bool) -> bool {
    dev || unlocked
}

/// `autorun_offered_for` for this process: its own build kind and the
/// unlock as the app last loaded or set it. Only meaningful in the app
/// process - the `--mcp` proxy learns the app's answer from `/tools`.
pub fn autorun_offered() -> bool {
    autorun_offered_for(dev_build(), crate::extras::unlocked())
}

/// The disabled set as it is actually applied: where the Auto Run tools
/// are not offered, they come first, always; where they are, they are
/// added only when `disabled` names them. Then whatever the frontend sent,
/// minus the core tools and duplicates, either way. One function, used by
/// tools/list, the call-time refusal and the skill writer, so no path can
/// disagree with another about what is off.
pub fn effective_disabled_for(disabled: &[String], offered: bool) -> Vec<String> {
    let mut out: Vec<String> = if offered {
        Vec::new()
    } else {
        DEV_ONLY_TOOLS.iter().map(|s| s.to_string()).collect()
    };
    for d in disabled {
        if !CORE_TOOLS.contains(&d.as_str()) && !out.iter().any(|o| o == d) {
            out.push(d.clone());
        }
    }
    out
}

/// `effective_disabled_for` for this process (`autorun_offered`).
pub fn effective_disabled(disabled: &[String]) -> Vec<String> {
    effective_disabled_for(disabled, autorun_offered())
}

/// One command per thing a person reaches for by name; every other tool
/// is called by the assistant when the guide says so. In the order they
/// are usually wanted rather than alphabetically -
/// `begin-test-case-writing` first because it is the one that should be
/// reached for first.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        stem: "begin-test-case-writing",
        tool: "begin_test_case_writing",
        desc: "Start a test-case writing job (asks what the file is called and what is in scope)",
        hint: "[PBI id, or what the cases should cover]",
        body: &[
            "Start a test-case writing job for: $ARGUMENTS",
            "",
            "Call `begin_test_case_writing` FIRST and put its questions to the developer -",
            "what the file is called (it goes in the repository's .test-cases folder),",
            "which specs are authoritative, what is out of scope. Those are theirs to",
            "answer, not yours to assume. The app watches that path, so what you write",
            "imports itself.",
            "",
            "Then call `get_writing_guide` and follow it. It is generated live from the org",
            "and project currently open, so it - not memory - is the instruction.",
            "",
            "For a large specification, fan the job out yourself: after intake, call",
            "`check_spec_coverage` with an empty draft (`json: \"[]\"`) and the plan's spec",
            "paths - every section comes back in `uncovered`, which IS the slice list. One",
            "subagent per slice, each with its own section range and output file",
            "(`<output-stem>-slice-<n>.json`), each calling `get_writing_guide` itself. Merge",
            "with `merge_case_files`, never by hand; then `check_spec_coverage` once on the",
            "merged file, then `optimize_cases` exactly once before handing over.",
        ],
    },
    CommandSpec {
        stem: "optimize",
        tool: "optimize_cases",
        desc: "Reorganise a draft into a run sheet a tester can work straight through",
        hint: "[path to the JSON, or paste it]",
        body: &[
            "Call `optimize_cases` on: $ARGUMENTS",
            "",
            "It spells navigation out as steps, trims expected results to the outcome, and",
            "orders the cases so the tester changes environment as few times as possible.",
            "Use `dry_run` first if the developer wants to see what would change; the report",
            "lists every rewrite rather than just counting them.",
        ],
    },
    CommandSpec {
        stem: "get-wiki-info",
        tool: "search_wiki",
        desc: "Search the project wiki for the spec behind a case",
        hint: "[search text]",
        body: &[
            "Call `search_wiki` for: $ARGUMENTS",
            "",
            "Returns page paths and snippets. Follow up with `/tcm:page` on a path to read",
            "the whole thing - cite what you find rather than paraphrasing from memory.",
        ],
    },
    CommandSpec {
        stem: "page",
        tool: "get_wiki_page",
        desc: "Read a wiki page in full, by path or URL",
        hint: "[wiki page path or URL]",
        body: &[
            "Call `get_wiki_page` for: $ARGUMENTS",
            "",
            "Use after `/tcm:get-wiki-info` has found the path. This is the text to quote in",
            "`reviewer_notes` - a short citation, not a summary.",
        ],
    },
    CommandSpec {
        stem: "heal",
        tool: "get_autorun_failures",
        desc: "Diagnose and repair failing Auto Run cases, one at a time",
        hint: "[case ids, or blank for every failed case in the newest run]",
        body: &[
            "Diagnose and repair failing Auto Run cases: $ARGUMENTS",
            "",
            "Call `get_autorun_guide` first and follow it; it holds every rule, and this is",
            "only the order to work in. Its \"Choosing a model for the work\" section says",
            "which part suits which model.",
            "",
            "1. Call `get_autorun_failures` for the case ids above, or for every failed case",
            "   in the newest run when none are given.",
            "2. A case whose failure is one of the `STOP:` lines is reported as it is",
            "   and not touched.",
            "3. Take one case at a time. Ask the person to bring the open Auto Run browser to",
            "   the failing step, then look with `get_autorun_page` and",
            "   `probe_autorun_locator`. Base the diagnosis on what the page shows, not on",
            "   what the failure text suggests.",
            "4. Name the cause as exactly one of: the locator no longer matches (renamed,",
            "   moved, duplicated); timing (the element or answer arrives later than the step",
            "   waits); data or a missing precondition; the environment (wrong site, wrong",
            "   account, session); or the application does not do what the case expects.",
            "5. For the first four, change only the locator, the waiting or the navigation.",
            "   Prove the changed action with `try_autorun_action`, then save it through",
            "   `save_autorun_script` with an `edits` entry, and a `quirk` when the cause is",
            "   about the application. Text that changes from run to run is matched on its",
            "   stable part with a non-exact name. Fix one step, then look again before the",
            "   next.",
            "6. For the fifth, do not touch the script. Call",
            "   `mark_autorun_suspected_defect` with the case, the step and a one-sentence",
            "   note of what the page did. The mark is refused unless that step failed in the",
            "   case's newest run, and refused when the failure is one of the `STOP:` lines.",
            "7. Never remove or weaken a check, never add a fixed wait, never go past the",
            "   repair cap. A refusal from the gate is final for that case.",
            "8. Finish with one line per case: the cause, then what was changed (steps),",
            "   marked (step and note), or why it was left alone. Then ask the person to",
            "   re-run those cases.",
        ],
    },
];

/// Static description of one AI tool: where its MCP config lives, how to
/// tell it's installed, and which JSON key holds its server map.
pub struct ToolSpec {
    pub id: &'static str,
    pub name: &'static str,
    /// Command name to probe on PATH (via `where`), if any.
    pub path_cmd: Option<&'static str>,
    /// Directory under `home` whose presence marks the tool installed
    /// (in addition to / instead of a PATH probe).
    pub install_dir: Option<&'static str>,
    /// Directory under `appdata` whose presence marks the tool installed
    /// (for tools with no `~`-rooted marker, e.g. Claude Desktop).
    pub install_appdata_dir: Option<&'static str>,
    /// Builds the absolute config-file path from `home` / `appdata`.
    pub config_path: fn(home: &str, appdata: &str) -> PathBuf,
    /// JSON key the server entry lives under (e.g. "mcpServers", "servers").
    pub entry_key: &'static str,
    /// Where a REPOSITORY's own copy of the config lives, for tools that
    /// read one - `None` for tools that only know a global config.
    pub project_config: Option<fn(root: &str) -> PathBuf>,
}

/// One MCP server as it appears in a tool's config file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct McpServer {
    /// The config key, e.g. "tcm".
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    /// Environment the tool must set when launching it. BTreeMap so the
    /// written config is byte-stable rather than reordering on every save.
    pub env: std::collections::BTreeMap<String, String>,
}

/// What the frontend needs to render one row of the AI-tools list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct DetectedTool {
    pub id: String,
    pub name: String,
    pub installed: bool,
    /// Which of `MANAGED_SERVERS` this tool's config currently carries.
    pub registered_servers: Vec<String>,
    /// "project" when this row reflects the working repository's config,
    /// "global" when the tool has none and the machine-wide config was read.
    pub scope: String,
    /// Our own servers still sitting in the tool's MACHINE-WIDE config while
    /// this row reads a repository's. A user-scope entry shadows the project
    /// one in most clients, so a leftover from before per-repo scoping is
    /// worth surfacing - the UI offers to retire it. Always empty for a
    /// "global" row, where it would just repeat `registered_servers`.
    pub global_registered_servers: Vec<String>,
}

/// Shared installed-check used by both `detect` (for every tool) and
/// `register_ai_tool` (to refuse registering a tool that isn't there):
/// PATH probe first, then the home/appdata marker directories.
pub fn is_installed(spec: &ToolSpec, home: &str, appdata: &str, on_path: &dyn Fn(&str) -> bool) -> bool {
    spec.path_cmd.is_some_and(on_path)
        || spec.install_dir.is_some_and(|dir| PathBuf::from(home).join(dir).is_dir())
        || spec
            .install_appdata_dir
            .is_some_and(|dir| PathBuf::from(appdata).join(dir).is_dir())
}

/// The config a registration for `spec` goes into, and which scope that
/// is. A repository wins for every tool that reads one; the two that do
/// not (Claude Desktop, Windsurf) stay global however the app is set.
pub fn config_for(
    spec: &ToolSpec,
    home: &str,
    appdata: &str,
    root: Option<&str>,
) -> (PathBuf, &'static str, &'static str) {
    match (root, spec.project_config) {
        (Some(r), Some(f)) => (f(r), spec.entry_key, "project"),
        _ => ((spec.config_path)(home, appdata), spec.entry_key, "global"),
    }
}

/// Detects installed/registered state for every known tool, given injected
/// base dirs and a PATH probe (so tests never touch the real filesystem).
pub fn detect(home: &str, appdata: &str, on_path: &dyn Fn(&str) -> bool) -> Vec<DetectedTool> {
    detect_in(home, appdata, on_path, None)
}

/// Detects installed/registered state for every known tool, reading each
/// tool's REPOSITORY config when `root` is given and the tool has one.
pub fn detect_in(
    home: &str,
    appdata: &str,
    on_path: &dyn Fn(&str) -> bool,
    root: Option<&str>,
) -> Vec<DetectedTool> {
    TOOL_SPECS
        .iter()
        .map(|spec| {
            let installed = is_installed(spec, home, appdata, on_path);
            let (config_path, key, scope) = config_for(spec, home, appdata, root);
            let registered_servers = managed_servers_in(&config_path, key);
            // Only for a repository row: on a global row this is the very
            // config already read above.
            let global_registered_servers = if scope == "project" {
                let (global_path, global_key, _) = config_for(spec, home, appdata, None);
                managed_servers_in(&global_path, global_key)
            } else {
                vec![]
            };
            DetectedTool {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                installed,
                registered_servers,
                scope: scope.to_string(),
                global_registered_servers,
            }
        })
        .collect()
}

/// The server map under `key` in the config at `path`, or None when the
/// file is missing, unparseable or has no such key.
fn server_entries(path: &std::path::Path, key: &str) -> Option<serde_json::Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get(key).cloned())
}

/// Which of `MANAGED_SERVERS` the config at `path` carries under `key`.
/// A missing or unparseable config reads as "none" - detection reports
/// state, it never repairs a file it could not understand.
fn managed_servers_in(path: &std::path::Path, key: &str) -> Vec<String> {
    let entries = server_entries(path, key);
    MANAGED_SERVERS
        .iter()
        .filter(|name| entries.as_ref().and_then(|e| e.get(**name)).is_some())
        .map(|name| name.to_string())
        .collect()
}

/// Whether the config at `path` carries `name` under `key` - read the same
/// way detection reads it.
pub fn config_carries(path: &std::path::Path, key: &str, name: &str) -> bool {
    server_entries(path, key).is_some_and(|e| e.get(name).is_some())
}

/// Our own server, as it should appear in a config.
pub fn tcm_server(exe: &str) -> McpServer {
    McpServer {
        name: TCM_SERVER.to_string(),
        command: exe.to_string(),
        args: vec!["--mcp".to_string()],
        env: Default::default(),
    }
}

/// One generated command.
pub struct CommandSpec {
    /// File stem, so `/tcm:<stem>`.
    pub stem: &'static str,
    /// The MCP tool this command exists to reach. Named so a tool switched
    /// off in the app can have its command taken out of the picker: a
    /// command that cannot work is worse than no command.
    pub tool: &'static str,
    /// What the picker shows.
    pub desc: &'static str,
    /// Shown after the name while typing; empty for the ones taking nothing.
    pub hint: &'static str,
    /// The prompt body. Joined with newlines; `$ARGUMENTS` is substituted by
    /// Claude Code.
    pub body: &'static [&'static str],
}

/// `~/.claude/commands/tcm/` - a directory, so the set groups as `tcm:*` in
/// the picker instead of scattering the entries through it.
pub fn command_dir(home: &str) -> PathBuf {
    PathBuf::from(home).join(".claude").join("commands").join("tcm")
}

/// `<repo>/.claude/commands/tcm/` - the same namespace, inside the
/// repository, so `/tcm:*` exists only where the app was pointed.
pub fn project_command_dir(root: &str) -> PathBuf {
    PathBuf::from(root).join(".claude").join("commands").join("tcm")
}

/// The single top-level file an earlier version wrote. Still named here so
/// registering can clear it: leaving it behind would put `/tcm-testcases`
/// in the picker next to the namespaced set, pointing at the same thing.
/// Named after the server's OLD key, which is what that version used - not
/// `TCM_SERVER`, whose `tcm.md` would be a different file.
pub fn legacy_command_path(home: &str) -> PathBuf {
    PathBuf::from(home)
        .join(".claude")
        .join("commands")
        .join(format!("{LEGACY_TCM_SERVER}.md"))
}

/// Remove `legacy_command_path(home)` when it is ours (carries
/// `COMMAND_MARKER`). A file of that name without the marker is somebody
/// else's and stays; a missing one is already the state wanted.
pub fn remove_legacy_command(home: &str) {
    let legacy = legacy_command_path(home);
    if matches!(std::fs::read_to_string(&legacy), Ok(t) if t.contains(COMMAND_MARKER)) {
        let _ = std::fs::remove_file(&legacy);
    }
}

/// Every command file to write: absolute path and full contents.
pub fn command_files(home: &str) -> Vec<(PathBuf, String)> {
    command_files_for(home, &[])
}

/// The same, minus any command whose tool is switched off in the app.
///
/// The tool side already refuses a disabled tool twice - it is filtered out
/// of `tools/list` and refused again on call, in case a client is working
/// from a list it cached. The picker was the one place that still offered
/// it, which is the place a person looks.
pub fn command_files_for(home: &str, disabled: &[String]) -> Vec<(PathBuf, String)> {
    command_files_in(&command_dir(home), disabled)
}

/// Every command file for `dir` - the global or the repository set.
pub fn command_files_in(dir: &std::path::Path, disabled: &[String]) -> Vec<(PathBuf, String)> {
    COMMANDS
        .iter()
        .filter(|c| !disabled.iter().any(|d| d == c.tool))
        .map(|c| (dir.join(format!("{}.md", c.stem)), command_markdown(c)))
        .collect()
}

/// Built from explicit lines, NOT from `\n\` string continuations: Rust
/// drops the leading whitespace of the line after a continuation, which
/// silently un-indents everything - fatal in YAML frontmatter, and the
/// description is what the picker shows.
/// Quote a value so it is always a valid YAML scalar.
///
/// Two descriptions read naturally with a colon in them - "Read the live
/// writing guide: format, allowed modules..." - and an unquoted colon makes
/// the frontmatter unparseable: YAML sees a second mapping key. Claude Code
/// happens to be lenient enough to show it anyway, which is exactly what
/// makes it worth fixing rather than leaving: nothing complains until
/// something stricter reads it. Quoting unconditionally means no future
/// description has to remember the rule.
fn yaml_scalar(v: &str) -> String {
    format!(
        "\"{}\"",
        v.replace('\\', "\\\\")
            .replace('\"', "\\\"")
    )
}

pub fn command_markdown(c: &CommandSpec) -> String {
    let mut lines: Vec<String> = vec![
        "---".into(),
        format!("name: {}", c.stem),
        format!("description: {}", yaml_scalar(c.desc)),
    ];
    if !c.hint.is_empty() {
        lines.push(format!("argument-hint: {}", yaml_scalar(c.hint)));
    }
    lines.push("---".into());
    lines.push(String::new());
    lines.push(COMMAND_MARKER.into());
    lines.push(String::new());
    for l in c.body {
        lines.push((*l).to_string());
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Where the Claude Code CLI actually lives, in the order to try.
///
/// Deliberately NOT a PATH lookup. The native installer drops
/// `claude.exe` in `~/.local/bin` and puts that directory on the PATH of
/// the sessions IT starts - it is written to neither `HKCU\Environment`
/// nor the system Path. This app is launched from Explorer and inherits
/// Explorer's environment, so `cmd /C claude ...` came back
/// "'claude' is not recognized" on machines where Claude Code works
/// perfectly well in a terminal or in VS Code. Detection still said
/// "installed", because `~/.claude` exists - so the tool looked ready and
/// registering it failed.
///
/// Measured on a machine with the native install: the exe is at
/// `~/.local/bin/claude.exe`, and that directory appears in neither
/// persisted PATH.
pub fn claude_cli_candidates(home: &str, appdata: &str) -> Vec<PathBuf> {
    let home = PathBuf::from(home);
    let appdata = PathBuf::from(appdata);
    vec![
        // Native installer (what the VS Code crowd tends to have).
        home.join(".local").join("bin").join("claude.exe"),
        home.join(".local").join("bin").join("claude.cmd"),
        // npm -g: %APPDATA%\npm is normally on the persisted PATH, so
        // these were the installs that always worked.
        appdata.join("npm").join("claude.cmd"),
        appdata.join("npm").join("claude.exe"),
    ]
}

/// Inserts (or replaces) `server`'s entry under `key` in `existing_json`,
/// preserving every other entry and top-level field. Errors on unparseable
/// JSON rather than clobbering it with a fresh file.
///
/// `type: "stdio"` is written for VS Code (the `servers` key), which is
/// what its schema and the company server's own docs expect; the other
/// tools infer it. `env` is omitted entirely when empty rather than
/// written as `{}`.
pub fn merge_entry(existing_json: &str, key: &str, server: &McpServer) -> Result<String, String> {
    let mut root: serde_json::Value = serde_json::from_str(existing_json)
        .map_err(|e| format!("existing config is not valid JSON: {e}"))?;
    if !root.is_object() {
        return Err("existing config is not a JSON object".to_string());
    }
    let root_obj = root.as_object_mut().unwrap();
    let entries = root_obj
        .entry(key.to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !entries.is_object() {
        return Err(format!("\"{key}\" is not a JSON object"));
    }
    let mut entry = serde_json::Map::new();
    if key == "servers" {
        entry.insert("type".into(), serde_json::json!("stdio"));
    }
    entry.insert("command".into(), serde_json::json!(server.command));
    entry.insert("args".into(), serde_json::json!(server.args));
    if !server.env.is_empty() {
        entry.insert("env".into(), serde_json::json!(server.env));
    }
    let entries = entries.as_object_mut().unwrap();
    entries.insert(server.name.clone(), serde_json::Value::Object(entry));
    // Registering under the current name retires the old one in the same
    // config: both would start the same server and offer every tool twice.
    for old in superseded_by(&server.name) {
        entries.remove(*old);
    }
    serde_json::to_string_pretty(&root).map_err(|e| format!("failed to serialize config: {e}"))
}

/// Removes the named server from the tool's config, preserving everything
/// else. `Ok(None)` = the entry wasn't there (nothing to write);
/// `Ok(Some(json))` = write this back. Errors on unparseable input - never
/// fabricate a config we couldn't read.
pub fn remove_entry(
    existing_json: &str,
    key: &str,
    server_name: &str,
) -> Result<Option<String>, String> {
    let mut root: serde_json::Value = serde_json::from_str(existing_json)
        .map_err(|e| format!("existing config is not valid JSON: {e}"))?;
    let Some(entries) = root.get_mut(key).and_then(|v| v.as_object_mut()) else {
        return Ok(None);
    };
    if entries.remove(server_name).is_none() {
        return Ok(None);
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|e| format!("failed to serialize config: {e}"))
}

/// The Claude Code permission rule that lets our `db_query` run without the
/// "Allow this tool?" prompt: one tool of one server, never a wildcard.
pub const CLAUDE_DB_QUERY_RULE: &str = "mcp__tcm__db_query";

/// Claude Code names a server's rules `mcp__<server>__<tool>`, and the
/// server alone `mcp__<server>`.
const CLAUDE_RULE_SERVER: &str = "mcp__tcm";
const LEGACY_CLAUDE_RULE_SERVER: &str = "mcp__tcm-testcases";

/// `rule` under the server's current name when it is one of the OLD name's
/// rules - the whole server, or one of its tools - else None. A server
/// whose name merely starts with the old one (`mcp__tcm-testcases-x__...`)
/// is somebody else's.
fn renamed_claude_rule(rule: &str) -> Option<String> {
    if rule == LEGACY_CLAUDE_RULE_SERVER {
        return Some(CLAUDE_RULE_SERVER.to_string());
    }
    rule.strip_prefix(LEGACY_CLAUDE_RULE_SERVER)
        .and_then(|rest| rest.strip_prefix("__"))
        .map(|tool| format!("{CLAUDE_RULE_SERVER}__{tool}"))
}

/// Rename the old name's rules in one permission list, in place: each takes
/// the old one's position, and one whose new form the list already holds
/// is dropped instead of written twice. `only` limits it to the rule that
/// renames to that. Every other entry stays exactly where it was. Answers
/// whether anything changed.
fn rename_claude_rules(list: &mut Vec<serde_json::Value>, only: Option<&str>) -> bool {
    let original = std::mem::take(list);
    let mut changed = false;
    for v in &original {
        let renamed = v
            .as_str()
            .and_then(renamed_claude_rule)
            .filter(|new| only.is_none_or(|o| o == new));
        let Some(new) = renamed else {
            list.push(v.clone());
            continue;
        };
        changed = true;
        let held = |l: &[serde_json::Value]| l.iter().any(|x| x.as_str() == Some(new.as_str()));
        if !held(&original) && !held(list) {
            list.push(serde_json::Value::String(new));
        }
    }
    changed
}

/// A Claude Code settings file's text with every permission rule for the
/// server's OLD name (`mcp__tcm-testcases`, `mcp__tcm-testcases__<tool>`)
/// carried over to the current one, in `permissions.allow`, `deny` and
/// `ask` - so an "always allow" given before the rename still holds after
/// it. Same position, never a duplicate, nothing else touched. `None` when
/// there is nothing to carry over (including an empty file), so an
/// unchanged file is never rewritten. A file that is not a JSON object is
/// refused, as `set_claude_allow` refuses it.
pub fn migrate_claude_permissions(settings: &str) -> Result<Option<String>, String> {
    if settings.trim().is_empty() {
        return Ok(None);
    }
    let mut root: serde_json::Value =
        serde_json::from_str(settings).map_err(|e| format!("could not read the settings file: {e}"))?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "the settings file is not a JSON object".to_string())?;
    let Some(perms) = obj.get_mut("permissions").and_then(|p| p.as_object_mut()) else {
        return Ok(None);
    };
    let mut changed = false;
    for key in ["allow", "deny", "ask"] {
        if let Some(list) = perms.get_mut(key).and_then(|l| l.as_array_mut()) {
            changed |= rename_claude_rules(list, None);
        }
    }
    if !changed {
        return Ok(None);
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|e| format!("failed to serialize settings: {e}"))
}

/// A Claude Code settings file's text with `rule` in `permissions.allow`
/// (`on`) or out of it, and nothing else touched - every other rule, key
/// and value is somebody else's. `None` when the file already says what was
/// asked, so an unchanged file is never rewritten.
///
/// The same rule under the server's old name is the same setting: it
/// counts as `rule` being there, and is rewritten to `rule` where it stood
/// (or taken out with it).
///
/// An empty or missing file reads as `{}`. A file that is not a JSON object
/// is refused rather than replaced: it is the person's own settings, and
/// overwriting what the app cannot read would lose it. Taking the rule out
/// also takes out an `allow` list, and then a `permissions` object, that it
/// leaves empty - so switching on and off again leaves the file as it was.
pub fn set_claude_allow(settings: &str, rule: &str, on: bool) -> Result<Option<String>, String> {
    let text = if settings.trim().is_empty() { "{}" } else { settings };
    let mut root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("could not read the settings file: {e}"))?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "the settings file is not a JSON object".to_string())?;

    let renamed = obj
        .get_mut("permissions")
        .and_then(|p| p.get_mut("allow"))
        .and_then(|a| a.as_array_mut())
        .is_some_and(|a| rename_claude_rules(a, Some(rule)));

    let has = obj
        .get("permissions")
        .and_then(|p| p.get("allow"))
        .and_then(|a| a.as_array())
        .is_some_and(|a| a.iter().any(|r| r == rule));
    if has == on && !renamed {
        return Ok(None);
    }

    // `has == on` here means only the rename is left to write.
    if has != on {
        if on {
            let perms = obj
                .entry("permissions")
                .or_insert_with(|| serde_json::json!({}))
                .as_object_mut()
                .ok_or_else(|| "\"permissions\" in the settings file is not an object".to_string())?;
            let allow = perms
                .entry("allow")
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut()
                .ok_or_else(|| "\"permissions.allow\" in the settings file is not a list".to_string())?;
            allow.push(serde_json::Value::String(rule.to_string()));
        } else if let Some(perms) = obj.get_mut("permissions").and_then(|p| p.as_object_mut()) {
            if let Some(allow) = perms.get_mut("allow").and_then(|a| a.as_array_mut()) {
                allow.retain(|r| r != rule);
                if allow.is_empty() {
                    perms.remove("allow");
                }
            }
            if perms.is_empty() {
                obj.remove("permissions");
            }
        }
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|e| format!("failed to serialize settings: {e}"))
}

/// Cursor's allowlist entry that lets our `db_query` run without asking:
/// `server:tool`, one tool of one server.
pub const CURSOR_DB_QUERY_ENTRY: &str = "tcm:db_query";

/// `entry` as it read under the server's old name, when it is one of ours.
fn legacy_cursor_entry(entry: &str) -> Option<String> {
    entry
        .strip_prefix(&format!("{TCM_SERVER}:"))
        .map(|tool| format!("{LEGACY_TCM_SERVER}:{tool}"))
}

/// A Cursor `permissions.json`'s text with every `mcpAllowlist` entry for
/// the server's OLD name (`tcm-testcases:<tool>`) carried over to the
/// current one (`tcm:<tool>`) - the Cursor half of
/// `migrate_claude_permissions`. Cursor compares entries case-insensitively,
/// so this does too: the prefix matches in any case, and an entry whose new
/// form the list already holds in any case is dropped rather than written
/// twice. Same position, nothing else touched, `None` when there is
/// nothing to carry over; a file that is not a JSON object is refused.
pub fn migrate_cursor_permissions(permissions: &str) -> Result<Option<String>, String> {
    if permissions.trim().is_empty() {
        return Ok(None);
    }
    let mut root: serde_json::Value =
        serde_json::from_str(permissions).map_err(|e| format!("could not read the permissions file: {e}"))?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "the permissions file is not a JSON object".to_string())?;
    let Some(list) = obj.get_mut("mcpAllowlist").and_then(|a| a.as_array_mut()) else {
        return Ok(None);
    };
    let old_prefix = format!("{LEGACY_TCM_SERVER}:");
    let renamed = |v: &serde_json::Value| -> Option<String> {
        let s = v.as_str()?;
        let head = s.get(..old_prefix.len())?;
        head.eq_ignore_ascii_case(&old_prefix)
            .then(|| format!("{TCM_SERVER}:{}", &s[old_prefix.len()..]))
    };
    let held = |l: &[serde_json::Value], e: &str| {
        l.iter().any(|x| x.as_str().is_some_and(|x| x.eq_ignore_ascii_case(e)))
    };
    let original = std::mem::take(list);
    let mut changed = false;
    for v in &original {
        let Some(new) = renamed(v) else {
            list.push(v.clone());
            continue;
        };
        changed = true;
        if !held(&original, &new) && !held(list, &new) {
            list.push(serde_json::Value::String(new));
        }
    }
    if !changed {
        return Ok(None);
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|e| format!("failed to serialize permissions: {e}"))
}

/// A Cursor `permissions.json`'s text with `entry` in `mcpAllowlist` (`on`)
/// or out of it, nothing else touched. `None` when nothing would change.
/// Same rules as `set_claude_allow`: an unreadable file is refused, never
/// replaced, and a list the entry leaves empty goes with it - and the same
/// entry under the server's old name counts as this one and is rewritten.
pub fn set_cursor_allow(permissions: &str, entry: &str, on: bool) -> Result<Option<String>, String> {
    let text = if permissions.trim().is_empty() { "{}" } else { permissions };
    let mut root: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("could not read the permissions file: {e}"))?;
    let obj = root
        .as_object_mut()
        .ok_or_else(|| "the permissions file is not a JSON object".to_string())?;
    let mut renamed = false;
    if let (Some(old), Some(list)) =
        (legacy_cursor_entry(entry), obj.get_mut("mcpAllowlist").and_then(|a| a.as_array_mut()))
    {
        let matches = |v: &serde_json::Value, e: &str| v.as_str().is_some_and(|r| r.eq_ignore_ascii_case(e));
        let original = std::mem::take(list);
        let already = original.iter().any(|v| matches(v, entry));
        for v in &original {
            if !matches(v, &old) {
                list.push(v.clone());
                continue;
            }
            renamed = true;
            if !already && !list.iter().any(|x| matches(x, entry)) {
                list.push(serde_json::Value::String(entry.to_string()));
            }
        }
    }
    let has = obj
        .get("mcpAllowlist")
        .and_then(|a| a.as_array())
        .is_some_and(|a| a.iter().any(|r| r.as_str().is_some_and(|r| r.eq_ignore_ascii_case(entry))));
    if has == on && !renamed {
        return Ok(None);
    }
    // `has == on` here means only the rename is left to write.
    if has != on {
        if on {
            obj.entry("mcpAllowlist")
                .or_insert_with(|| serde_json::json!([]))
                .as_array_mut()
                .ok_or_else(|| "\"mcpAllowlist\" in the permissions file is not a list".to_string())?
                .push(serde_json::Value::String(entry.to_string()));
        } else if let Some(list) = obj.get_mut("mcpAllowlist").and_then(|a| a.as_array_mut()) {
            list.retain(|r| !r.as_str().is_some_and(|r| r.eq_ignore_ascii_case(entry)));
            if list.is_empty() {
                obj.remove("mcpAllowlist");
            }
        }
    }
    serde_json::to_string_pretty(&root)
        .map(Some)
        .map_err(|e| format!("failed to serialize permissions: {e}"))
}

/// Writes `contents` to `path` atomically: write to a sibling temp file in
/// the same directory (so the final `rename` stays on one volume - atomic
/// on NTFS), then rename it over `path`. Cleans up the temp file if the
/// rename fails, so a crash mid-write never leaves a half-written config.
/// A rename blocked by another process holding the file (a sharing
/// violation on Windows) is retried with backoff first - see
/// `atomic_write_with`.
pub fn atomic_write(path: &std::path::Path, contents: &str) -> Result<(), String> {
    atomic_write_with(path, contents, |p, s| std::fs::write(p, s), |a, b| std::fs::rename(a, b), std::thread::sleep)
}

/// How long to wait before each retry of a rename another process blocked.
pub const RENAME_RETRY_WAITS_MS: [u64; 5] = [50, 100, 200, 400, 800];

/// A rename Windows refused because another process has the target open -
/// antivirus scanning the file just written, an editor, the search indexer.
/// Usually gone within a moment, so worth waiting out.
fn held_by_another_process(e: &std::io::Error) -> bool {
    // 32 = ERROR_SHARING_VIOLATION, 5 = ERROR_ACCESS_DENIED.
    e.kind() == std::io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32) | Some(5))
}

/// `atomic_write` with the file operations and the wait passed in - the
/// seam tests/suite/ai_tools.rs drives. A rename that fails because another
/// process holds the file is retried after each of `RENAME_RETRY_WAITS_MS`;
/// any other failure, or the last retry's, removes the temp file and
/// returns the error. So does a temp write that fails part-way.
pub fn atomic_write_with(
    path: &std::path::Path,
    contents: &str,
    write: impl FnOnce(&std::path::Path, &str) -> std::io::Result<()>,
    mut rename: impl FnMut(&std::path::Path, &std::path::Path) -> std::io::Result<()>,
    mut sleep: impl FnMut(std::time::Duration),
) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?
        .to_string_lossy();
    let tmp_path = parent.join(format!("{file_name}.tcm-tmp-{}", std::process::id()));

    if let Err(e) = write(&tmp_path, contents) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to write temp file {}: {e}", tmp_path.display()));
    }

    let mut result = rename(&tmp_path, path);
    for wait in RENAME_RETRY_WAITS_MS {
        match &result {
            Err(e) if held_by_another_process(e) => {
                sleep(std::time::Duration::from_millis(wait));
                result = rename(&tmp_path, path);
            }
            _ => break,
        }
    }
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to move temp file into place at {}: {e}", path.display()));
    }
    Ok(())
}
