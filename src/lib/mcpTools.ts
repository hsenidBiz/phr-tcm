// Which of our MCP tools an assistant is allowed to call.
//
// Stored as the DISABLED set, not the enabled one: a new tool added in a
// later release is then available by default rather than silently missing
// because it wasn't in someone's saved list.
//
// Each environment keeps its own set (activeEnv.ts); everything here reads
// and writes the ACTIVE environment's.

import { BRIDGE_SWITCH_KEYS, envScopedKey, subscribeActiveEnv } from "./activeEnv";
import { modulesSnapshot, realModulesSnapshot, subscribeModules, type Module } from "./modules";
import { isCaptureMode } from "../dev/capture";

/** The active environment's stored set. */
const storageKey = (): string => envScopedKey(BRIDGE_SWITCH_KEYS.tools);

export type McpToolInfo = { name: string; label: string; summary: string };

/** Mirrors `mcp.rs`'s tools_list - kept here so the settings UI can show
 * what each tool does without a round trip. `tcm_mcp.rs` asserts the same
 * names on the Rust side, so a drift shows up as a failing test. */
export const MCP_TOOLS: McpToolInfo[] = [
  {
    name: "begin_test_case_writing",
    label: "Start a writing job",
    summary: "Asks you how the set should be written, before anything is.",
  },
  { name: "get_guide", label: "Guides", summary: "The live guide for the job at hand: format rules and your org's allowed Module values for writing cases." },
  { name: "get_test_cases", label: "Cases by PBI or id", summary: "The cases on a PBI, or specific cases by their ids - style, and what is covered." },
  { name: "search_test_suites", label: "Find a test suite", summary: "The plans and suites in this project, by plan name, suite name or PBI id." },
  { name: "get_suite_test_cases", label: "Cases in a test suite", summary: "The cases in one suite, in the suite's own order." },
  {
    name: "get_run_results",
    label: "Run results",
    summary: "A PBI's latest run results - failed, blocked, passed or any other outcome - with the tester's comments, and a count of each.",
  },
  { name: "check_spec_coverage", label: "Specification coverage", summary: "Which spec sections have no case yet - findings to account for, not errors." },
  { name: "merge_case_files", label: "Merge slice files", summary: "Merge fan-out slice files into one draft through the real importer." },
  {
    name: "save_autorun_script",
    label: "Save Auto Run scripts",
    summary: "Save browser scripts for a PBI's cases - one call covers the whole set.",
  },
  {
    name: "get_autorun_script",
    label: "Read a script",
    summary: "Read one case's saved browser script, whole.",
  },
  {
    name: "delete_autorun_script",
    label: "Delete a script",
    summary: "Delete one case's saved script. It is kept aside on this machine for 30 days, then purged.",
  },
  {
    name: "get_autorun_page",
    label: "See the open page",
    summary: "Read the page in the browser you opened, one element per line with its locator.",
  },
  {
    name: "probe_autorun_locator",
    label: "Try a locator",
    summary: "What a locator matches on the open page right now, and whether it is visible - or one element's styles, listeners, animations and structure, read without touching the page.",
  },
  {
    name: "try_autorun_action",
    label: "Try an action",
    summary: "Carry out one script action against the open page, without recording anything.",
  },
  {
    name: "replay_autorun_to_step",
    label: "Replay to a step",
    summary: "Replay a case's saved steps up to the failing one, asking you first when its script must not save - or stop the assistant's replay that is still going.",
  },
  {
    name: "start_autorun_discovery",
    label: "Start exploring",
    summary: "Open the Auto Run browser and sign in as a saved account, so the assistant can explore the live app.",
  },
  {
    name: "discover_autorun_actions",
    label: "Explore",
    summary: "Carry out one action, or up to 20 in order, while exploring, and see what each did and the page they led to.",
  },
  {
    name: "save_autorun_area",
    label: "Save a found area",
    summary: "Save a screen found through the menus as an area, once its clicks are replayed and arrive.",
  },
  {
    name: "end_autorun_discovery",
    label: "Stop exploring",
    summary: "Close the browser the assistant was exploring in.",
  },
  {
    name: "reset_autorun_browser",
    label: "Reset the Auto Run browser",
    summary: "Give the assistant's Auto Run browser a clean page - other tabs closed, back at the home page - without signing out, or let go of a broken one.",
  },
  {
    name: "save_autorun_component",
    label: "Save a component",
    summary: "Save a widget or short flow, once tried live, as a component any script can use - or remove one no saved script uses.",
  },
  {
    name: "get_autorun_network_wait",
    label: "Network wait",
    summary: "Whether an unattended run is paused because the company network is unreachable, and when it gives up.",
  },
  {
    name: "search_autorun_requests",
    label: "Captured requests",
    summary: "The requests the page in the Auto Run browser made, newest first, with secrets hidden - all, or found by path, method, status, type or body text.",
  },
  {
    name: "get_autorun_request",
    label: "A captured request",
    summary: "One captured request's headers and body, or its response's status, headers, timing and body, with secrets hidden.",
  },
  {
    name: "autorun_capture",
    label: "The capture filter",
    summary: "Which requests the Auto Run browser's capture keeps from now on, how many it keeps, and the imported sets.",
  },
  {
    name: "autorun_har",
    label: "HAR files",
    summary: "Save the capture, secrets hidden, as a HAR file in the app's own captures folder, or load one from there as a read-only set.",
  },
  {
    name: "set_autorun_headers",
    label: "Set test headers",
    summary: "Extra request headers for the assistant's Auto Run browser, to test an edge case; named in every answer until reset.",
  },
  {
    name: "autorun_cookies",
    label: "Cookies",
    summary: "The Auto Run browser's cookies for the application's own site (the one TCM's sign-in confirmed), with the sign-in's values hidden; set one that is not the sign-in's, or clear them all: a real sign-out.",
  },
  {
    name: "get_autorun_failures",
    label: "Auto Run failures",
    summary: "What failed in a run on this machine, and when the script must be left alone.",
  },
  {
    name: "record_quirk",
    label: "Record a quirk",
    summary: "One line about how this application behaves, kept for the next script or template - or retire one that no longer helps.",
  },
  {
    name: "record_validation_rule",
    label: "Record a validation rule",
    summary: "Remembers (or removes) a validation rule the application showed in an area.",
  },
  {
    name: "mark_autorun_suspected_defect",
    label: "Mark a suspected defect",
    summary: "Mark a failed step as the application's fault, not the script's, with a note, and leave the script alone.",
  },
  {
    name: "report_case_divergence",
    label: "Report a case divergence",
    summary: "Tell the person where the live app or its data differs from a case, and keep the script as the case says.",
  },
  {
    name: "set_autorun_order",
    label: "Set Auto Run's order",
    summary: "Set the order Auto Run runs a PBI's cases in on this machine. Run Tests' order is not changed.",
  },
  {
    name: "propose_accounts",
    label: "Propose test logins",
    summary: "Suggest logins for the active environment - with their passwords only in a test environment - for you to add as accounts.",
  },
  {
    name: "get_accounts",
    label: "Read the accounts",
    summary: "The active environment's account keys and usernames - with passwords only in a test environment - and this project's Test files a case can upload.",
  },
  {
    name: "save_test_file",
    label: "Save a test file",
    summary: "Save a file the assistant made into this project's Test files, so a case or a template can upload it. Only documents a test uploads are taken.",
  },
  {
    name: "list_api_templates",
    label: "List API templates",
    summary:
      "This project's saved templates, filtered by module, search, flow or id and paged: in full when one page holds them, otherwise a compact index. Or its fixtures, with their current outputs and last run.",
  },
  {
    name: "prove_api_template",
    label: "Prove an API template",
    summary: "Run a draft template and save it only if every step passed.",
  },
  {
    name: "run_api_template",
    label: "Run an API template",
    summary: "Run a saved template and return its outputs, or what had been created when it failed.",
  },
  {
    name: "save_api_flow",
    label: "Save a flow",
    summary: "Save a module's stages with a check for each, after running every check once on a sample record.",
  },
  {
    name: "get_api_flow_progress",
    label: "Flow progress",
    summary: "Which stages are done for one record and which come next.",
  },
  {
    name: "save_api_fixture",
    label: "Save a fixture",
    summary: "Save an ordered list of proven templates that makes a draft the same way every time.",
  },
  {
    name: "run_api_fixture",
    label: "Run a fixture",
    summary: "Run or rebuild a fixture and return its outputs, what it made and any warnings.",
  },
  {
    name: "db_lookup",
    label: "Find a table",
    summary: "The tables and columns behind a topic, or one table's whole column list.",
  },
  {
    name: "db_query",
    label: "Run a statement",
    summary: "Run one SQL statement, or several as one all-or-nothing transaction, on the chosen connection and read the result.",
  },
  {
    name: "open_report",
    label: "Open a report",
    summary: "Open a Crystal Reports .rpt file and read it as a tree of areas, sections and objects - or create a blank one and open it.",
  },
  {
    name: "read_report",
    label: "Read the open report",
    summary: "Read the open report again, or only one section or object of it.",
  },
  {
    name: "edit_report",
    label: "Edit the open report",
    summary: "Apply one batch of edits, or a layout plan, and save it over the file.",
  },
  {
    name: "compare_reports",
    label: "Compare two reports",
    summary: "List every difference between two .rpt files.",
  },
  {
    name: "check_report",
    label: "Check a report",
    summary: "Run the offline load checks on a report: what the engine would refuse, and what it would only warn about.",
  },
  {
    name: "preview_report",
    label: "Preview the open report",
    summary: "Draw the open report to a PDF from its saved data, with the page count and the renderer's notes.",
  },
  {
    name: "preview_report_live",
    label: "Preview with live data",
    summary:
      "Draw the open report from fresh rows: run its stored procedure on the chosen database. Needs writes switched on and the Dev login.",
  },
  { name: "validate_cases", label: "Check a draft", summary: "Check a draft with the app's real importer." },
  { name: "get_tags", label: "Project tags", summary: "Tag names this project already uses." },
  { name: "optimize_cases", label: "Build the run sheet", summary: "Reorganise a draft into a tester-ready run sheet." },
  { name: "transform_cases", label: "Bulk edits", summary: "Bulk edits: retag, retitle, set module, sort, dedupe." },
  { name: "search_pbis", label: "Find a PBI",
    // "Work item" is the umbrella - a bug and a task are work items too.
    // This query filters on type = Product Backlog Item, so it returns
    // nothing else, and saying "work item" promised more than it does.
    summary: "Find a Product Backlog Item by title, or by its id." },
  { name: "search_wiki", label: "Search the wiki", summary: "Search the project wiki for documentation." },
  { name: "get_wiki_page", label: "Read a wiki page", summary: "Read a wiki page found by search_wiki." },
];

