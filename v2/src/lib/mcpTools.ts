// Which of our MCP tools an assistant is allowed to call.
//
// Stored as the DISABLED set, not the enabled one: a new tool added in a
// later release is then available by default rather than silently missing
// because it wasn't in someone's saved list.

const KEY = "tcm-v2-mcp-disabled";

export type McpToolInfo = { name: string; summary: string };

/** Mirrors `mcp.rs`'s tools_list - kept here so the settings UI can show
 * what each tool does without a round trip. `tcm_mcp.rs` asserts the same
 * names on the Rust side, so a drift shows up as a failing test. */
export const MCP_TOOLS: McpToolInfo[] = [
  { name: "get_writing_guide", summary: "Format rules and your org's allowed Module values." },
  { name: "get_example_cases", summary: "Real cases from a PBI, to copy the house style." },
  { name: "get_tags", summary: "Tag names this project already uses." },
  { name: "optimize_cases", summary: "Reorganise a draft into a tester-ready run sheet." },
  { name: "transform_cases", summary: "Bulk edits: retag, retitle, set module, sort, dedupe." },
  { name: "search_pbis", summary: "Find a work item id by title." },
  { name: "search_wiki", summary: "Search the project wiki for documentation." },
  { name: "get_wiki_page", summary: "Read a wiki page found by search_wiki." },
];

export function loadDisabledTools(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((t) => typeof t === "string") : [];
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
 * toggled, instead of the change waiting for the next org/project change. */
export function subscribeDisabledTools(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

let snapshot: string[] = loadDisabledTools();
let snapshotRaw = JSON.stringify(snapshot);

/** A STABLE array reference between changes - useSyncExternalStore
 * re-renders forever if the snapshot is a fresh object every call. */
export function disabledToolsSnapshot(): string[] {
  let raw = "[]";
  try {
    raw = localStorage.getItem(KEY) ?? "[]";
  } catch {
    // fall through to the cached value
  }
  if (raw !== snapshotRaw) {
    snapshotRaw = raw;
    snapshot = loadDisabledTools();
  }
  return snapshot;
}

export function toggleTool(disabled: string[], name: string): string[] {
  return disabled.includes(name)
    ? disabled.filter((n) => n !== name)
    : [...disabled, name];
}
