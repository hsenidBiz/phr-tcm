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

export const EMPTY_DB_CONFIG: DbServerConfig = {
  exe_path: "",
  db_type: "mssql",
  connection_string: "",
  schema_filter: "",
};

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
}

export function forgetDbConfig(): void {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // nothing to do
  }
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