/** Mirrors ai_tools.rs - the Rust side is the one that enforces both. */
export const CORE_TOOLS = ["begin_test_case_writing", "get_guide", "get_test_cases", "check_spec_coverage", "transform_cases",
  // Finishing a draft is part of writing one: a set that cannot be
  // checked, ordered into a run sheet, or merged back from its slices
  // is a set nobody can ship.
  "validate_cases", "optimize_cases", "merge_case_files"] as const;

/** The tools of each module, mirroring `ai_tools.rs`'s `AUTORUN_TOOLS`,
 * `API_TEMPLATE_TOOLS`, `DB_TOOLS` and `CRYSTAL_TOOLS` (a test reads the Rust consts and
 * compares). A tool whose module is off is absent entirely - not listed, no
 * switch, no skill file - and Rust refuses a direct call. */
export const AUTORUN_TOOLS = [
  "save_autorun_script",
  "get_autorun_script",
  "delete_autorun_script",
  "get_autorun_page",
  "probe_autorun_locator",
  "try_autorun_action",
  "replay_autorun_to_step",
  "start_autorun_discovery",
  "discover_autorun_actions",
  "save_autorun_area",
  "end_autorun_discovery",
  "reset_autorun_browser",
  "save_autorun_component",
  "get_autorun_failures",
  "record_quirk",
  "record_validation_rule",
  "mark_autorun_suspected_defect",
  "report_case_divergence",
  "set_autorun_order",
  "propose_accounts",
  "get_accounts",
  "save_test_file",
  "get_autorun_network_wait",
  "search_autorun_requests",
  "get_autorun_request",
  "autorun_capture",
  "autorun_har",
  "set_autorun_headers",
  "autorun_cookies",
] as const;

