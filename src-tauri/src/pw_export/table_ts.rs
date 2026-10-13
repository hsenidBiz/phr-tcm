//! The table checks of a raw spec (`expect_row`, `expect_no_row`,
//! `expect_sorted`, `expect_row_count`) need one helper, `tables`, written
//! once into a spec that uses any of them.
//!
//! It reads the table with the app's own reader (`READ_TABLE_JS`, word for
//! word) through `locator.evaluate`, and judges the read with a TypeScript
//! port of the pure functions of `browser::table`: the same column matching
//! by header, the same case and whitespace rules (Rust's `is_whitespace`,
//! not JavaScript's `\s`), the same number and date readings and the same
//! sentences. A check that a row is absent - and a count of 0 or `at_most` -
//! passes only once the table has held still for `SETTLE_MS`, as in the app.
//!
//! Two shapes, as the app has two ways of waiting:
//!
//! - **A check that passes as soon as it holds** (`expect_row`,
//!   `expect_sorted`, a count with `equals` above 0 or `at_least`) is an
//!   `expect.poll` of `tables.row`, `tables.sorted` or `tables.rows`. The
//!   first two answer `null` when the check holds, else the app's own
//!   failure sentence; `rows` answers the number of rows, or the sentence
//!   when there is no table to count. None of them throws: `expect.poll`
//!   stops at a callback that throws, while the app looks again.
//! - **A check that passes on too few rows** (`expect_no_row`, a count of
//!   0, `at_most`) cannot be a poll: it must hold still for `SETTLE_MS`,
//!   and when time runs out on a table still changing, the last read
//!   decides. `tables.noRow` and `tables.stillRows` run the app's own loop
//!   (`keep_looking`) with the step's timeout, once, and the spec expects
//!   on the answer: `null` or the reason for `noRow`; the count, or the
//!   reason when there was no table, for `stillRows`.
//!
//! A count is compared with Playwright's numeric matchers (`toBe`,
//! `toBeGreaterThanOrEqual`, `toBeLessThanOrEqual`), so a count that is
//! wrong fails with Playwright's own "Expected / Received" message rather
//! than the app's "the table has N rows, not M" - on purpose: the number
//! is the whole story. Only when there is no table to count does the
//! answer, and so the message, become the app's sentence.
//!
//! The table is looked for with `count()` first, which does not wait, so a
//! missing table is said at once ("is not on the page", as the app says
//! it) instead of `evaluate` waiting for it until the step gives up.

use crate::browser::dialogs::MESSAGE_MAX;
use crate::browser::expect::NOT_ON_PAGE;
use crate::browser::table::{READ_TABLE_JS, SETTLE_MS, SETTLE_POLL_MS};

/// What the reader needs to type-check, each said once: `this`, and the
/// header row, which a strict compiler would otherwise type as `null` for
/// good. Nothing else of the constant changes.
const READER_TYPES: [(&str, &str); 2] = [
    ("function() {", "function (this: HTMLTableElement) {"),
    ("let headerRow = null;", "let headerRow: HTMLTableRowElement | null = null;"),
];

/// The app's table reader as a TypeScript function of the element, one
/// line per entry: the body is the constant's, word for word, with only
/// the types of `READER_TYPES` added.
pub fn read_table_ts() -> Vec<String> {
    let mut typed = READ_TABLE_JS.to_string();
    for (js, ts) in READER_TYPES {
        assert!(typed.matches(js).count() == 1, "READ_TABLE_JS no longer says `{js}` once");
        typed = typed.replacen(js, ts, 1);
    }
    let mut lines: Vec<String> = typed.lines().map(str::to_string).collect();
    if let Some(first) = lines.first_mut() {
        *first = format!("const readTable = (e: Element) => ({first}");
    }
    if let Some(last) = lines.last_mut() {
        last.push_str(").call(e as HTMLTableElement);");
    }
    lines
}

/// The pure part: a port of `browser::table`'s judging. It touches no page,
/// so a test can run it as it is.
pub fn table_logic_ts() -> String {
    TABLE_LOGIC_TS.replace("MESSAGE_MAX", &MESSAGE_MAX.to_string())
}

