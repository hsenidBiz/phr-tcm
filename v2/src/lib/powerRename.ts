/**
 * Bulk title rename: the whole transform, as one pure function.
 *
 * The preview a user approves and the strings that get written are the SAME
 * strings - computed here once, shown, then sent. Nothing recomputes them on
 * the way to Azure DevOps. That is deliberate: a preview that predicts what
 * a second implementation will do is a preview that can be wrong, and "it
 * said one thing and did another" is the failure this app can least afford.
 *
 * No React, no IPC, no Rust - so the rules below are all testable directly.
 */

/** Azure DevOps rejects a longer System.Title; validate.ts uses the same. */
export const MAX_TITLE = 255;

export type Casing = "keep" | "upper" | "lower" | "title";

export type RenameRule = {
  find: string;
  replace: string;
  useRegex: boolean;
  matchCase: boolean;
  /** Replace only the first match in each title rather than every one. */
  firstOnly: boolean;
  casing: Casing;
  prefix: string;
  suffix: string;
  /** What `${n}` counts from, and how many digits it pads to. */
  numberFrom: number;
  numberPad: number;
};

export const EMPTY_RULE: RenameRule = {
  find: "",
  replace: "",
  useRegex: false,
  matchCase: false,
  firstOnly: false,
  casing: "keep",
  prefix: "",
  suffix: "",
  numberFrom: 1,
  numberPad: 1,
};

export type RowStatus =
  /** The rule does not touch this title. */
  | { kind: "unchanged" }
  | { kind: "renamed" }
  /** Cannot be written - Apply stays disabled while any row is blocked. */
  | { kind: "blocked"; reason: string }
  /** Writable, but probably not meant. */
  | { kind: "warned"; reason: string };

export type RenameRow = {
  /** Work item id, or null for a draft that has not been created yet. */
  id: number | null;
  /** Position in the list this row came from.
   *
   * A draft has no id, so a caller matching rows back by TITLE gets it
   * wrong the moment a rename makes two of them the same - undo then put
   * the old title on whichever one it happened to reach first, leaving
   * titles attached to the wrong steps. The position does not change under
   * a rename, so it is the identity that survives one. */
  index: number;
  before: string;
  after: string;
  status: RowStatus;
};

export type RenamePreview = {
  rows: RenameRow[];
  /** The regex the user typed could not be parsed. Rows are left untouched. */
  error: string | null;
  renamed: number;
  blocked: number;
  warned: number;
};