/** The network tools: Auto Run's (in AUTORUN_TOOLS and its row), and
 * besides that behind the Network tools switch (networkTools.ts), which Rust
 * enforces - mirroring `ai_tools.rs`'s `NETWORK_TOOLS`. */
export const NETWORK_TOOLS = [
  "search_autorun_requests",
  "get_autorun_request",
  "autorun_capture",
  "autorun_har",
  "set_autorun_headers",
  "autorun_cookies",
] as const;

export const API_TEMPLATE_TOOLS = [
  "list_api_templates",
  "prove_api_template",
  "run_api_template",
  "save_api_flow",
  "get_api_flow_progress",
  "save_api_fixture",
  "run_api_fixture",
] as const;

export const DB_TOOLS = ["db_lookup", "db_query"] as const;

export const CRYSTAL_TOOLS = [
  "open_report",
  "read_report",
  "edit_report",
  "compare_reports",
  "check_report",
  "preview_report",
  "preview_report_live",
] as const;

/** The module a tool belongs to; null for a tool that is part of Core.
 * Mirrors `ai_tools.rs`'s `module_of_tool`. */
export function moduleOfTool(name: string): Module | null {
  if ((AUTORUN_TOOLS as readonly string[]).includes(name)) return "auto_run";
  if ((API_TEMPLATE_TOOLS as readonly string[]).includes(name)) return "api_templates";
  if ((DB_TOOLS as readonly string[]).includes(name)) return "db_access";
  if ((CRYSTAL_TOOLS as readonly string[]).includes(name)) return "crystal_reports";
  return null;
}

