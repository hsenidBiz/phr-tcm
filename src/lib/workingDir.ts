// The working repositories: the folders per-repo test-case files, skills
// and MCP registrations belong to. Several can be saved; exactly one is
// CURRENT - the one files go to and registration targets - and each has
// its own AI-tools switch. `loadWorkingDir()` is what the rest of the app
// reads: the current repository while its switch is on, else "" - so the
// bridge push, Import File's copy-in and the AI Bridge gate never had to
// learn about the list. Observable so they all react the moment it moves.

const LIST_KEY = "tcm-v2-repositories";
const CURRENT_KEY = "tcm-v2-current-repo";
/** The single-value key from before the list; migrated on first read. */
const LEGACY_KEY = "tcm-v2-working-dir";

/** Mirrors `workspace::CASES_DIR` on the Rust side. */
export const CASES_DIR = ".test-cases";

export type Repository = { path: string; enabled: boolean };

const norm = (p: string) => p.replace(/\//g, "\\").replace(/\\+$/, "").toLowerCase();

/** Two spellings of one folder - case and slash style do not make a
 * second repository on Windows. */
export function samePath(a: string, b: string): boolean {
  return norm(a) === norm(b);
}

function isRepository(v: unknown): v is Repository {
  return (
    typeof v === "object" &&
    v !== null &&
    typeof (v as Repository).path === "string" &&
    (v as Repository).path.trim() !== "" &&
    typeof (v as Repository).enabled === "boolean"
  );
}

function writeList(list: Repository[], current: string): void {
  try {
    if (list.length) localStorage.setItem(LIST_KEY, JSON.stringify(list));
    else localStorage.removeItem(LIST_KEY);
    if (current) localStorage.setItem(CURRENT_KEY, current);
    else localStorage.removeItem(CURRENT_KEY);
    localStorage.removeItem(LEGACY_KEY);
  } catch {
    // storage unavailable -> nothing is remembered; the tab will ask again
  }
}

/** The saved list from storage, migrating a pre-list single value into its first entry
 * (enabled and current) so nobody loses the repository they had set. What is ON DISK,
 * never the tour's override - this is the MUTATORS' way in, so a write during the tour
 * edits the user's real list and cannot persist sample data. Readers go through
 * `readList()`, which is where the override is applied. */
function readSaved(): { list: Repository[]; current: string } {
  try {
    const raw = localStorage.getItem(LIST_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      const list = Array.isArray(parsed) ? parsed.filter(isRepository) : [];
      const current = (localStorage.getItem(CURRENT_KEY) ?? "").trim();
      return { list, current: list.some((r) => samePath(r.path, current)) ? current : "" };
    }
    const legacy = (localStorage.getItem(LEGACY_KEY) ?? "").trim();
    if (legacy) {
      const list = [{ path: legacy, enabled: true }];
      writeList(list, legacy);
      return { list, current: legacy };
    }
  } catch {
    // corrupt storage -> start empty
  }
  return { list: [], current: "" };
}

/** The saved list, with the tour override applied for readers if active.
 * Mutators bypass this and use readSaved() directly. */
function readList(): { list: Repository[]; current: string } {
  if (tourList) return tourList;
  return readSaved();
}

export function loadRepositories(): Repository[] {
  return readList().list;
}

export function loadCurrentPath(): string {
  return readList().current;
}

/** The repository the app works in right now: the current one while its
 * AI-tools switch is on, else "" (nothing selected, or selected but off). */
export function loadWorkingDir(): string {
  const { list, current } = readList();
  const entry = list.find((r) => samePath(r.path, current));
  return entry && entry.enabled ? entry.path : "";
}

const listeners = new Set<() => void>();
const notify = () => {
  for (const l of listeners) l();
};

// The tour shows the AI Bridge tab as it looks once a repository has been
// chosen - a new user would otherwise only ever see the empty card. In
// memory only: the saved list is not touched and comes straight back.
let tourList: { list: Repository[]; current: string } | null = null;

export function setTourRepositories(list: Repository[], current: string): void {
  tourList = { list, current };
  notify();
}

export function clearTourRepositories(): void {
  if (!tourList) return;
  tourList = null;
  notify();
}

/** Save a repository (deduped by path), switch it on, and make it current. */
export function addRepository(path: string): void {
  const trimmed = path.trim();
  if (!trimmed) return;
  const { list } = readSaved();
  const existing = list.find((r) => samePath(r.path, trimmed));
  const next = existing
    ? list.map((r) => (r === existing ? { ...r, enabled: true } : r))
    : [...list, { path: trimmed, enabled: true }];
  writeList(next, existing ? existing.path : trimmed);
  notify();
}

export function removeRepository(path: string): void {
  const { list, current } = readSaved();
  const next = list.filter((r) => !samePath(r.path, path));
  writeList(next, samePath(current, path) ? "" : current);
  notify();
}

export function setRepositoryEnabled(path: string, enabled: boolean): void {
  const { list, current } = readSaved();
  writeList(
    list.map((r) => (samePath(r.path, path) ? { ...r, enabled } : r)),
    current,
  );
  notify();
}

/** Make a saved repository the current one; an unknown path is ignored. */
export function setCurrentRepository(path: string): void {
  const { list } = readSaved();
  const entry = list.find((r) => samePath(r.path, path));
  if (!entry) return;
  writeList(list, entry.path);
  notify();
}

/** Kept for the single-value callers: a path adds it and makes it current,
 * an empty string deselects (the list itself is left alone). */
export function saveWorkingDir(path: string): void {
  if (path.trim()) {
    addRepository(path);
    return;
  }
  const { list } = readSaved();
  writeList(list, "");
  notify();
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

export function currentPathSnapshot(): string {
  return loadCurrentPath();
}

// `useSyncExternalStore` needs the SAME array back while nothing changed,
// or React sees a new identity on every render and loops. Cache on the
// raw stored text: identical text, identical array.
let cachedRaw: string | null = null;
let cachedList: Repository[] = [];
export function repositoriesSnapshot(): Repository[] {
  const { list } = readList();
  const raw = JSON.stringify(list);
  if (raw !== cachedRaw) {
    cachedRaw = raw;
    cachedList = list;
  }
  return cachedList;
}

export function casesDir(root: string): string {
  return `${root.replace(/[\\/]+$/, "")}\\${CASES_DIR}`;
}

/** Is `path` inside `<root>/.test-cases`? A sibling folder that merely
 * shares the prefix is outside. Matches `workspace::is_inside` in spirit;
 * the Rust side is authoritative where a file is actually written. */
export function isInsideCasesDir(root: string, path: string): boolean {
  const d = norm(casesDir(root));
  const p = norm(path);
  return p === d || p.startsWith(`${d}\\`);
}
