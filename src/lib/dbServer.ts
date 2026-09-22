// Settings for the company's SQL Server MCP server, registered alongside
// ours so an assistant can read the database schema and the test cases in
// one session.
//
// The connection string is kept here, in the app's own local settings.
// That is a deliberate call by the owner: the database is reachable only
// from the company network behind a separate sign-in, and is device
// locked, so a string on this machine is not a usable credential
// elsewhere. It is stored so a registration can be repeated for another
// editor without retyping it - and it is written into each AI tool's MCP
// config anyway, which is how MCP passes environment to a server.

import type { DbServerConfig } from "../bindings";

const KEY = "tcm-v2-db-mcp";

/** The create/update/delete switch. "1" only when it is on, and absent
 * otherwise - so a fresh profile and a cleared one both read off, which is
 * the only default a switch like this may have. */
const WRITES_KEY = "tcm-v2-db-writes";

const listeners = new Set<() => void>();

function notify(): void {
  for (const l of listeners) l();
}

/** Subscription so App can re-push the bridge context the moment the
 * connection or the write switch changes, instead of the change waiting
 * for the next org/project change. The same pattern the disabled tool set
 * uses. */
export function subscribeDbSettings(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export const EMPTY_DB_CONFIG: DbServerConfig = {
  exe_path: "",
  db_type: "mssql",
  connection_string: "",
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

export function loadDbConfig(): DbServerConfig {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return { ...EMPTY_DB_CONFIG };
    const parsed = JSON.parse(raw) as Partial<DbServerConfig>;
    // Field-by-field, so a stored blob from an older shape can't leave a
    // required field undefined and blow up the form.
    return {
      exe_path: parsed.exe_path ?? "",
      db_type: parsed.db_type || "mssql",
      connection_string: parsed.connection_string ?? "",
      schema_filter: parsed.schema_filter ?? "",
    };
  } catch {
    return { ...EMPTY_DB_CONFIG };
  }
}

export function saveDbConfig(config: DbServerConfig): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(config));
  } catch {
    // storage unavailable -> the settings last for this session only
  }
  notify();
}

export function forgetDbConfig(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // nothing to do
  }
  notify();
}

/** Whether the assistant may create, update and delete on the chosen
 * connection. Off unless it was explicitly switched on. */
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

/** The connection the database tools use, or "" when none is chosen.
 * A primitive, so `useSyncExternalStore` is happy to re-read it. */
export function dbConnectionSnapshot(): string {
  return loadDbConfig().connection_string.trim();
}

export function dbWritesSnapshot(): boolean {
  return loadDbWrites();
}

/** Whether a connection string signs in as the dev login - the only user
 * the app lets an assistant write as.
 *
 * A mirror of `db::guard::access_for` in Rust, which is the door that
 * actually enforces it. This copy decides only whether the write SWITCH
 * can be moved: a screen that offered it on a read-only connection would
 * be promising something the backend refuses. */
export function isDevLoginConnection(connectionString: string): boolean {
  for (const part of connectionString.split(";")) {
    const at = part.indexOf("=");
    if (at < 0) continue;
    const key = part.slice(0, at).replace(/\s+/g, "").toLowerCase();
    if (key === "userid" || key === "uid" || key === "user") {
      return part.slice(at + 1).trim().toLowerCase().endsWith("_devlogin");
    }
  }
  return false;
}

/** Everything the server needs before it can be registered. */
export function isDbConfigComplete(c: DbServerConfig): boolean {
  return Boolean(c.exe_path.trim() && c.db_type.trim() && c.connection_string.trim());
}

/** A connection string with its password masked, for showing back what is
 * stored without putting the secret on screen. */
export function maskConnectionString(raw: string): string {
  return raw.replace(/(password\s*=)([^;]*)/gi, (_m, key: string) => `${key}••••••`);
}
