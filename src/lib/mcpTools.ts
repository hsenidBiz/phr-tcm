// Which of our MCP tools an assistant is allowed to call.
//
// Stored as the DISABLED set, not the enabled one: a new tool added in a
// later release is then available by default rather than silently missing
// because it wasn't in someone's saved list.
//
// Each environment keeps its own set (activeEnv.ts); everything here reads
// and writes the ACTIVE environment's.
//
// The set never names a module's tool. A tool that belongs to a module is
// offered exactly while the module is on in Settings: the module is its
// switch, and a second one here would only be a way to lose a tool with the
// module on and nothing left to turn it back on.

import { BRIDGE_SWITCH_KEYS, envScopedKey, subscribeActiveEnv } from "./activeEnv";
import { modulesSnapshot, type Module } from "./modules";
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
  { name: "search_test_suites", label: "Find a test suite", summary: "The plans and suites, by plan name, suite name or PBI id." },
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
    summary: "Read the page in the assistant's own Auto Run browser (or the one you opened), one element per line with its locator.",
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
    summary: "Open an Auto Run browser and sign in as a saved account, so the assistant can explore the live app. It opens one of its own when another assistant is using the main one.",
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
    summary: "End the assistant's exploration in its own Auto Run browser.",
  },
  {
    name: "reset_autorun_browser",
    label: "Reset the Auto Run browser",
    summary: "Give the assistant's own Auto Run browser a clean page - other tabs closed, back at the home page - without signing out, or let go of a broken one. Never another assistant's.",
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
    name: "record_explore_change",
    label: "Record a change",
    summary: "Note a record the assistant created, changed or deleted while exploring, so it can be undone later.",
  },
  {
    name: "get_explore_changes",
    label: "Explore change lists",
    summary: "The assistant's explore sessions, newest first, or one session's change list and the saves it did not record.",
  },
  {
    name: "get_kept_actions",
    label: "Kept actions",
    summary: "The actions of the assistant's explore session that worked, numbered, to save as a setup.",
  },
  {
    name: "save_setup",
    label: "Save a setup",
    summary: "Save kept actions as a named setup that builds a test record, with a rule to find one that exists.",
  },
  {
    name: "list_setups",
    label: "Saved setups",
    summary: "The project's saved setups: what each builds, its inputs and when it was last used.",
  },
  {
    name: "run_setup",
    label: "Run a setup",
    summary: "Find a test record that fits, or build one in the explore session, on its change list.",
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
    name: "rule_case_divergence",
    label: "Rule on a divergence",
    summary: "Apply the ruling you gave in the chat on a divergence, quoting your words, as you would on the case card. You can undo it there.",
  },
  {
    name: "clear_autorun_suspected_defect",
    label: "Clear a suspected defect",
    summary: "Clear the assistant's suspected-defect mark on a case when you say so in the chat.",
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
    name: "send_api_request",
    label: "Send one API request",
    summary: "Send one request from the assistant's explore session's own signed-in page, and save a call that worked as an API template.",
  },
  {
    name: "undo_explore_creates",
    label: "Undo an explore session's creates",
    summary: "Delete what the assistant's own explore session created, through the proven delete templates, when you ask it to in the chat.",
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
  {
    name: "get_report_parameters",
    label: "Report parameters",
    summary: "List the open report's parameters, their kinds and pick lists.",
  },
  {
    name: "find_in_report",
    label: "Find in the report",
    summary: "Find text and field references across the open report and its subreports.",
  },
  {
    name: "replace_in_report",
    label: "Replace in the report",
    summary: "Replace text across the open report, with a dry run that shows each change first.",
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
  { name: "search_wiki", label: "Search the wiki", summary: "Search the wikis for documentation." },
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
  "rule_case_divergence",
  "clear_autorun_suspected_defect",
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
  "record_explore_change",
  "get_explore_changes",
  "get_kept_actions",
  "save_setup",
  "list_setups",
  "run_setup",
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
  "send_api_request",
  "undo_explore_creates",
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
  "get_report_parameters",
  "find_in_report",
  "replace_in_report",
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

/** Whether the AI Tools screen should show a module's tool (in its breakdown
 * and cards) right now: its module is on (capture mode's view: every
 * module), hidden again in capture mode for the Auto Run and API template
 * tools - the screen is documented and captured, so a shot must never name
 * them. Whether a tool is OFFERED is Rust's call, from the real modules.
 * DISPLAY ONLY: never wire this into `loadDisabledTools` or anything that
 * feeds `register`/`setBridgeContext`. */
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
  // A module's tools are not here: they have no switch. The module being on
  // in Settings is what offers them, as a group, and Rust enforces it.
];

/** The one human name and summary a pair shows, keyed by its first member. */
const PAIR_ROWS: Record<string, { label: string; summary: string }> = {
  search_wiki: { label: "Project wiki", summary: "Search the wikis and read the pages they find." },
  search_test_suites: {
    label: "Test Suites",
    summary: "Find a test suite by plan, name or PBI, and read the cases in it.",
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
 * whatever this list shows. Nor does a module's tool: the module being on
 * in Settings is its switch, so a second one here is only a way to lose a
 * tool with its module on. What is left is exactly the set of choices this
 * screen can honour.
 */
export function visibleTools(): McpToolInfo[] {
  return MCP_TOOLS.filter((t) => !isCoreTool(t.name) && moduleOfTool(t.name) === null);
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

/** Whether Database Read Access is on: the Database Access module is on.
 * Auto Run's precondition checks follow it - while it is off, none is run. */
export function dbReadAccessOn(): boolean {
  return modulesSnapshot().has("db_access");
}

/** A stored list as names this screen can honour: old names carried to the
 * new ones, no repeats, nothing but strings, and neither a core tool (always
 * on) nor a module's tool (on with its module, with no switch of its own). */
function switchableNames(parsed: unknown[]): string[] {
  const renamed = parsed.map((t) => (typeof t === "string" ? RENAMED_TOOLS[t] ?? t : t));
  return [...new Set(renamed)].filter(
    (t): t is string => typeof t === "string" && !isCoreTool(t) && moduleOfTool(t) === null,
  );
}

export function loadDisabledTools(): string[] {
  try {
    const raw = localStorage.getItem(storageKey());
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const kept = switchableNames(parsed);
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

/** Take a module's tools out of every saved list: each environment's, and the
 * single one older versions kept before there were environments.
 *
 * A module's tools used to have a switch of their own on the AI Bridge tab,
 * and a person may have switched some off. That switch is gone - the module
 * being on is what offers them - so a name left in a saved list would keep a
 * tool from an assistant with nothing on screen to turn it back on. A list
 * left empty is removed; one with nothing to take out is not touched; one
 * that cannot be read is left as it is. Runs once at start-up, and does
 * nothing when run again. */
export function clearModuleToolSwitches(): void {
  const base = BRIDGE_SWITCH_KEYS.tools;
  try {
    const keys: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key !== null && (key === base || key.startsWith(`${base}:`))) keys.push(key);
    }
    for (const key of keys) {
      let parsed: unknown;
      try {
        parsed = JSON.parse(localStorage.getItem(key) ?? "");
      } catch {
        continue;
      }
      if (!Array.isArray(parsed)) continue;
      const kept = switchableNames(parsed);
      if (kept.length === parsed.length && kept.every((t, i) => t === parsed[i])) continue;
      if (kept.length === 0) localStorage.removeItem(key);
      else localStorage.setItem(key, JSON.stringify(kept));
    }
  } catch {
    // storage unavailable -> nothing saved to clean
  }
}

const listeners = new Set<() => void>();

/** Save the list the screen shows. A module's tool is never written: it has
 * no switch to take it off again. */
export function saveDisabledTools(names: string[]): void {
  const all = [...new Set(names)].filter((n) => !isCoreTool(n) && moduleOfTool(n) === null);
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
 * Also fires when the active environment changes, which changes whose set
 * it is. */
export function subscribeDisabledTools(cb: () => void): () => void {
  listeners.add(cb);
  const offEnv = subscribeActiveEnv(cb);
  return () => {
    listeners.delete(cb);
    offEnv();
  };
}

/** Keyed on whose set it is and the stored list. */
function snapshotKeyFor(raw: string): string {
  return `${storageKey()}:${raw}`;
}

clearModuleToolSwitches();
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
