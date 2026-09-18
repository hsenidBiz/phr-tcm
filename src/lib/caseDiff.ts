import type { Step, TestCase, TestCaseFull } from "../bindings";
import { splitTags } from "../components/ui/tagfield";

/** What an UPDATE will actually change on the server - mirrors the Rust
 * update rule: blank module/preconditions/tags never overwrite existing
 * values (they land in blankSkipped); title/status/steps always write.
 * Step detail carries the old/new content so the review can render a
 * git-style -/+ diff. */
export type StepDiff = {
  index: number;
  kind: "added" | "removed" | "changed";
  /** The server's current step (absent for added). */
  old?: Step;
  /** The queued step (absent for removed). */
  new?: Step;
};

export type CaseDiff = {
  fields: { name: string; old: string; new: string }[];
  steps: {
    added: number;
    removed: number;
    changed: number;
    /** Steps whose text is unchanged but whose stored `type` is wrong for
     * their Expected Result (an ActionStep that has one). The upload repairs
     * the type in place, so this counts as a change. */
    retyped: number;
    /** Which steps those are, and the type each moves from and to - what
     * the open diff lists, since a retyped step has no text change to show. */
    retypedDetail: { index: number; from: string; to: string }[];
    detail: StepDiff[];
  };
  blankSkipped: string[];
  noop: boolean;
};

/** The `type` Azure DevOps gives a step - mirrors `step_type` in Rust. */
function stepTypeFor(expected: string): "ValidateStep" | "ActionStep" {
  return expected.trim() ? "ValidateStep" : "ActionStep";
}

/** Two steps say the same thing: the same text, and the same Shared Steps
 * reference (or neither has one). A shared step has no text, so the
 * reference is what tells two apart. */
export function sameStep(a: Step, b: Step): boolean {
  return a.action === b.action && a.expected === b.expected && (a.shared ?? null) === (b.shared ?? null);
}

/** Each top-level node's `type` in document order, from the raw Steps XML
 * the server holds - "" for a Shared Steps reference, whose nested steps
 * are part of it and never counted. Mirrors `parse_step_types` in Rust.
 * Empty when there is no XML to read - a stub, the tour's sample data, or
 * a fixture that never carried the field. */
function storedStepTypes(stepsXml: string | undefined): string[] {
  const out: string[] = [];
  const topLevel = /<compref\b[^>]*?\/>|<compref\b[^>]*>[\s\S]*?<\/compref>|<step\b([^>]*)>/g;
  for (const m of (stepsXml ?? "").matchAll(topLevel)) {
    out.push(m[0].startsWith("<compref") ? "" : (/\btype="([^"]*)"/.exec(m[1])?.[1] ?? ""));
  }
  return out;
}

function tagsEqual(a: string, b: string): boolean {
  const norm = (v: string) =>
    splitTags(v)
      .map((t) => t.toLowerCase())
      .sort()
      .join("\u0000");
  return norm(a) === norm(b);
}

/** Which optional custom fields this project actually has. A project with
 * no Module field has `moduleRef: null`, and the update skips that field
 * entirely - so the review must not promise a change that cannot happen. */
export type FieldRefs = { moduleRef: string | null; preconditionsRef: string | null };