const TABLE_LOGIC_TS: &str = r#"type TableRead = { headers: string[]; rows: string[][] };
type Got<T> = { ok: true; v: T } | { ok: false; why: string };
type SortAs = 'text' | 'number' | 'date' | { date: string };
type Day = [number, number, number];
// Whitespace as Rust's char::is_whitespace has it.
const SPACES = /[\t-\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/;
const LEAD = /^[\t-\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/;
const TRAIL = /[\t-\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+$/;
const trimStart = (s: string): string => s.replace(LEAD, '');
const trimEnd = (s: string): string => s.replace(TRAIL, '');
const trim = (s: string): string => trimEnd(trimStart(s));
const collapse = (s: string): string => trim(s).split(SPACES).join(' ');
const chars = (s: string): string[] => Array.from(s);
const cut = (s: string): string => chars(s).slice(0, MESSAGE_MAX).join('');
const asciiLower = (s: string): string => s.replace(/[A-Z]/g, (c: string) => c.toLowerCase());
const strs = (v: unknown): string[] => (Array.isArray(v) ? v.map((s: unknown) => (typeof s === 'string' ? s : '')) : []);
// The reader's answer, each row padded to the headers; null when not a table.
const fromValue = (v: unknown): TableRead | null => {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) return null;
  const o = v as { headers?: unknown; rows?: unknown };
  if (!('headers' in o) || !('rows' in o)) return null;
  const headers = strs(o.headers);
  const rows = (Array.isArray(o.rows) ? o.rows : []).map((r: unknown) => {
    const cells = strs(r);
    while (cells.length < headers.length) cells.push('');
    return cells;
  });
  return { headers, rows };
};
const column = (read: TableRead, name: string): Got<number> => {
  const want = collapse(name).toLowerCase();
  const i = read.headers.findIndex((h: string) => collapse(h).toLowerCase() === want);
  if (i >= 0) return { ok: true, v: i };
  const list = read.headers.length === 0 ? 'none - it has no header row' : read.headers.map((h: string) => `"${cut(h)}"`).join(', ');
  return { ok: false, why: `the table has no column "${trim(name)}" - its columns are ${list}` };
};
const cell = (read: TableRead, row: number, col: number): string => read.rows[row][col] ?? '';
const cellsWords = (cells: [string, string][]): string => cells.map(([c, t]) => `${trim(c)} "${t}"`).join(', ');
const findRow = (read: TableRead, cells: [string, string][], exact: boolean): Got<number | null> => {
  const cols: [number, string][] = [];
  for (const [c, t] of cells) {
    const i = column(read, c);
    if (!i.ok) return i;
    cols.push([i.v, collapse(t).toLowerCase()]);
  }
  for (let r = 0; r < read.rows.length; r++) {
    const all = cols.every(([i, want]) => {
      const got = collapse(cell(read, r, i)).toLowerCase();
      return exact ? got === want : got.includes(want);
    });
    if (all) return { ok: true, v: r };
  }
  return { ok: true, v: null };
};
const rowCheck = (read: TableRead, cells: [string, string][], exact: boolean): Got<string> => {
  const found = findRow(read, cells, exact);
  if (!found.ok) return found;
  if (found.v !== null) return { ok: true, v: `row ${found.v + 1} has ${cellsWords(cells)}` };
  return { ok: false, why: `no row has ${cellsWords(cells)} - the table has ${read.rows.length} rows` };
};
const noRowCheck = (read: TableRead, cells: [string, string][], exact: boolean): Got<string> => {
  const found = findRow(read, cells, exact);
  if (!found.ok) return found;
  if (found.v !== null) return { ok: false, why: `a row has ${cellsWords(cells)} (row ${found.v + 1})` };
  return { ok: true, v: `no row has ${cellsWords(cells)} - the table has ${read.rows.length} rows` };
};
const stripCurrency = (v: string): string | null => {
  for (const sym of ['$', '\u00a3', '\u20ac']) {
    if (v.startsWith(sym)) return v.slice(sym.length);
  }
  for (const code of ['lkr', 'rs.', 'rs']) {
    const head = chars(v).slice(0, code.length).join('');
    if (asciiLower(head) === code) {
      const rest = v.slice(head.length);
      if (!/^\p{Alphabetic}/u.test(rest)) return rest;
    }
  }
  return null;
};
const parseNumber = (value: string): number | null => {
  let v = trim(value);
  if (v.endsWith('%')) v = trimEnd(v.slice(0, -1));
  let neg = false;
  if (v.startsWith('-')) {
    neg = true;
    v = trimStart(v.slice(1));
  }
  const bare = stripCurrency(v);
  if (bare !== null) {
    v = trimStart(bare);
    if (!neg && v.startsWith('-')) {
      neg = true;
      v = trimStart(v.slice(1));
    }
  }
  const dot = v.indexOf('.');
  const int = dot < 0 ? v : v.slice(0, dot);
  const frac = dot < 0 ? null : v.slice(dot + 1);
  if (int === '' || !/^[0-9,]+$/.test(int)) return null;
  if (int.includes(',')) {
    const groups = int.split(',');
    if (groups[0].length < 1 || groups[0].length > 3 || groups.slice(1).some((g: string) => g.length !== 3)) return null;
  }
  if (frac !== null && !/^[0-9]+$/.test(frac)) return null;
  const plain = int.replace(/,/g, '');
  const n = Number(frac === null ? plain : `${plain}.${frac}`);
  return neg ? -n : n;
};
const MONTHS = ['jan', 'feb', 'mar', 'apr', 'may', 'jun', 'jul', 'aug', 'sep', 'oct', 'nov', 'dec'];
const daysIn = (y: number, m: number): number => {
  if ([1, 3, 5, 7, 8, 10, 12].includes(m)) return 31;
  if ([4, 6, 9, 11].includes(m)) return 30;
  if (m === 2) return (y % 4 === 0 && y % 100 !== 0) || y % 400 === 0 ? 29 : 28;
  return 0;
};
const valid = (y: number, m: number, d: number): Day | null => (m >= 1 && m <= 12 && d >= 1 && d <= daysIn(y, m) ? [y, m, d] : null);
type Part = 'yyyy' | 'MMM' | 'MM' | 'dd' | 'd' | { lit: string };
const formatParts = (fmt: string): Part[] | null => {
  const parts: Part[] = [];
  let rest = fmt;
  while (rest !== '') {
    let part: Part;
    if (rest.startsWith('yyyy')) part = 'yyyy';
    else if (rest.startsWith('MMM')) part = 'MMM';
    else if (rest.startsWith('MM')) part = 'MM';
    else if (rest.startsWith('dd')) part = 'dd';
    else if (rest.startsWith('d')) part = 'd';
    else {
      const c = chars(rest)[0];
      if (/^[A-Za-z]$/.test(c)) return null;
      part = { lit: c };
    }
    parts.push(part);
    rest = rest.slice(typeof part === 'string' ? part.length : part.lit.length);
  }
  const has = (...p: string[]): boolean => parts.some((x: Part) => typeof x === 'string' && p.includes(x));
  return has('yyyy') && has('MM', 'MMM') && has('dd', 'd') ? parts : null;
};
const takeDigits = (s: string, min: number, max: number): [number, string] | null => {
  let n = 0;
  while (n < max && n < s.length && s[n] >= '0' && s[n] <= '9') n++;
  return n < min ? null : [Number(s.slice(0, n)), s.slice(n)];
};
const parseWith = (value: string, parts: Part[]): Day | null => {
  let y: number | null = null;
  let m: number | null = null;
  let d: number | null = null;
  let rest = trim(value);
  for (const p of parts) {
    if (typeof p !== 'string') {
      if (!rest.startsWith(p.lit)) return null;
      rest = rest.slice(p.lit.length);
    } else if (p === 'MMM') {
      const word = chars(rest).slice(0, 3).join('');
      const i = MONTHS.indexOf(asciiLower(word));
      if (i < 0) return null;
      m = i + 1;
      rest = rest.slice(word.length);
    } else {
      const [min, max] = p === 'yyyy' ? [4, 4] : p === 'd' ? [1, 2] : [2, 2];
      const t = takeDigits(rest, min, max);
      if (t === null) return null;
      if (p === 'yyyy') y = t[0];
      else if (p === 'MM') m = t[0];
      else d = t[0];
      rest = t[1];
    }
  }
  if (rest !== '' || y === null || m === null || d === null) return null;
  return valid(y, m, d);
};
const partsOf = (fmt: string): Part[] => formatParts(fmt) as Part[];
const parseUnslashed = (value: string): Day | null => parseWith(value, partsOf('yyyy-MM-dd')) ?? parseWith(value, partsOf('d MMM yyyy'));
const notA = (value: string, col: string, what: string): string => `"${cut(value)}" in ${col} is not a ${what}`;
const cmpDay = (a: Day, b: Day): number => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
const dateKeys = (col: string, values: string[]): Got<Day[]> => {
  const dmy = partsOf('dd/MM/yyyy');
  const mdy = partsOf('MM/dd/yyyy');
  const readAll = (parts: Part[]): Day[] | null => {
    const out: Day[] = [];
    for (const v of values) {
      const k = parseUnslashed(v) ?? parseWith(v, parts);
      if (k === null) return null;
      out.push(k);
    }
    return out;
  };
  const a = readAll(dmy);
  const b = readAll(mdy);
  if (a !== null && b !== null) {
    const order = (k: Day[]): string => k.slice(1).map((x: Day, i: number) => Math.sign(cmpDay(k[i], x))).join(',');
    if (JSON.stringify(a) !== JSON.stringify(b) && order(a) !== order(b)) {
      return { ok: false, why: `the dates in ${col} could be read two ways - give as: "date" a format` };
    }
    return { ok: true, v: a };
  }
  if (a !== null) return { ok: true, v: a };
  if (b !== null) return { ok: true, v: b };
  const bad =
    values.find((v: string) => (parseUnslashed(v) ?? parseWith(v, dmy) ?? parseWith(v, mdy)) === null) ??
    values.find((v: string) => (parseUnslashed(v) ?? parseWith(v, dmy)) === null) ??
    '';
  return { ok: false, why: notA(bad, col, 'date') };
};
// Text in code point order, as Rust compares strings.
const cmpText = (a: string, b: string): number => {
  const x = chars(a);
  const y = chars(b);
  for (let i = 0; i < Math.min(x.length, y.length); i++) {
    const d = (x[i].codePointAt(0) as number) - (y[i].codePointAt(0) as number);
    if (d !== 0) return d;
  }
  return x.length - y.length;
};
const checkSorted = (col: string, values: string[], order: 'ascending' | 'descending', sortAs: SortAs): Got<string> => {
  const kept = values.map((v: string, i: number): [number, string] => [i, trim(v)]).filter(([, v]) => v !== '');
  const texts = kept.map(([, v]) => v);
  let cmp: (i: number) => number;
  if (sortAs === 'text') {
    const keys = texts.map((v: string) => v.toLowerCase());
    cmp = (i: number) => cmpText(keys[i], keys[i + 1]);
  } else if (sortAs === 'number') {
    const keys: number[] = [];
    for (const v of texts) {
      const n = parseNumber(v);
      if (n === null) return { ok: false, why: notA(v, col, 'number') };
      keys.push(n);
    }
    cmp = (i: number) => (keys[i] < keys[i + 1] ? -1 : keys[i] > keys[i + 1] ? 1 : 0);
  } else {
    let keys: Day[];
    if (sortAs === 'date') {
      const got = dateKeys(col, texts);
      if (!got.ok) return got;
      keys = got.v;
    } else {
      const parts = formatParts(sortAs.date);
      if (parts === null) return { ok: false, why: `"${sortAs.date}" is not a date format - use letters such as dd/MM/yyyy` };
      keys = [];
      for (const v of texts) {
        const k = parseWith(v, parts);
        if (k === null) return { ok: false, why: notA(v, col, 'date') };
        keys.push(k);
      }
    }
    cmp = (i: number) => cmpDay(keys[i], keys[i + 1]);
  }
  for (let i = 0; i + 1 < kept.length; i++) {
    const c = cmp(i);
    if (order === 'ascending' ? c > 0 : c < 0) {
      const [ra, a] = kept[i];
      const [rb, b] = kept[i + 1];
      return { ok: false, why: `${col} is not in ${order} order - row ${ra + 1} "${cut(a)}" comes before row ${rb + 1} "${cut(b)}"` };
    }
  }
  return { ok: true, v: `${col} is in ${order} order (${kept.length} values)` };
};
const sortedCheck = (read: TableRead, name: string, order: 'ascending' | 'descending', sortAs: SortAs): Got<string> => {
  const i = column(read, name);
  if (!i.ok) return i;
  return checkSorted(trim(name), read.rows.map((_: string[], r: number) => cell(read, r, i.v)), order, sortAs);
};"#;

/// The part that touches the page: reading the table, the app's waiting
/// loop for the checks that must hold still, and the checks the spec calls.
fn table_io_ts() -> String {
    TABLE_IO_TS
        .replace("SETTLE_POLL_MS", &SETTLE_POLL_MS.to_string())
        .replace("SETTLE_MS", &SETTLE_MS.to_string())
        .replace("NOT_ON_PAGE", NOT_ON_PAGE)
}

const TABLE_IO_TS: &str = r#"type TableLoc = ReturnType<typeof page.locator>;
type Look = { read: TableRead | null; why: string; byLocator: boolean };
// One look, never waiting past `end`: a table that is not there is said at once.
const look = async (loc: TableLoc, end: number | null): Promise<Look> => {
  try {
    if ((await loc.count()) === 0) return { read: null, why: 'NOT_ON_PAGE', byLocator: true };
    const left = end === null ? undefined : Math.max(1, end - Date.now());
    const read = fromValue(await loc.evaluate(readTable, undefined, { timeout: left }));
    return read === null ? { read: null, why: `${loc} is not a table or grid`, byLocator: false } : { read, why: '', byLocator: false };
  } catch (e) {
    return { read: null, why: e instanceof Error ? e.message : String(e), byLocator: true };
  }
};
const said = (loc: TableLoc, l: Look): string => (l.byLocator ? `${loc} ${l.why}` : l.why);
const why = (got: Got<string>): string | null => (got.ok ? null : got.why);
// The app's loop for a check that passes on too few rows: look again until
// it holds and the table has read the same for SETTLE_MS, or time runs out;
// then a check still holding passes on its last read.
const settle = async (loc: TableLoc, timeout: number, judge: (read: TableRead) => string | null): Promise<{ why: string | null; read: TableRead | null }> => {
  const end = Date.now() + timeout;
  let last = `waited ${timeout}ms: ${loc} NOT_ON_PAGE`;
  let lastRead: TableRead | null = null;
  let same: string | null = null;
  let since = 0;
  let holding = false;
  for (;;) {
    const l = await look(loc, end);
    if (l.read !== null) {
      const key = JSON.stringify(l.read);
      if (key !== same) {
        same = key;
        since = Date.now();
      }
      lastRead = l.read;
      const no = judge(l.read);
      if (no === null) {
        if (Date.now() - since >= SETTLE_MS) return { why: null, read: l.read };
        holding = true;
      } else {
        holding = false;
        last = no;
      }
    } else {
      same = null;
      lastRead = null;
      holding = false;
      last = l.byLocator ? `waited ${timeout}ms: ${loc} ${l.why}` : l.why;
    }
    if (Date.now() >= end) return holding ? { why: null, read: lastRead } : { why: last, read: lastRead };
    await new Promise((r) => setTimeout(r, Math.min(SETTLE_POLL_MS, Math.max(0, end - Date.now()))));
  }
};
const row = async (loc: TableLoc, cells: [string, string][], exact: boolean): Promise<string | null> => {
  const l = await look(loc, null);
  return l.read !== null ? why(rowCheck(l.read, cells, exact)) : said(loc, l);
};
const sorted = async (loc: TableLoc, name: string, order: 'ascending' | 'descending', sortAs: SortAs): Promise<string | null> => {
  const l = await look(loc, null);
  return l.read !== null ? why(sortedCheck(l.read, name, order, sortAs)) : said(loc, l);
};
const rows = async (loc: TableLoc): Promise<number | string> => {
  const l = await look(loc, null);
  return l.read !== null ? l.read.rows.length : said(loc, l);
};
const noRow = async (loc: TableLoc, cells: [string, string][], exact: boolean, timeout: number): Promise<string | null> =>
  (await settle(loc, timeout, (read: TableRead) => why(noRowCheck(read, cells, exact)))).why;
// `equals` 0 or `at_most`: the count of the last read, or why there was none.
const stillRows = async (loc: TableLoc, most: number, timeout: number): Promise<number | string> => {
  const got = await settle(loc, timeout, (read: TableRead) =>
    read.rows.length <= most ? null : `the table has ${read.rows.length} rows, not ${most === 0 ? '0' : `at most ${most}`}`,
  );
  return got.read !== null ? got.read.rows.length : (got.why as string);
};
return { row, noRow, sorted, rows, stillRows };"#;

/// The whole helper, declared once as `tables`, one line per entry.
pub fn tables_ts(indent: usize) -> Vec<String> {
    let pad = " ".repeat(indent);
    let inner = format!("{pad}  ");
    let mut out = vec![format!("{pad}// The app's table checks: each answers null when it holds, else why not.")];
    out.push(format!("{pad}const tables = (() => {{"));
    let (logic, io) = (table_logic_ts(), table_io_ts());
    let body = read_table_ts().into_iter().chain(logic.lines().chain(io.lines()).map(str::to_string));
    for l in body {
        out.push(if l.is_empty() { l } else { format!("{inner}{l}") });
    }
    out.push(format!("{pad}}})();"));
    out
}
