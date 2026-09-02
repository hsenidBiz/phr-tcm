// The working repository: the one folder per-repo test-case files, skills
// and MCP registrations belong to. One choice for the whole app (a repo
// can serve several org/projects), persisted so it survives restarts, and
// observable so App re-pushes the bridge context the moment it changes.

const KEY = "tcm-v2-working-dir";

/** Mirrors `workspace::CASES_DIR` on the Rust side. */
export const CASES_DIR = ".test-cases";

export function loadWorkingDir(): string {
  try {
    return (localStorage.getItem(KEY) ?? "").trim();
  } catch {
    return "";
  }
}

const listeners = new Set<() => void>();

export function saveWorkingDir(path: string): void {
  const trimmed = path.trim();
  try {
    if (trimmed) localStorage.setItem(KEY, trimmed);
    else localStorage.removeItem(KEY);
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  for (const l of listeners) l();
}

export function subscribeWorkingDir(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

/** Strings compare by value, so a fresh read is a stable snapshot. */
export function workingDirSnapshot(): string {
  return loadWorkingDir();
}

export function casesDir(root: string): string {
  return `${root.replace(/[\\/]+$/, "")}\\${CASES_DIR}`;
}

const norm = (p: string) => p.replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();

/** Is `path` inside `<root>/.test-cases`? A sibling folder that merely
 * shares the prefix is outside. Matches `workspace::is_inside` in spirit;
 * the Rust side is authoritative where a file is actually written. */
export function isInsideCasesDir(root: string, path: string): boolean {
  const d = norm(casesDir(root));
  const p = norm(path);
  return p === d || p.startsWith(`${d}\\`);
}