/** Whether a tool is offered right now: Core, or its module is really on.
 * Read live from the REAL set, not capture mode's all-on view: it decides
 * what `loadDisabledTools()` keeps and what `snapshotKeyFor()` keys on,
 * which feed `register`'s `commands.registerAiTool(...)` - real files Rust
 * writes to disk. Capture mode must affect display only; see `toolShown`. */
export function toolOffered(name: string): boolean {
  const m = moduleOfTool(name);
  return m === null || realModulesSnapshot().has(m);
}

/** Whether the AI Tools screen should show a tool right now: its module is
 * on (capture mode's view: every module), hidden again in capture mode for the Auto Run and API template tools - the
 * screen is documented and captured, so a shot must never name them.
 * DISPLAY ONLY: never wire this into `loadDisabledTools`, `snapshotKeyFor`,
 * or anything that feeds `register`/`setBridgeContext`. */
export function toolShown(name: string): boolean {
  const m = moduleOfTool(name);
  if (m !== null && !modulesSnapshot().has(m)) return false;
  return !(isCaptureMode() && (m === "auto_run" || m === "api_templates"));
}

export function isCoreTool(name: string): boolean {
  return (CORE_TOOLS as readonly string[]).includes(name);
}

/** Tools that are one CHOICE, so they get one switch.
 *
 * `get_wiki_page` reads a page `search_wiki` found - it takes the path from
 * a search hit and has no way to name a page on its own. Offered as two
 * switches, half the combinations were useless: a search whose results
 * nothing can open, or a reader that can never be handed anything. The
 * pair moves together under one human name; "AI Tools Breakdown" on the
 * same tab describes each half.
 */