export function diffCase(
  queued: TestCase,
  current: TestCaseFull,
  refs: FieldRefs = { moduleRef: "", preconditionsRef: "" },
): CaseDiff {
  const fields: CaseDiff["fields"] = [];
  const blankSkipped: string[] = [];

  // Always-written fields.
  if (queued.title.trim() !== current.title.trim()) {
    fields.push({ name: "Title", old: current.title, new: queued.title });
  }
  if (queued.automation_status !== current.automation_status) {
    fields.push({
      name: "Automation status",
      old: current.automation_status,
      new: queued.automation_status,
    });
  }

  // Blank-skip fields: a blank queued value intentionally leaves the
  // server value alone (only noted when there IS a server value to keep).
  //
  // Module and Preconditions live in custom fields that not every project
  // has. When the project has none, the write skips the field and the value
  // is dropped on the floor - so listing it as a pending change would be a
  // promise the submit cannot keep. It is NOT blank-skipped either: that
  // list means "your blank is preserving a server value", which is a
  // different thing to say.
  const blankable: [string, string, string][] = [
    ["Tags", queued.tags, current.tags],
    ...(refs.moduleRef === null
      ? []
      : ([["Module", queued.module_value, current.module_value]] as [string, string, string][])),
    ...(refs.preconditionsRef === null
      ? []
      : ([
          ["Preconditions", queued.preconditions, current.preconditions],
        ] as [string, string, string][])),
  ];
  for (const [name, q, c] of blankable) {
    if (!q.trim()) {
      if (c.trim()) blankSkipped.push(name);
      continue;
    }
    const equal = name === "Tags" ? tagsEqual(q, c) : q.trim() === c.trim();
    if (!equal) fields.push({ name, old: c, new: q });
  }

  // Steps compare positionally: index-by-index changed, extras added or
  // removed. Steps always write, so any difference is a real change.
  const detail: CaseDiff["steps"]["detail"] = [];
  const max = Math.max(queued.steps.length, current.steps.length);
  for (let i = 0; i < max; i++) {
    const q = queued.steps[i];
    const c = current.steps[i];
    if (q && !c) detail.push({ index: i, kind: "added", new: q });
    else if (!q && c) detail.push({ index: i, kind: "removed", old: c });
    else if (q && c && !sameStep(q, c)) {
      detail.push({ index: i, kind: "changed", old: c, new: q });
    }
  }
  // A step with unchanged text can still need writing: every step this
  // app wrote before 1.25.1 was an ActionStep, whatever its Expected
  // Result, and the upload now repairs that in place. Counted only when
  // the stored XML lines up with the parsed steps one-for-one - anything
  // else is not something to claim from a regex.
  const storedTypes = storedStepTypes(current.steps_xml);
  const touched = new Set(detail.map((d) => d.index));
  const retypedDetail: CaseDiff["steps"]["retypedDetail"] = [];
  if (storedTypes.length === current.steps.length) {
    queued.steps.forEach((q, i) => {
      if (q.shared != null) return; // a reference has no type of its own
      const to = stepTypeFor(q.expected);
      if (i < storedTypes.length && !touched.has(i) && storedTypes[i] !== to) {
        retypedDetail.push({ index: i, from: storedTypes[i], to });
      }
    });
  }
  const retyped = retypedDetail.length;

  const steps = {
    added: detail.filter((d) => d.kind === "added").length,
    removed: detail.filter((d) => d.kind === "removed").length,
    changed: detail.filter((d) => d.kind === "changed").length,
    retyped,
    retypedDetail,
    detail,
  };

  return {
    fields,
    steps,
    blankSkipped,
    noop: fields.length === 0 && detail.length === 0 && retyped === 0,
  };
}

/** "Steps 1, 2 and 5" - 1-based, for people. */
function stepList(indices: number[]): string {
  const n = indices.map((i) => String(i + 1));
  const joined = n.length === 1 ? n[0] : `${n.slice(0, -1).join(", ")} and ${n[n.length - 1]}`;
  return `${n.length === 1 ? "Step" : "Steps"} ${joined}`;
}

/** The open diff's sentences for step-type repairs, one per target type. */
export function retypedLines(d: CaseDiff): string[] {
  const lines: string[] = [];
  const toValidate = d.steps.retypedDetail.filter((r) => r.to === "ValidateStep").map((r) => r.index);
  const toAction = d.steps.retypedDetail.filter((r) => r.to === "ActionStep").map((r) => r.index);
  if (toValidate.length) {
    lines.push(
      `${stepList(toValidate)} ${toValidate.length === 1 ? "becomes a validation step, because it has" : "become validation steps, because they have"} an Expected Result.`,
    );
  }
  if (toAction.length) {
    lines.push(
      `${stepList(toAction)} ${toAction.length === 1 ? "becomes an action step, because it has" : "become action steps, because they have"} no Expected Result.`,
    );
  }
  return lines;
}

/** One-line summary for the review row.
 *
 * Phrased as an invitation - "Click to view 2 fields · 3 steps changing" -
 * because it IS a button that opens the diff, and "2 fields · 3 steps
 * change" read as a statement of fact. A reviewer who does not know the
 * detail is one click away goes and opens the work item instead. */
export function diffSummary(d: CaseDiff): string {
  const parts: string[] = [];
  if (d.fields.length) parts.push(`${d.fields.length} field${d.fields.length === 1 ? "" : "s"}`);
  const stepCount = d.steps.added + d.steps.removed + d.steps.changed;
  if (stepCount) parts.push(`${stepCount} step${stepCount === 1 ? "" : "s"}`);
  if (d.steps.retyped) parts.push(`${d.steps.retyped} step type${d.steps.retyped === 1 ? "" : "s"}`);
  return parts.length ? `Click to view ${parts.join(" · ")} changing` : "";
}
