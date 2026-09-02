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
        // ~/.claude.json to see whether tcm-testcases is already there.
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

/// Turn whatever the person picked for the database server into a real
/// invocation, or refuse. Only a launchable command may reach a config:
/// 1.19.9/1.19.10 let "any existing path" through, and picking the repo
/// FOLDER registered a directory as `command` - which no MCP client can
/// spawn, so the server never started and every tool call failed silently
/// inside the client.
///
/// - `.exe`/`.cmd`/`.bat`: run directly.
/// - `.dll`: a published framework-dependent build - runs via `dotnet`.
/// - a folder: the newest built `.exe` beneath it (`obj\` intermediates,
///   test hosts, `node_modules` and dot-dirs excluded); an unbuilt folder
///   is refused with the `dotnet build` instruction rather than
///   registering something dead.
/// - anything else (`.csproj`, `.cs`, ...): refused with guidance.
pub fn resolve_db_command(picked: &std::path::Path) -> Result<(String, Vec<String>), String> {
    if !picked.exists() {
        return Err(format!("{} does not exist", picked.display()));
    }
    let as_string = |p: &std::path::Path| p.to_string_lossy().to_string();
    if picked.is_file() {
        let ext = picked
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        return match ext.as_str() {
            "exe" | "cmd" | "bat" => Ok((as_string(picked), vec![])),
            "dll" => Ok(("dotnet".to_string(), vec![as_string(picked)])),
            _ => Err(format!(
                "{} is not something an MCP client can launch - pick the built server \
                 executable (.exe), a published .dll, or the project folder",
                picked.display()
            )),
        };
    }
    // A folder: find the built server executable beneath it.
    let mut found: Vec<PathBuf> = vec![];
    let mut stack = vec![picked.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if p.is_dir() {
                // Never intermediate build state, never a test host.
                if name == "obj"
                    || name == "ref"
                    || name.contains("test")
                    || name == "node_modules"
                    || name.starts_with('.')
                {
                    continue;
                }
                stack.push(p);
            } else if name.ends_with(".exe") {
                found.push(p);
            }
        }
    }
    // Newest build wins - the one `dotnet build` just produced.
    found.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
    match found.pop() {
        Some(exe) => Ok((as_string(&exe), vec![])),
        None => Err(format!(
            "no built executable found under {} - run `dotnet build` there first, then \
             register again (the build lands in src\\...\\bin\\...\\*.exe)",
            picked.display()
        )),
    }
}

/// Our own MCP server's key in every tool's config.
pub const TCM_SERVER: &str = "tcm-testcases";

/// The company's SQL Server schema MCP server, registered alongside ours
/// so an assistant can read the database and the test cases in one place.
pub const DB_SERVER: &str = "phr-db-mcp";

/// Every server this app manages. Anything else in a config is somebody
/// else's and is never touched.
pub const MANAGED_SERVERS: &[&str] = &[TCM_SERVER, DB_SERVER];

/// The Claude Code slash commands written alongside the MCP registration.
///
/// # Why commands at all
///
/// Without them the tools are only reachable by name: somebody has to know
/// `begin_test_case_writing` exists and type it out. One command per tool
/// puts the whole set in the picker, under a `tcm:` prefix so they group
/// together the way `sc:` does rather than scattering through it.
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