export const TOOL_PAIRS: readonly (readonly string[])[] = [
  ["search_wiki", "get_wiki_page"],
  // The same shape: the reader takes the plan id and suite id the search
  // returns, and has no other way to name a suite.
  ["search_test_suites", "get_suite_test_cases"],
  // Not the same shape as the two above - each of these can be called on
  // its own - but authoring an Auto Run script is ONE job, and these are
  // its steps: read the format, look at the page, try a locator or an
  // action, read what a run did, save the result and record what you
  // learned. Half of them switched on is half a job, so they move
  // together. Offered only while the Auto Run module is on; see toolOffered.
  // The accounts a script runs as belong to the same job: proposing
  // logins for the environment, and reading the keys a script names. The
  // network tools ride here too, and besides need the Network tools switch
  // (networkTools.ts), which Rust enforces.
  // (The guide is Core's `get_guide`, refused by topic while the module is
  // off.)
  [
    "save_autorun_script",
    "get_autorun_script",
    "delete_autorun_script",
    "get_autorun_page",
    "probe_autorun_locator",
    "try_autorun_action",
    "replay_autorun_to_step",
    "start_autorun_discovery",
    "discover_autorun_actions",
    "save_autorun_area",
    "end_autorun_discovery",
    "reset_autorun_browser",
    "save_autorun_component",
    "get_autorun_failures",
    "record_quirk",
    "record_validation_rule",
    "mark_autorun_suspected_defect",
    "report_case_divergence",
    "set_autorun_order",
    "propose_accounts",
    "get_accounts",
    "save_test_file",
    "get_autorun_network_wait",
    "search_autorun_requests",
    "get_autorun_request",
    "autorun_capture",
    "autorun_har",
    "set_autorun_headers",
    "autorun_cookies",
  ],
  // Building an API template is one job too: read the format, see what
  // is saved, prove a draft, run it. A list with no way to run what it
  // lists, or a prove with no guide to the format, is half a job. Offered
  // only while the API Templates module is on. Proving and running also need the separate
  // API templates switch - a different decision, like database writes.
  // The stages a module's wizard is mapped into belong to that job: a
  // template that writes to a stage is only as good as the order the
  // application allows, and that order is what the flow tools hold.
  [
    "list_api_templates",
    "prove_api_template",
    "run_api_template",
    "save_api_flow",
    "get_api_flow_progress",
    // A fixture is built from the proven templates, and run like one.
    "save_api_fixture",
    "run_api_fixture",
    // What a template's author learns about the application goes on the
    // project's one quirks list, through the Auto Run row's own tool: it
    // rides in both rows (SHARED_TOOLS), so switching Auto Run off does not
    // take it away.
    "record_quirk",
  ],
  // Reading the company database is one choice: finding the table and
  // reading it are two halves of the same question, and a lookup whose
  // answer nothing can query is a map with no road. Creating, updating
  // and deleting is a SEPARATE switch, on the Database Read Access card - it
  // is a different decision, and it is off until someone makes it.
  ["db_lookup", "db_query"],
  // Working on a report is one job: open it, read it, change it, check it,
  // see it. A preview with nothing to open is half a job. Offered only while
  // the Crystal Reports module is on; its guide is Core's `get_guide`,
  // refused by topic while the module is off.
  [
    "open_report",
    "read_report",
    "edit_report",
    "compare_reports",
    "check_report",
    "preview_report",
    "preview_report_live",
  ],
];

/** The one human name and summary a pair shows, keyed by its first member. */
const PAIR_ROWS: Record<string, { label: string; summary: string }> = {
  search_wiki: { label: "Project wiki", summary: "Search the project wiki and read the pages it finds." },
  search_test_suites: {
    label: "Test Suites",
    summary: "Find a test suite by plan, name or PBI, and read the cases in it.",
  },
  save_autorun_script: {
    label: "Auto Run scripts",
    summary:
      "Read the script guide, see the page in the open browser, try a locator or an action, replay a case to its failing step, read a run's failures, save, read, repair and delete scripts, set Auto Run's order for a PBI, and propose and read the environment's test logins.",
  },
  list_api_templates: {
    label: "API templates",
    summary:
      "Map a module's stages, then build, prove and run templates that write test data through the application's own endpoints, in the order the application allows.",
  },
  db_lookup: {
    label: "Database Read Access",
    summary: "Look up tables and run SELECT on the connection chosen below.",
  },
  open_report: {
    label: "Crystal Reports",
    summary:
      "Open, read, edit, compare, check and preview Crystal Reports .rpt files, and create blank ones. An accepted edit is saved over the file at once.",
  },
};

function pairOf(name: string): readonly string[] | undefined {
  return TOOL_PAIRS.find((p) => p.includes(name));
}

/** One line in the AI Bridge list: a single tool, or a pair sharing a switch. */
export type McpToolRow = { key: string; label: string; summary: string; names: string[] };

/** The rows the AI Bridge tab renders, with each pair collapsed into one.
 *
 * A pair takes the position of its FIRST member and carries a summary
 * describing the pair rather than either half.
 */
export function visibleRows(): McpToolRow[] {
  const rows: McpToolRow[] = [];
  const done = new Set<string>();
  for (const t of visibleTools()) {
    if (done.has(t.name)) continue;
    const pair = pairOf(t.name);
    if (!pair) {
      rows.push({ key: t.name, label: t.label, summary: t.summary, names: [t.name] });
      continue;
    }
    for (const n of pair) done.add(n);
    const row = PAIR_ROWS[pair[0]] ?? { label: t.label, summary: t.summary };
    rows.push({
      key: pair.join("+"),
      label: row.label,
      summary: row.summary,
      names: [...pair],
    });
  }
  return rows;
}

