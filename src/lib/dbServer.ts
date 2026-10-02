// The Database Read Access card's local choices: which database the app's own
// database tools use, and whether they may write.
//
// None of it is secret. Each database's login lives in Windows Credential
// Manager, owned by Rust; the webview names a database by its id and never
// holds a password or a connection string. The one exception is a string an
// older version saved here, which `migrateLegacyDbConnection` moves into
// Rust once and then deletes.

import { commands } from "../bindings";
import { logUi } from "./uiLog";

/** What older versions kept: the settings for registering a separate
 * database server, and before that a whole connection string. Nothing
 * writes it any more - it is read once, to move a connection string into
 * Rust, and then removed. */
const LEGACY_KEY = "tcm-v2-db-mcp";

/** The id of the database the tools use - absent when none is chosen. */
const SELECTED_KEY = "tcm-v2-db-selected";

/** The create/update/delete switch. "1" only when it is on, and absent
 * otherwise - so a fresh profile and a cleared one both read off, which is
 * the only default a switch like this may have. */
const WRITES_KEY = "tcm-v2-db-writes";

const listeners = new Set<() => void>();

function notify(): void {
  for (const l of listeners) l();
}

/** Subscription so App can re-push the bridge context the moment the
 * database or the write switch changes, instead of the change waiting for
 * the next org/project change. The same pattern the disabled tool set
 * uses. */
export function subscribeDbSettings(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

function readBlob(): Record<string, unknown> | null {
  try {
    const raw = localStorage.getItem(LEGACY_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? (parsed as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** A connection string an older version stored, not yet moved into Rust. */
function legacyConnectionString(blob: Record<string, unknown> | null): string {
  const cs = blob?.connection_string;
  return typeof cs === "string" ? cs.trim() : "";
}

/** The local half of "Forget them": the choice of database, and whatever an
 * older version left under the legacy key. The saved logins are Rust's,
 * wiped by `forgetDbCredentials`. */
export function forgetDbConfig(): void {
  try {
    localStorage.removeItem(LEGACY_KEY);
    localStorage.removeItem(SELECTED_KEY);
  } catch {
    // nothing to do
  }
  notify();
}

/** After one of the person's own databases is removed: a choice that named
 * it names nothing now, so it goes - no database chosen - rather than
 * pointing the tools at an id this build no longer knows. */
export function forgetRemovedDb(id: string): void {
  if (loadSelectedDb() === id) saveSelectedDb("");
}

export function loadSelectedDb(): string {
  try {
    return localStorage.getItem(SELECTED_KEY) ?? "";
  } catch {
    return "";
  }
}

export function saveSelectedDb(id: string): void {
  try {
    if (id) localStorage.setItem(SELECTED_KEY, id);
    else localStorage.removeItem(SELECTED_KEY);
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  notify();
}

/** The database the tools use, or "" when none is chosen. A primitive, so
 * `useSyncExternalStore` is happy to re-read it. */
export function selectedDbSnapshot(): string {
  return loadSelectedDb();
}

/** Whether the assistant may create, update and delete on the chosen
 * database. Off unless it was explicitly switched on. */
export function loadDbWrites(): boolean {
  try {
    return localStorage.getItem(WRITES_KEY) === "1";
  } catch {
    return false;
  }
}

export function saveDbWrites(on: boolean): void {
  try {
    if (on) localStorage.setItem(WRITES_KEY, "1");
    else localStorage.removeItem(WRITES_KEY);
  } catch {
    // storage unavailable -> the choice lasts for this session only
  }
  notify();
}

export function dbWritesSnapshot(): boolean {
  return loadDbWrites();
}

/** Whether a user name is the dev login - the only user the app lets an
 * assistant write as.
 *
 * A mirror of `db::guard::access_for` in Rust, which is the door that
 * actually enforces it. This copy decides only whether the write SWITCH
 * can be moved: a screen that offered it on a read-only login would be
 * promising something the backend refuses. */
export function isDevLoginUser(user: string): boolean {
  return user.trim().toLowerCase().endsWith("_devlogin");
}

/** The same rule, read out of a connection string's user key. */
export function isDevLoginConnection(connectionString: string): boolean {
  for (const part of connectionString.split(";")) {
    const at = part.indexOf("=");
    if (at < 0) continue;
    const key = part.slice(0, at).replace(/\s+/g, "").toLowerCase();
    if (key === "userid" || key === "uid" || key === "user") {
      return isDevLoginUser(part.slice(at + 1));
    }
  }
  return false;
}

let migrating: Promise<void> | null = null;

/** Moves a connection string an older version kept in the webview into
 * Rust, once. Rust answers the database it is - a shipped one, or "own" -
 * and that becomes the selection.
 *
 * Only a SUCCESSFUL import removes the string: Rust's import overwrites the
 * saved "own" login, so a leftover that could run again would one day
 * replace a login saved after it. A failed one leaves everything as it was
 * for the next start to retry. Concurrent callers share one run. */
export function migrateLegacyDbConnection(): Promise<void> {
  migrating ??= runMigration().finally(() => {
    migrating = null;
  });
  return migrating;
}

/** Whether the current selection is a database this build knows, and so
 * a choice the person made. Unsure (the list could not be read) counts as
 * yes: keeping a pick is recoverable, overwriting one is not. */
async function keepsCurrentSelection(): Promise<boolean> {
  const current = loadSelectedDb();
  if (!current) return false;
  try {
    const known = await commands.dbDatabases();
    return known.some((d) => d.id === current);
  } catch {
    return true;
  }
}

/** The legacy key goes once nothing in it is still waiting to move. */
function dropLegacyBlob(): void {
  try {
    localStorage.removeItem(LEGACY_KEY);
  } catch {
    // storage unavailable -> nothing was stored to remove either
  }
}

async function runMigration(): Promise<void> {
  const cs = legacyConnectionString(readBlob());
  if (!cs) {
    // Only an old version's server settings, if anything: nothing reads
    // them now, so they do not stay behind.
    dropLegacyBlob();
    return;
  }
  let id: string;
  try {
    const res = await commands.importLegacyDbConnection(cs);
    if (res.status === "error") {
      // Rust's sentences name keys and stores, never the string's values.
      logUi(`database login: could not move the saved connection: ${res.error}`);
      return;
    }
    id = res.data;
  } catch {
    logUi("database login: could not move the saved connection");
    return;
  }
  // A retry lands on a later start, and the person may have picked a
  // database while the string waited - their pick wins. Only an empty
  // selection, or one naming a database this build does not know, takes
  // the imported one.
  if (!(await keepsCurrentSelection())) saveSelectedDb(id);
  // The string has moved, and nothing else under the key is read any more.
  dropLegacyBlob();
  notify();
}
