// Which of our MCP tools an assistant is allowed to call.
//
// Stored as the DISABLED set, not the enabled one: a new tool added in a
// later release is then available by default rather than silently missing
// because it wasn't in someone's saved list.

import { extrasUnlockedSnapshot, subscribeExtras } from "./extras";
import { isCaptureMode } from "../dev/capture";

const KEY = "tcm-v2-mcp-disabled";

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
  { name: "get_run_failures", label: "Run failures", summary: "What failed in a PBI's latest runs, with the tester's comments." },
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
    name: "db_lookup",
    label: "Find a table",
    summary: "The tables and columns behind a topic, or one table's whole column list.",
  },
  {
    name: "db_query",
    label: "Run a statement",
    summary: "Run one SQL statement on the chosen connection and read the result.",
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

/** The Auto Run tools: offered only where Auto Run is (autoRunToolsOffered)
 * - absent entirely (not listed, no switch, no skill file) elsewhere.
 * Mirrors `ai_tools.rs`'s `DEV_ONLY_TOOLS`. */
export const DEV_ONLY_TOOLS = [
  "get_autorun_guide",
  "save_autorun_script",
  "get_autorun_page",
  "probe_autorun_locator",
  "try_autorun_action",
  "get_autorun_failures",
  "record_autorun_quirk",
] as const;

/** True in `tauri dev` and in this test suite, false in `tauri build` - a
 * compile-time constant, read once at module load. Mirrors
 * `lib/extras.ts`'s `autoRunVisible` and `ai_tools.rs`'s `dev_build()`. */
export const DEV_BUILD: boolean = import.meta.env.DEV;

/** Whether the Auto Run tools are offered right now: always in a
 * development build, and in a release build once this machine's optional
 * extras are unlocked (lib/extras). Read live, not at module load. Mirrors
 * `ai_tools.rs`'s `autorun_offered()`.
 *
 * Deliberately NOT capture-mode-aware: this also decides what
 * `loadDisabledTools()` keeps and what `snapshotKeyFor()` keys on, which
 * feed `register`'s `commands.registerAiTool(...)` - real files Rust writes
 * to disk. Capture mode must affect display only; see `autoRunToolsShown`
 * for the screen's own gate. */
export function autoRunToolsOffered(): boolean {
  return DEV_BUILD || extrasUnlockedSnapshot();
}

/** Whether the AI Tools screen should show anything about the Auto Run
 * tools right now: `autoRunToolsOffered()`, hidden again in capture mode -
 * the screen is documented and captured, so a shot must never name them.
 * DISPLAY ONLY: never wire this into `loadDisabledTools`, `snapshotKeyFor`,
 * or anything that feeds `register`/`setBridgeContext` - what is actually
 * offered, saved and registered must never depend on capture mode. */
export function autoRunToolsShown(): boolean {
  return autoRunToolsOffered() && !isCaptureMode();
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
  // together. Offered only where Auto Run is; see autoRunToolsOffered.
  [
    "get_autorun_guide",
    "save_autorun_script",
    "get_autorun_page",
    "probe_autorun_locator",
    "try_autorun_action",
    "get_autorun_failures",
    "record_autorun_quirk",
  ],
  // Reading the company database is one choice: finding the table and
  // reading it are two halves of the same question, and a lookup whose
  // answer nothing can query is a map with no road. Creating, updating
  // and deleting is a SEPARATE switch, on the Company database card - it
  // is a different decision, and it is off until someone makes it.
  ["db_lookup", "db_query"],
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
      "Read the script guide, see the page in the open browser, try a locator or an action, read a run's failures, save and repair scripts.",
  },
  db_lookup: {
    label: "Company database (read)",
    summary: "Look up tables and run SELECT on the connection chosen below.",
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
      // Display gate only - see autoRunToolsShown's own doc comment.
      (autoRunToolsShown() || !(DEV_ONLY_TOOLS as readonly string[]).includes(t.name)),
  );
}

export function loadDisabledTools(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    const kept: string[] = parsed.filter(
      (t) =>
        typeof t === "string" &&
        !isCoreTool(t) &&
        // A list saved while the Auto Run tools were offered can name one
        // of them. Dropped where the Auto Run tools are not offered,
        // where it would be meaningless - there is no request it could
        // ever be attached to.
        (autoRunToolsOffered() || !(DEV_ONLY_TOOLS as readonly string[]).includes(t)),
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

export function saveDisabledTools(names: string[]): void {
  try {
    if (names.length === 0) localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, JSON.stringify(names));
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  for (const l of listeners) l();
}

/** Subscription so App can re-push the bridge context the moment a tool is
 * toggled, instead of the change waiting for the next org/project change.
 * Also fires when the optional extras are unlocked or relocked: that
 * changes which saved names count. */
export function subscribeDisabledTools(cb: () => void): () => void {
  listeners.add(cb);
  const offExtras = subscribeExtras(cb);
  return () => {
    listeners.delete(cb);
    offExtras();
  };
}

/** Keyed on the stored list AND whether the Auto Run tools are offered:
 * the same stored list loads differently either side of an unlock. */
function snapshotKeyFor(raw: string): string {
  return `${autoRunToolsOffered() ? 1 : 0}:${raw}`;
}

let snapshot: string[] = loadDisabledTools();
let snapshotKey = snapshotKeyFor(JSON.stringify(snapshot));

/** A STABLE array reference between changes - useSyncExternalStore
 * re-renders forever if the snapshot is a fresh object every call. */
export function disabledToolsSnapshot(): string[] {
  let raw = "[]";
  try {
    raw = localStorage.getItem(KEY) ?? "[]";
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