/** Switch a row: every name it carries moves together. Off when any of them
 * is already off, so a half-off pair turns fully ON at the first click
 * rather than needing two. */
export function toggleRow(disabled: string[], names: string[]): string[] {
  const anyOff = names.some((n) => disabled.includes(n));
  if (anyOff) return disabled.filter((n) => !names.includes(n));
  return [...disabled, ...names];
}

/** The rows the AI Bridge tab renders: the tools that can be switched.
 *
 * The always-on tools never appear - a row carrying no switch was a
 * control that did nothing, and they are enforced in `ai_tools.rs`
 * whatever this list shows. Nor do the Auto Run tools, unless they are
 * offered - a switch for a tool that is not offered would toggle
 * nothing. What is left is exactly the set of choices this screen can
 * honour, right now.
 */
export function visibleTools(): McpToolInfo[] {
  return MCP_TOOLS.filter(
    (t) =>
      !isCoreTool(t.name) &&
      // Display gate only - see toolShown's own doc comment.
      toolShown(t.name),
  );
}

/** Tools that were renamed, old name -> new. A saved list naming the old
 *  one means the person switched it off; the new name has to stay off
 *  too, or a rename would quietly hand an assistant a tool they refused. */
export const RENAMED_TOOLS: Readonly<Record<string, string>> = {
  get_run_failures: "get_run_results",
  // Merged into one quirk tool that records and retires.
  record_autorun_quirk: "record_quirk",
  retire_autorun_quirk: "record_quirk",
  record_app_quirk: "record_quirk",
  retire_app_quirk: "record_quirk",
  // The four guides became Core's one `get_guide`: the name is then dropped
  // as Core's, rather than carried forever as a name that matches nothing.
  get_writing_guide: "get_guide",
  get_autorun_guide: "get_guide",
  get_api_template_guide: "get_guide",
  get_report_guide: "get_guide",
  // Folded into a sibling as an argument or part of its answer.
  list_test_files: "get_accounts",
  list_api_fixtures: "list_api_templates",
  new_report: "open_report",
  remove_autorun_component: "save_autorun_component",
  discover_autorun_action: "discover_autorun_actions",
  inspect_autorun_element: "probe_autorun_locator",
  stop_autorun_replay: "replay_autorun_to_step",
  release_autorun_browser: "reset_autorun_browser",
  list_autorun_requests: "search_autorun_requests",
  get_autorun_response: "get_autorun_request",
  get_autorun_response_body: "get_autorun_request",
  set_autorun_capture: "autorun_capture",
  get_autorun_capture: "autorun_capture",
  export_autorun_requests: "autorun_har",
  import_autorun_requests: "autorun_har",
  get_autorun_cookies: "autorun_cookies",
  set_autorun_cookie: "autorun_cookies",
  clear_autorun_cookies: "autorun_cookies",
};

/** Tools that ride in more than one row. Such a tool is off only while
 * every row carrying it is off - a row whose module is off counts as off -
 * so switching one row off never takes it from the job another row is on
 * for. `loadDisabledTools` settles it; the screen and Rust read that. */
export const SHARED_TOOLS: readonly string[] = [
  ...new Set(TOOL_PAIRS.flat().filter((n, i, all) => all.indexOf(n) !== i)),
];

function isShared(name: string): boolean {
  return SHARED_TOOLS.includes(name);
}

/** A row is on while any of its own (not shared) tools is offered and not
 * in `disabled`. */
function rowOn(pair: readonly string[], disabled: readonly string[]): boolean {
  return pair.some((n) => !isShared(n) && toolOffered(n) && !disabled.includes(n));
}

/** `disabled` with each shared tool off exactly when no row carrying it is
 * on. */
function settleShared(disabled: string[]): string[] {
  const out = disabled.filter((n) => !isShared(n));
  for (const name of SHARED_TOOLS) {
    const on = TOOL_PAIRS.filter((p) => p.includes(name)).some((p) => rowOn(p, out));
    if (!on && toolOffered(name)) out.push(name);
  }
  return out;
}

