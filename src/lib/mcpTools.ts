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
  { name: "get_writing_guide", label: "Writing guide", summary: "Format rules and your org's allowed Module values." },
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
    name: "get_autorun_guide",
    label: "Auto Run guide",
    summary: "How to write an Auto Run browser script, and where assertions may come from.",
  },
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
    summary: "What a locator matches on the open page right now, and whether it is visible.",
  },
  {
    name: "try_autorun_action",
    label: "Try an action",
    summary: "Carry out one script action against the open page, without recording anything.",
  },
  {
    name: "replay_autorun_to_step",
    label: "Replay to a step",
    summary: "Replay a case's saved steps up to the failing one, asking you first when its script must not save.",
  },
  {
    name: "start_autorun_discovery",
    label: "Start exploring",
    summary: "Open the Auto Run browser and sign in as a saved account, so the assistant can explore the live app.",
  },
  {
    name: "discover_autorun_action",
    label: "Explore one action",
    summary: "Carry out one action while exploring, and see what it did and what it wrote.",
  },
  {
    name: "discover_autorun_actions",
    label: "Explore several actions",
    summary: "Carry out up to 20 actions in order while exploring, and see how each went and the page they led to.",
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
    name: "release_autorun_browser",
    label: "Release the Auto Run browser",
    summary: "Let go of an Auto Run browser the app still holds after it closed, and close the app's own browsers.",
  },
  {
    name: "reset_autorun_browser",
    label: "Reset the Auto Run browser",
    summary: "Give the assistant's Auto Run browser a clean page - other tabs closed, back at the home page - without signing out.",
  },
  {
    name: "save_autorun_component",
    label: "Save a component",
    summary: "Save a widget or short flow, once tried live, as a component any script can use.",
  },
  {
    name: "remove_autorun_component",
    label: "Remove a component",
    summary: "Remove a component no saved script uses.",
  },
  {
    name: "get_autorun_network_wait",
    label: "Network wait",
    summary: "Whether an unattended run is paused because the company network is unreachable, and when it gives up.",
  },
  {
    name: "list_autorun_requests",
    label: "Captured requests",
    summary: "The requests the page in the Auto Run browser made, newest first, with secrets hidden.",
  },
  {
    name: "search_autorun_requests",
    label: "Search captured requests",
    summary: "Find captured requests by path, method, status, type or body text.",
  },
  {
    name: "get_autorun_request",
    label: "A captured request",
    summary: "One captured request's headers and body, with secrets hidden.",
  },
  {
    name: "get_autorun_response",
    label: "A captured response",
    summary: "One captured response's status, headers, size and timing.",
  },
  {
    name: "get_autorun_response_body",
    label: "A captured response body",
    summary: "One captured response's body, with secrets hidden, in pages.",
  },
  {
    name: "set_autorun_capture",
    label: "Set the capture filter",
    summary: "Which requests the Auto Run browser's capture keeps from now on.",
  },
  {
    name: "get_autorun_capture",
    label: "The capture filter",
    summary: "The capture's filter, how many requests it keeps, and the imported sets.",
  },
  {
    name: "export_autorun_requests",
    label: "Export captured requests",
    summary: "Save the capture, secrets hidden, as a HAR file in the app's own captures folder.",
  },
  {
    name: "import_autorun_requests",
    label: "Import captured requests",
    summary: "Load a HAR file from the app's captures folder as a read-only set.",
  },
  {
    name: "set_autorun_headers",
    label: "Set test headers",
    summary: "Extra request headers for the assistant's Auto Run browser, to test an edge case; named in every answer until reset.",
  },
  {
    name: "get_autorun_cookies",
    label: "Read cookies",
    summary: "The Auto Run browser's cookies for a site, with the sign-in's values hidden.",
  },
  {
    name: "set_autorun_cookie",
    label: "Set a test cookie",
    summary: "One cookie that is not the sign-in's, on the page's site in the assistant's Auto Run browser.",
  },
  {
    name: "clear_autorun_cookies",
    label: "Sign out by clearing cookies",
    summary: "Delete the site's cookies in the assistant's Auto Run browser: a real sign-out.",
  },
  {
    name: "inspect_autorun_element",
    label: "Inspect an element",
    summary: "One element's styles, listeners, animations and structure in the Auto Run browser, read without touching the page.",
  },
  {
    name: "get_autorun_failures",
    label: "Auto Run failures",
    summary: "What failed in a run on this machine, and when the script must be left alone.",
  },
  {
    name: "record_autorun_quirk",
    label: "Record a quirk",
    summary: "One line about how this application behaves, kept for the next script.",
  },
  {
    name: "retire_autorun_quirk",
    label: "Retire a quirk",
    summary: "Retire one of the assistant's own quirks that no longer helps, optionally with a better one.",
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
    summary: "The active environment's account keys and usernames - with passwords only in a test environment.",
  },
  {
    name: "list_test_files",
    label: "List test files",
    summary: "This project's Test files a case can upload, by name and size.",
  },
  {
    name: "get_api_template_guide",
    label: "API template guide",
    summary: "The template format, the authoring workflow, and this project's account keys and address.",
  },
  {
    name: "list_api_templates",
    label: "List API templates",
    summary:
      "This project's saved templates, filtered by module, search, flow or id and paged: in full when one page holds them, otherwise a compact index.",
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
    name: "list_api_fixtures",
    label: "List fixtures",
    summary: "This project's fixtures, with their current outputs and last run.",
  },
  {
    name: "record_app_quirk",
    label: "Record a quirk (API)",
    summary: "One line about how this application behaves, learned building API templates.",
  },
  {
    name: "retire_app_quirk",
    label: "Retire a quirk (API)",
    summary: "Retire one of the assistant's own quirks that no longer helps.",
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
    summary: "Open a Crystal Reports .rpt file and read it as a tree of areas, sections and objects.",
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
    name: "new_report",
    label: "Create a blank report",
    summary: "Create a blank .rpt at a path with no file yet, and open it.",
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
  {
    name: "get_report_guide",
    label: "Report guide",
    summary: "The units, the edit ops, the layout plan and the limits for working with reports.",
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
export const CORE_TOOLS = ["begin_test_case_writing", "get_writing_guide", "get_test_cases", "check_spec_coverage", "transform_cases",
  // Finishing a draft is part of writing one: a set that cannot be
  // checked, ordered into a run sheet, or merged back from its slices
  // is a set nobody can ship.
  "validate_cases", "optimize_cases", "merge_case_files"] as const;

/** The tools of each module, mirroring `ai_tools.rs`'s `AUTORUN_TOOLS`,
 * `API_TEMPLATE_TOOLS`, `DB_TOOLS` and `CRYSTAL_TOOLS` (a test reads the Rust consts and
 * compares). A tool whose module is off is absent entirely - not listed, no
 * switch, no skill file - and Rust refuses a direct call. */
export const AUTORUN_TOOLS = [
  "get_autorun_guide",
  "save_autorun_script",
  "get_autorun_script",
  "delete_autorun_script",
  "get_autorun_page",
  "probe_autorun_locator",
  "try_autorun_action",
  "replay_autorun_to_step",
  "start_autorun_discovery",
  "discover_autorun_action",
  "discover_autorun_actions",
  "save_autorun_area",
  "end_autorun_discovery",
  "release_autorun_browser",
  "reset_autorun_browser",
  "save_autorun_component",
  "remove_autorun_component",
  "get_autorun_failures",
  "record_autorun_quirk",
  "retire_autorun_quirk",
  "record_validation_rule",
  "mark_autorun_suspected_defect",
  "set_autorun_order",
  "propose_accounts",
  "get_accounts",
  "list_test_files",
  "get_autorun_network_wait",
  "list_autorun_requests",
  "search_autorun_requests",
  "get_autorun_request",
  "get_autorun_response",
  "get_autorun_response_body",
  "set_autorun_capture",
  "get_autorun_capture",
  "export_autorun_requests",
  "import_autorun_requests",
  "set_autorun_headers",
  "get_autorun_cookies",
  "set_autorun_cookie",
  "clear_autorun_cookies",
  "inspect_autorun_element",
] as const;

export const API_TEMPLATE_TOOLS = [
  "get_api_template_guide",
  "list_api_templates",
  "prove_api_template",
  "run_api_template",
  "save_api_flow",
  "get_api_flow_progress",
  "save_api_fixture",
  "run_api_fixture",
  "list_api_fixtures",
  "record_app_quirk",
  "retire_app_quirk",
] as const;

export const DB_TOOLS = ["db_lookup", "db_query"] as const;

export const CRYSTAL_TOOLS = [
  "open_report",
  "read_report",
  "edit_report",
  "compare_reports",
  "check_report",
  "new_report",
  "preview_report",
  "preview_report_live",
  "get_report_guide",
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
  // logins for the environment, and reading the keys a script names.
  [
    "get_autorun_guide",
    "save_autorun_script",
    "get_autorun_script",
    "delete_autorun_script",
    "get_autorun_page",
    "probe_autorun_locator",
    "try_autorun_action",
    "replay_autorun_to_step",
    "start_autorun_discovery",
    "discover_autorun_action",
    "discover_autorun_actions",
    "save_autorun_area",
    "end_autorun_discovery",
    "release_autorun_browser",
    "reset_autorun_browser",
    "save_autorun_component",
    "remove_autorun_component",
    "get_autorun_failures",
    "record_autorun_quirk",
    "retire_autorun_quirk",
    "record_validation_rule",
    "mark_autorun_suspected_defect",
    "set_autorun_order",
    "propose_accounts",
    "get_accounts",
    "list_test_files",
    "get_autorun_network_wait",
    "list_autorun_requests",
    "search_autorun_requests",
    "get_autorun_request",
    "get_autorun_response",
    "get_autorun_response_body",
    "set_autorun_capture",
    "get_autorun_capture",
    "export_autorun_requests",
    "import_autorun_requests",
    "set_autorun_headers",
    "get_autorun_cookies",
    "set_autorun_cookie",
    "clear_autorun_cookies",
    "inspect_autorun_element",
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
    "get_api_template_guide",
    "list_api_templates",
    "prove_api_template",
    "run_api_template",
    "save_api_flow",
    "get_api_flow_progress",
    // A fixture is built from the proven templates, and run like one.
    "save_api_fixture",
    "run_api_fixture",
    "list_api_fixtures",
    // What a template's author learns about the application goes on the
    // project's one quirks list - the Auto Run row's tools under this
    // row's own names, so switching Auto Run off does not take them away.
    "record_app_quirk",
    "retire_app_quirk",
  ],
  // Reading the company database is one choice: finding the table and
  // reading it are two halves of the same question, and a lookup whose
  // answer nothing can query is a map with no road. Creating, updating
  // and deleting is a SEPARATE switch, on the Database Read Access card - it
  // is a different decision, and it is off until someone makes it.
  ["db_lookup", "db_query"],
  // Working on a report is one job: open it, read it, change it, check it,
  // see it, and the guide that says how. An edit tool without the guide, or
  // a preview with nothing to open, is half a job. Offered only while the
  // Crystal Reports module is on.
  [
    "open_report",
    "read_report",
    "edit_report",
    "compare_reports",
    "check_report",
    "new_report",
    "preview_report",
    "preview_report_live",
    "get_report_guide",
  ],
];

/** The one human name and summary a pair shows, keyed by its first member. */
const PAIR_ROWS: Record<string, { label: string; summary: string }> = {
  search_wiki: { label: "Project wiki", summary: "Search the project wiki and read the pages it finds." },
  search_test_suites: {
    label: "Test Suites",
    summary: "Find a test suite by plan, name or PBI, and read the cases in it.",
  },
  get_autorun_guide: {
    label: "Auto Run scripts",
    summary:
      "Read the script guide, see the page in the open browser, try a locator or an action, replay a case to its failing step, read a run's failures, save, read, repair and delete scripts, set Auto Run's order for a PBI, and propose and read the environment's test logins.",
  },
  get_api_template_guide: {
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
};

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
    // silently hand an assistant a tool this list says is off.
    for (const pair of TOOL_PAIRS) {
      if (pair.some((n) => kept.includes(n))) {
        for (const n of pair) if (!kept.includes(n)) kept.push(n);
      }
    }
    return kept;
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
