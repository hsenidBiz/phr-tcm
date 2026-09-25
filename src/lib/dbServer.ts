// The Company database card's local choices: which database the app's own
// database tools use, whether they may write, and the settings for
// registering the company's separate SQL Server MCP server beside ours.
//
// None of it is secret. Each database's login lives in Windows Credential
// Manager, owned by Rust; the webview names a database by its id and never
// holds a password or a connection string. The one exception is a string an
// older version saved here, which `migrateLegacyDbConnection` moves into
// Rust once and then deletes.

import { commands, type DbServerConfig } from "../bindings";
import { logUi } from "./uiLog";

const KEY = "tcm-v2-db-mcp";

/** The id of the database the tools use - absent when none is chosen. */
const SELECTED_KEY = "tcm-v2-db-selected";

/** The create/update/delete switch. "1" only when it is on, and absent
 * otherwise - so a fresh profile and a cleared one both read off, which is
 * the only default a switch like this may have. */
const WRITES_KEY = "tcm-v2-db-writes";

/** What the PHR X registration keeps on this machine. The database it
 * registers is the selected one, added as `db_id` when registering. */
export type DbServerSettings = Omit<DbServerConfig, "db_id">;

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

export const EMPTY_DB_CONFIG: DbServerSettings = {
  exe_path: "",
  db_type: "mssql",
  schema_filter: "",
};

/** Whether the person ever saved a config on this machine - the shipped
 * defaults only fill a form that has never been touched. */
export function hasStoredDbConfig(): boolean {
  try {
    return localStorage.getItem(KEY) != null;
  } catch {
    return false;
  }
}

function readBlob(): Record<string, unknown> | null {
  try {
    const raw = localStorage.getItem(KEY);
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

export function loadDbConfig(): DbServerSettings {
  const parsed = readBlob();
  if (!parsed) return { ...EMPTY_DB_CONFIG };
  // Field-by-field, so a stored blob from an older shape can't leave a
  // required field undefined and blow up the form.
  const str = (v: unknown) => (typeof v === "string" ? v : "");
  return {
    exe_path: str(parsed.exe_path),
    db_type: str(parsed.db_type) || "mssql",
    schema_filter: str(parsed.schema_filter),
  };
}

export function saveDbConfig(config: DbServerSettings): void {
  const next: Record<string, unknown> = {
    exe_path: config.exe_path,
    db_type: config.db_type,
    schema_filter: config.schema_filter,
  };
  // A string that has not moved into Rust yet (its import failed and waits
  // for the next start) rides along, or editing the server path here would
  // throw away the login the person saved before.
  const legacy = legacyConnectionString(readBlob());
  if (legacy) next.connection_string = legacy;
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // storage unavailable -> the settings last for this session only
  }
  notify();
}

/** The local half of "Forget them": the PHR X settings and the choice of
 * database. The saved logins are Rust's, wiped by `forgetDbCredentials`. */
export function forgetDbConfig(): void {
  try {
    localStorage.removeItem(KEY);
    localStorage.removeItem(SELECTED_KEY);
  } catch {
    // nothing to do
  }
  notify();
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

/** Everything the PHR X server needs before it can be registered. Whether
 * the chosen database has a login saved is Rust's to answer. */
export function isDbConfigComplete(c: DbServerSettings, dbId: string): boolean {
  return Boolean(c.exe_path.trim() && c.db_type.trim() && dbId);
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

async function runMigration(): Promise<void> {
  const cs = legacyConnectionString(readBlob());
  if (!cs) return;
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
  saveSelectedDb(id);
  // Re-read rather than reuse the blob from before the await: the PHR X
  // settings may have been edited meanwhile.
  const blob = readBlob();
  if (blob) {
    delete blob.connection_string;
    try {
      localStorage.setItem(KEY, JSON.stringify(blob));
    } catch {
      // storage unavailable -> nothing was stored to remove either
    }
  }
  notify();
}