/** Whether Database Read Access is switched on: the Database Access module
 * is on and the reading pair's `db_query` is not among the disabled tools.
 * Auto Run's precondition checks follow it - while it is off, none is run. */
export function dbReadAccessOn(): boolean {
  return modulesSnapshot().has("db_access") && !disabledToolsSnapshot().includes("db_query");
}

export function loadDisabledTools(): string[] {
  try {
    const raw = localStorage.getItem(storageKey());
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const renamed = parsed.map((t) => (typeof t === "string" ? RENAMED_TOOLS[t] ?? t : t));
    const kept: string[] = [...new Set(renamed)].filter(
      (t) =>
        typeof t === "string" &&
        !isCoreTool(t) &&
        // A list saved while a module was on can name one of its tools.
        // Dropped while that module is off, where it would be meaningless -
        // there is no request it could ever be attached to. The database
        // pair is the exception: switching Database Access off and on
        // again must not quietly re-enable reading the person had refused.
        (toolOffered(t) || moduleOfTool(t) === "db_access"),
    );
    // A list saved before the pairing can name one half of a pair. Complete
    // it toward OFF: the pair is one switch now, and the alternative would
    // silently hand an assistant a tool this list says is off. A shared
    // tool says nothing about any one row: it is settled after.
    for (const pair of TOOL_PAIRS) {
      if (pair.some((n) => !isShared(n) && kept.includes(n))) {
        for (const n of pair) if (!isShared(n) && !kept.includes(n)) kept.push(n);
      }
    }
    return settleShared(kept);
  } catch {
    return [];
  }
}

const listeners = new Set<() => void>();

/** Every name in storage, unfiltered. */
function storedNames(): string[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(storageKey()) ?? "[]");
    return Array.isArray(parsed) ? parsed.filter((t): t is string => typeof t === "string") : [];
  } catch {
    return [];
  }
}

/** Save the list the screen shows. Off hides, never deletes: the names of a
 * module that is off are not on the screen's list (the read side hides
 * them), so they are carried over from storage - otherwise any toggle made
 * while a module is off would erase them, and they would come back enabled
 * with the module. */
export function saveDisabledTools(names: string[]): void {
  const hidden = storedNames().filter((n) => !isCoreTool(n) && moduleOfTool(n) !== null && !toolOffered(n));
  const all = [...new Set([...names, ...hidden])];
  try {
    if (all.length === 0) localStorage.removeItem(storageKey());
    else localStorage.setItem(storageKey(), JSON.stringify(all));
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  for (const l of listeners) l();
}

/** Subscription so App can re-push the bridge context the moment a tool is
 * toggled, instead of the change waiting for the next org/project change.
 * Also fires when a module is switched on or off: that changes which saved
 * names count - and when the active environment changes, which changes
 * whose set it is. */
export function subscribeDisabledTools(cb: () => void): () => void {
  listeners.add(cb);
  const offModules = subscribeModules(cb);
  const offEnv = subscribeActiveEnv(cb);
  return () => {
    listeners.delete(cb);
    offModules();
    offEnv();
  };
}

/** Keyed on whose set it is, the stored list AND which modules are on: the
 * same stored list loads differently either side of a module being
 * switched. */
function snapshotKeyFor(raw: string): string {
  return `${storageKey()}|${[...realModulesSnapshot()].sort().join("+")}:${raw}`;
}

let snapshot: string[] = loadDisabledTools();
let snapshotKey = snapshotKeyFor(JSON.stringify(snapshot));

/** A STABLE array reference between changes - useSyncExternalStore
 * re-renders forever if the snapshot is a fresh object every call. */
export function disabledToolsSnapshot(): string[] {
  let raw = "[]";
  try {
    raw = localStorage.getItem(storageKey()) ?? "[]";
  } catch {
    // fall through to the cached value
  }
  const key = snapshotKeyFor(raw);
  if (key !== snapshotKey) {
    snapshotKey = key;
    snapshot = loadDisabledTools();
  }
  return snapshot;
}

export function toggleTool(disabled: string[], name: string): string[] {
  if (isCoreTool(name)) return disabled;
  return disabled.includes(name)
    ? disabled.filter((n) => n !== name)
    : [...disabled, name];
}