/// One per tool the MCP server exposes, in the order they are usually
/// wanted rather than alphabetically - `write` first because it is the one
/// that should be reached for first.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        stem: "write",
        tool: "begin_test_case_writing",
        desc: "Start a test-case writing job (asks where the file goes and what is in scope)",
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
        ],
    },
    CommandSpec {
        stem: "fanout",
        tool: "merge_case_files",
        desc: "Split a large spec across slice-writing subagents and merge the result",
        hint: "[plan path, and the section ranges to split across]",
        body: &[
            "Fan a large writing job out across subagents, for: $ARGUMENTS",
            "",
            "Finish intake with `begin_test_case_writing` FIRST - dispatch never starts before",
            "the plan is approved. Get the slice list by calling `check_spec_coverage` with",
            "`json: \"[]\"` (an empty draft) and the plan's spec_paths - every section comes",
            "back in `uncovered`, which IS the slice list, parsed from the document itself.",
            "Never invent section ranges yourself.",
            "",
            "One output file per slice, named `<output-stem>-slice-<n>.json`. Dispatch one",
            "subagent per slice and hand each one: the plan's path, its own explicit section",
            "range, its own output path, an instruction to call `get_writing_guide` itself",
            "(subagents reach these tools directly), and 2-3 exemplar cases from",
            "`get_test_cases`.",
            "",
            "When every slice is written, merge them with `merge_case_files` - never by hand.",
            "Run `check_spec_coverage` on the merged file and account for every `uncovered`",
            "entry out loud. Then run `optimize_cases` exactly once, on the merged file, never",
            "per slice, before handing it over.",
        ],
    },
    CommandSpec {
        stem: "guide",
        tool: "get_writing_guide",
        desc: "Read the live writing guide: format, allowed modules, tags, workflow",
        hint: "",
        body: &[
            "Call `get_writing_guide` and follow what it returns.",
            "",
            "It is generated live from the org and project the developer has open - the",
            "allowed Module values, the tags already in use, the format and the order to call",
            "things in. It changes per project, so do not work from memory and do not copy",
            "its rules anywhere.",
        ],
    },
    CommandSpec {
        stem: "examples",
        tool: "get_test_cases",
        desc: "Show the test cases already on a PBI, in import JSON shape",
        hint: "[PBI id]",
        body: &[
            "Call `get_test_cases` for: $ARGUMENTS",
            "",
            "Use them to copy the house style and granularity before drafting anything new.",
            "`titles_only` is the cheap way to check for duplicates across a large PBI.",
        ],
    },
    CommandSpec {
        stem: "failures",
        tool: "get_run_failures",
        desc: "Show what failed in a PBI's latest runs, with the tester's comments",
        hint: "[PBI id]",
        body: &[
            "Call `get_run_failures` for: $ARGUMENTS",
            "",
            "Each failure carries the tester's comment and any linked bugs - that is what",
            "actually broke, in their words. To write regression cases from it, read the",
            "failed case itself with `get_test_cases` first, then extend the coverage",
            "instead of restating the case that already failed.",
        ],
    },
    CommandSpec {
        stem: "autorun",
        tool: "get_autorun_guide",
        desc: "Read how to write an Auto Run browser script for a test case",
        hint: "",
        body: &[
            "Call `get_autorun_guide` and follow it.",
            "",
            "The one rule worth repeating before you start: read the application's source",
            "for SELECTORS - the real id of a control beats a guess - but take every",
            "assertion from the test case's own expected result. An assertion copied from",
            "the implementation passes through the bug it was meant to catch.",
        ],
    },
    CommandSpec {
        stem: "script",
        tool: "save_autorun_script",
        desc: "Write and save Auto Run scripts for a PBI's test cases",
        hint: "[PBI id, or which cases to script]",
        body: &[
            "Write Auto Run action scripts for: $ARGUMENTS",
            "",
            "Call `get_autorun_guide` first if you have not this session, then",
            "`get_test_cases` for the cases themselves. Read the application's source for",
            "the selectors. Save everything in ONE `save_autorun_script` call - it takes a",
            "list, and a whole PBI in one batch is easier to review than a case at a time.",
            "",
            "Say which steps you could not automate rather than inventing a check that only",
            "looks like coverage. Someone watches every run; an honest gap costs them",
            "nothing, a false green costs them the bug.",
        ],
    },
    CommandSpec {
        stem: "validate",
        tool: "validate_cases",
        desc: "Check a draft with the app's real importer",
        hint: "[path to the JSON, or paste it]",
        body: &[
            "Call `validate_cases` on: $ARGUMENTS",
            "",
            "It runs Test Case Manager's REAL importer, so a pass means the app will read the",
            "file the way you expect. Pass a `path` for anything large rather than inlining",
            "it. Fix every warning and validate again - do not explain a warning away.",
            "`advisories` are different: judgement calls, not defects. Read each one, decide,",
            "and tell the developer what you decided and why.",
        ],
    },
    CommandSpec {
        stem: "coverage",
        tool: "check_spec_coverage",
        desc: "Which parts of the spec have no case yet - run before optimize_cases",
        hint: "[spec file path(s), and the draft]",
        body: &[
            "Call `check_spec_coverage` with the draft and the plan's spec paths: $ARGUMENTS",
            "",
            "It reports findings to read and account for, not a pass/fail - a partial draft is",
            "a normal state, not an error. Report `uncovered` to the developer and account for",
            "every entry before handing the file over: \"out of scope for this batch\" is a fine",
            "answer, silence is not.",
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
        stem: "transform",
        tool: "transform_cases",
        desc: "Bulk-edit a draft: retag, retitle, set module, add or remove steps",
        hint: "[what to change]",
        body: &[
            "Call `transform_cases` to apply: $ARGUMENTS",
            "",
            "Use this rather than rewriting the file yourself - it is declarative, it reports",
            "what it touched, and it will not quietly reshape the cases around your edit.",
        ],
    },
    CommandSpec {
        stem: "tags",
        tool: "get_tags",
        desc: "List the tags this project already uses",
        hint: "[optional search text]",
        body: &[
            "Call `get_tags` with: $ARGUMENTS",
            "",
            "Prefer an existing tag over inventing a near-duplicate - a new \"smoke-test\"",
            "next to an established \"smoke\" fragments the project's tags for everyone.",
        ],
    },
    CommandSpec {
        stem: "pbis",
        tool: "search_pbis",
        desc: "Find a PBI by title text",
        hint: "[search text]",
        body: &["Call `search_pbis` for: $ARGUMENTS", "", "Returns matching work item ids and titles in the current project."],
    },
    CommandSpec {
        stem: "wiki",
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
        desc: "Read a wiki page in full, by path",
        hint: "[wiki page path]",
        body: &[
            "Call `get_wiki_page` for: $ARGUMENTS",
            "",
            "Use after `/tcm:wiki` has found the path. This is the text to quote in",
            "`reviewer_notes` - a short citation, not a summary.",
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
    /// The config key, e.g. "tcm-testcases".
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
            let entries = std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v.get(key).cloned());
            let registered_servers = MANAGED_SERVERS
                .iter()
                .filter(|name| entries.as_ref().and_then(|e| e.get(**name)).is_some())
                .map(|name| name.to_string())
                .collect();
            DetectedTool {
                id: spec.id.to_string(),
                name: spec.name.to_string(),
                installed,
                registered_servers,
                scope: scope.to_string(),
            }
        })
        .collect()
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
/// the picker instead of scattering ten entries through it.
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
pub fn legacy_command_path(home: &str) -> PathBuf {
    PathBuf::from(home)
        .join(".claude")
        .join("commands")
        .join(format!("{TCM_SERVER}.md"))
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
    entries
        .as_object_mut()
        .unwrap()
        .insert(server.name.clone(), serde_json::Value::Object(entry));
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

/// Writes `contents` to `path` atomically: write to a sibling temp file in
/// the same directory (so the final `rename` stays on one volume - atomic
/// on NTFS), then rename it over `path`. Cleans up the temp file if the
/// rename fails, so a crash mid-write never leaves a half-written config.
pub fn atomic_write(path: &std::path::Path, contents: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("{} has no file name", path.display()))?
        .to_string_lossy();
    let tmp_path = parent.join(format!("{file_name}.tcm-tmp-{}", std::process::id()));

    std::fs::write(&tmp_path, contents)
        .map_err(|e| format!("failed to write temp file {}: {e}", tmp_path.display()))?;

    if let Err(e) = std::fs::rename(&tmp_path, path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to move temp file into place at {}: {e}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod atomic_write_tests {
    use super::atomic_write;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let dir = std::env::temp_dir().join(format!("tcm-ai-tools-atomic-write-{nanos}-{n}"));
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn atomic_write_creates_new_file() {
        let dir = TempDir::new();
        let target = dir.path().join("config.json");
        atomic_write(&target, "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = TempDir::new();
        let target = dir.path().join("config.json");
        std::fs::write(&target, "old-contents").unwrap();
        atomic_write(&target, "new-contents").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new-contents");
    }

    #[test]
    fn atomic_write_leaves_no_stray_temp_file() {
        let dir = TempDir::new();
        let target = dir.path().join("config.json");
        atomic_write(&target, "contents").unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.contains("tcm-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "expected no leftover temp files, found: {leftovers:?}");
    }
}