/** Everything RegExp treats as syntax, so a plain find means what it says. */
function escapeLiteral(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Title Case that does not vandalise acronyms.
 *
 * A test case title is full of them - "API", "PBI", "HRM", "SSO" - and the
 * usual implementation lowercases the tail of every word, turning "API" into
 * "Api". So a word that already carries a capital is left exactly as it is;
 * only all-lowercase words are capitalised.
 */
function titleCase(s: string): string {
  return s.replace(/\S+/g, (word) =>
    /[A-Z]/.test(word) ? word : word.charAt(0).toUpperCase() + word.slice(1),
  );
}

function applyCasing(s: string, casing: Casing): string {
  switch (casing) {
    case "upper":
      return s.toUpperCase();
    case "lower":
      return s.toLowerCase();
    case "title":
      return titleCase(s);
    case "keep":
      return s;
  }
}

/** `${n}` anywhere in the composed title - replace text, prefix or suffix. */
function applyNumber(s: string, index: number, rule: RenameRule): string {
  if (!s.includes("${n}")) return s;
  const n = rule.numberFrom + index;
  const pad = Math.max(1, Math.min(12, Math.floor(rule.numberPad) || 1));
  return s.split("${n}").join(String(n).padStart(pad, "0"));
}

/** True when the rule would leave every title exactly as it is. */
export function ruleIsEmpty(rule: RenameRule): boolean {
  return (
    rule.find === "" &&
    rule.prefix === "" &&
    rule.suffix === "" &&
    rule.casing === "keep"
  );
}

/**
 * One title through the whole rule, in a fixed order so combining the parts
 * stays predictable: find/replace, then casing, then prefix and suffix, then
 * the number. Casing runs BEFORE the affixes so an UPPER rule cannot shout a
 * prefix the user typed in mixed case.
 */
function renameOne(title: string, index: number, rule: RenameRule, re: RegExp | null): string {
  let out = title;
  if (re) out = out.replace(re, rule.replace);
  out = applyCasing(out, rule.casing);
  out = `${rule.prefix}${out}${rule.suffix}`;
  return applyNumber(out, index, rule);
}

/**
 * Build the rule's regex, or null when there is no find term.
 *
 * Non-regex finds go through RegExp too, escaped - so "match case" and
 * "first only" behave identically whether or not the box is ticked, instead
 * of one code path using indexOf and the other not.
 */
function buildRegex(rule: RenameRule): { re: RegExp | null; error: string | null } {
  if (rule.find === "") return { re: null, error: null };
  const flags = (rule.matchCase ? "" : "i") + (rule.firstOnly ? "" : "g");
  const source = rule.useRegex ? rule.find : escapeLiteral(rule.find);
  try {
    return { re: new RegExp(source, flags), error: null };
  } catch (e) {
    return { re: null, error: e instanceof Error ? e.message : "Invalid pattern" };
  }
}

/**
 * The rule applied to a list of titles, in the order they are displayed -
 * which is the order `${n}` counts in, so the numbers on screen are the
 * numbers that get written.
 *
 * `otherTitles` are titles NOT being renamed that a result could collide
 * with (the rest of the PBI). A collision is a warning, never a block: this
 * app treats a matching title as a duplicate and never as an update, so it
 * is legal, just rarely intended.
 */
export function previewRename(
  cases: { id: number | null; title: string }[],
  rule: RenameRule,
  otherTitles: string[] = [],
): RenamePreview {
  const { re, error } = buildRegex(rule);
  if (error) {
    return {
      rows: cases.map((c, i) => ({
        id: c.id,
        index: i,
        before: c.title,
        after: c.title,
        status: { kind: "unchanged" } as RowStatus,
      })),
      error,
      renamed: 0,
      blocked: 0,
      warned: 0,
    };
  }

  const seen = new Map<string, number>();
  for (const t of otherTitles) {
    const k = t.trim().toLowerCase();
    seen.set(k, (seen.get(k) ?? 0) + 1);
  }

  const draft = cases.map((c, i) => ({
    id: c.id,
    index: i,
    before: c.title,
    after: re || !ruleIsEmpty(rule) ? renameOne(c.title, i, rule, re) : c.title,
  }));

  // Collisions are counted across the WHOLE result, so two rows that rename
  // to the same thing are both flagged rather than just the second.
  for (const d of draft) {
    const k = d.after.trim().toLowerCase();
    seen.set(k, (seen.get(k) ?? 0) + 1);
  }

  let renamed = 0;
  let blocked = 0;
  let warned = 0;
  const rows: RenameRow[] = draft.map((d) => {
    const trimmed = d.after.trim();
    let status: RowStatus;
    if (trimmed === "") {
      status = { kind: "blocked", reason: "A title cannot be empty." };
      blocked++;
    } else if (trimmed.length > MAX_TITLE) {
      status = {
        kind: "blocked",
        reason: `${trimmed.length} characters - Azure DevOps allows ${MAX_TITLE}.`,
      };
      blocked++;
    } else if (d.after === d.before) {
      status = { kind: "unchanged" };
    } else if ((seen.get(trimmed.toLowerCase()) ?? 0) > 1) {
      status = { kind: "warned", reason: "Another test case already has this title." };
      warned++;
      renamed++;
    } else {
      status = { kind: "renamed" };
      renamed++;
    }
    return { ...d, status };
  });

  return { rows, error: null, renamed, blocked, warned };
}

/** The rows that would actually be written - what Apply sends. */
export function rowsToApply(preview: RenamePreview): RenameRow[] {
  return preview.rows.filter(
    (r) => r.status.kind === "renamed" || r.status.kind === "warned",
  );
}

/** Whether Apply may run at all. */
export function canApply(preview: RenamePreview): boolean {
  return preview.error === null && preview.blocked === 0 && preview.renamed > 0;
}

/**
 * What was renamed, kept so it can be put back.
 *
 * There is no undo in Azure DevOps here - this app issues no DELETE and does
 * not roll back revisions - so undo is simply the same write in the opposite
 * direction, from titles captured before the change. It lasts as long as the
 * screen does; that covers the case it exists for, which is noticing a
 * mistyped pattern seconds after applying it.
 */
export type RenameUndo = {
  /** id -> the title it had before. Drafts are keyed by their old title. */
  entries: { id: number | null; from: string; to: string }[];
  at: number;
};

export function undoFromRows(rows: RenameRow[], at: number): RenameUndo {
  return {
    entries: rows.map((r) => ({ id: r.id, from: r.after, to: r.before })),
    at,
  };
}
