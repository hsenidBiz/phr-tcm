/**
 * Build a SQL Server connection string from fields, and read one back
 * into fields - so the AI Bridge tab can ask for host, database, user and
 * password instead of expecting the whole string typed by hand.
 *
 * The STORED value stays the single string (DbServerConfig is unchanged;
 * it is what the MCP server receives as CONNECTION_STRING). These two
 * functions are the seam: parse() must read anything build() writes, and
 * anything it does not recognise - Encrypt, timeouts, options this app
 * has never heard of - survives in `extras` and is written back verbatim.
 * A builder that silently dropped an unknown option would break the one
 * database it was built for.
 */

export type ConnFields = {
  host: string;
  /** Blank means the driver's default (1433). */
  port: string;
  database: string;
  user: string;
  password: string;
  /** Company default: the DB server's certificate is self-signed. */
  trustCert: boolean;
  /** Unrecognised `key=value` pairs, in their original order. */
  extras: string;
};

export const EMPTY_FIELDS: ConnFields = {
  host: "",
  port: "",
  database: "",
  user: "",
  password: "",
  trustCert: true,
  extras: "",
};

export function parseConnString(raw: string): ConnFields {
  const f: ConnFields = { ...EMPTY_FIELDS, trustCert: false };
  const extras: string[] = [];
  for (const part of raw.split(";")) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    const eq = trimmed.indexOf("=");
    if (eq < 0) {
      extras.push(trimmed);
      continue;
    }
    const key = trimmed.slice(0, eq).trim().toLowerCase();
    const value = trimmed.slice(eq + 1).trim();
    switch (key) {
      case "server":
      case "data source": {
        // "host,port" is the SQL Server shape; a bare host is common too.
        const comma = value.lastIndexOf(",");
        if (comma >= 0 && /^\d+$/.test(value.slice(comma + 1).trim())) {
          f.host = value.slice(0, comma).trim();
          f.port = value.slice(comma + 1).trim();
        } else {
          f.host = value;
        }
        break;
      }
      case "database":
      case "initial catalog":
        f.database = value;
        break;
      case "user id":
      case "uid":
        f.user = value;
        break;
      case "password":
      case "pwd":
        f.password = value;
        break;
      case "trustservercertificate":
        f.trustCert = value.toLowerCase() === "true" || value.toLowerCase() === "yes";
        break;
      default:
        extras.push(trimmed);
    }
  }
  f.extras = extras.join("; ");
  return f;
}

export function buildConnString(f: ConnFields): string {
  // The trust flag rides along with a connection; alone it is not one, and
  // emitting it for an untouched form would make an empty config look set.
  const substantive =
    f.host.trim() || f.database.trim() || f.user.trim() || f.password || f.extras.trim();
  if (!substantive) return "";
  const parts: string[] = [];
  if (f.host.trim()) {
    parts.push(`Server=${f.host.trim()}${f.port.trim() ? `,${f.port.trim()}` : ""}`);
  }
  if (f.database.trim()) parts.push(`Database=${f.database.trim()}`);
  if (f.user.trim()) parts.push(`User Id=${f.user.trim()}`);
  if (f.password) parts.push(`Password=${f.password}`);
  if (f.trustCert) parts.push("TrustServerCertificate=True");
  for (const e of f.extras.split(";")) {
    if (e.trim()) parts.push(e.trim());
  }
  return parts.length ? parts.join(";") + ";" : "";
}

/** The builder can only faithfully represent a string it can read back.
 * True when parse->build loses nothing the original said - the form
 * starts in field mode when this holds, and in single-string mode when
 * it does not, so an exotic string is never silently rewritten. */
export function isRepresentable(raw: string): boolean {
  if (!raw.trim()) return true;
  const norm = (s: string) =>
    s
      .split(";")
      .map((p) => p.trim().toLowerCase().replace(/\s*=\s*/, "="))
      .filter(Boolean)
      .sort()
      .join(";");
  return norm(buildConnString(parseConnString(raw))) === norm(raw);
}
