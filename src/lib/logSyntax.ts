/**
 * Colouring for the app log, after VS Code's "Log" language mode: the
 * level tag stands out by severity, and the values a reader scans for -
 * numbers, status codes, host names, ids - stand out from the prose around
 * them. Pure, so the Settings log view only maps kinds to theme colours.
 */

export type LogTokenKind = "plain" | "number" | "host" | "guid" | "constant" | "date" | "arrow" | LogLevel;
export type LogLevel = "error" | "warn" | "info" | "debug";
export type LogToken = { text: string; kind: LogTokenKind };

/** A level word as it appears in a message or the level column. */
export function levelOf(word: string): LogLevel | null {
  const w = word.toLowerCase();
  if (w === "error" || w === "err" || w === "fatal" || w === "fail" || w === "failure") return "error";
  if (w === "warn" || w === "warning") return "warn";
  if (w === "info" || w === "information" || w === "notice") return "info";
  if (w === "debug" || w === "dbg" || w === "trace" || w === "verbose") return "debug";
  return null;
}

// One pass, first match wins, in this order - a GUID is not three numbers,
// a date is not a subtraction, and a host name's digits are not numbers.
const PATTERN = new RegExp(
  [
    // 2026-09-11, 2026-09-11 04:11:18, 04:11:18.123, 2026-09-11T04:11:18Z
    String.raw`(?<date>\b\d{4}-\d{2}-\d{2}(?:[T ]\d{2}:\d{2}(?::\d{2}(?:[.,]\d+)?)?(?:Z|[+-]\d{2}:?\d{2})?)?\b|\b\d{2}:\d{2}:\d{2}(?:[.,]\d+)?\b)`,
    String.raw`(?<guid>\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b)`,
    // A host name, optionally behind a scheme: only the host is coloured,
    // the path after it stays plain, the way VS Code shows it.
    String.raw`(?<host>\b(?:https?:\/\/)?(?:[a-zA-Z0-9-]+\.)+(?:com|net|org|io|dev|app|ms|azure|visualstudio|microsoft|local|localhost)\b)`,
    String.raw`(?<level>\b(?:ERROR|ERR|FATAL|WARN|WARNING|INFO|DEBUG|TRACE|VERBOSE)\b)`,
    String.raw`(?<constant>\b(?:true|false|null|undefined|None)\b)`,
    String.raw`(?<arrow>->|=>|<-)`,
    // A dotted version (1.25.5) is one number, not three.
    String.raw`(?<number>(?<![\w.])-?\d+(?:\.\d+)*(?![\w]))`,
  ].join("|"),
  "g",
);

export function tokenizeLog(message: string): LogToken[] {
  const out: LogToken[] = [];
  let at = 0;
  const push = (text: string, kind: LogTokenKind) => {
    if (!text) return;
    const last = out[out.length - 1];
    if (last && last.kind === kind && kind === "plain") last.text += text;
    else out.push({ text, kind });
  };
  for (const m of message.matchAll(PATTERN)) {
    const g = m.groups ?? {};
    const start = m.index ?? 0;
    push(message.slice(at, start), "plain");
    if (g.level) push(m[0], levelOf(m[0]) ?? "plain");
    else if (g.date) push(m[0], "date");
    else if (g.guid) push(m[0], "guid");
    else if (g.host) push(m[0], "host");
    else if (g.constant) push(m[0], "constant");
    else if (g.arrow) push(m[0], "arrow");
    else push(m[0], "number");
    at = start + m[0].length;
  }
  push(message.slice(at), "plain");
  return out;
}

/** Theme colours per kind - tokens only, so every theme preset reads right.
 * The theme has no blue, so the accent stands in where VS Code uses it. */
export const LOG_KIND_CLASS: Record<LogTokenKind, string> = {
  plain: "text-text",
  number: "text-accent",
  host: "text-accent",
  guid: "text-accent",
  constant: "text-accent",
  date: "text-muted",
  arrow: "text-faint",
  error: "text-danger",
  warn: "text-warning",
  info: "text-success",
  debug: "text-warning/70",
};
