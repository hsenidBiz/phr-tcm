// The Runner's folding tree: one level per folder from suites/ to the
// feature (any depth), then spec, test, as the repo's own `npm run qa` shows
// it. Every branch and test has a tick box; a branch's
// box ticks or clears everything under it, and says how many of its tests
// are ticked. Each level hangs from a guide line, and a test's last result
// is a coloured dot beside its word.

import { memo, useState } from "react";
import type { LastResult, ListedTest } from "../../bindings";
import { Checkbox } from "../../components/ui/checkbox";
import { IconHideDetails, IconShowDetails } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";

export type Branch = {
  id: string;
  label: string;
  children: Branch[];
  /** Only on a spec: its tests, in line order. */
  tests: ListedTest[];
  /** Every test id at or below this branch. */
  ids: string[];
};

/** `rating-methods` as `Rating Methods`. */
export const toTitle = (s: string) =>
  s
    .split("-")
    .filter(Boolean)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(" ");

/** A level's shown name: the first folder uppercased (`SL`), the shared tree
 * as `Common Components`, any other folder title-cased. Mirrors
 * `level_name` in runner.rs. */
export const levelName = (index: number, folder: string) =>
  folder === "common-components" ? "Common Components" : index === 0 ? folder.toUpperCase() : toTitle(folder);

/** The levels a test hangs from, as [id, label] pairs: one per folder of its
 * path, then the spec. */
function levels(t: ListedTest): [string, string][] {
  const out: [string, string][] = t.path.map((folder, i) => [t.path.slice(0, i + 1).join("/"), levelName(i, folder)]);
  out.push([t.file, t.spec]);
  return out;
}

/** The tests, already in tree order, as branches. */
export function buildBranches(tests: ListedTest[]): Branch[] {
  const root: Branch = { id: "", label: "", children: [], tests: [], ids: [] };
  for (const t of tests) {
    let at = root;
    at.ids.push(t.id);
    for (const [id, label] of levels(t)) {
      let next = at.children.find((b) => b.id === id);
      if (!next) {
        next = { id, label, children: [], tests: [], ids: [] };
        at.children.push(next);
      }
      next.ids.push(t.id);
      at = next;
    }
    at.tests.push(t);
  }
  return root.children;
}

const RESULT: Record<LastResult, { text: string; cls: string; dot: string }> = {
  Passed: { text: "passed", cls: "text-success", dot: "bg-success" },
  Failed: { text: "failed", cls: "text-danger", dot: "bg-danger" },
  Flaky: { text: "flaky", cls: "text-warning", dot: "bg-warning" },
  Skipped: { text: "skipped", cls: "text-muted", dot: "bg-faint" },
};

type Props = {
  branches: Branch[];
  ticked: Set<string>;
  onTick: (ids: string[], on: boolean) => void;
};

/** The list under a branch: indented, hanging from a guide line. */
const NESTED = "ml-[0.875rem] border-l border-border pl-2";

/** Memo: the Runner renders for every frame of output, the tree only when
 * its branches, ticks or `onTick` change. */
export default memo(RunnerTree);

function RunnerTree({ branches, ticked, onTick }: Props) {
  const [folded, setFolded] = useState<Set<string>>(() => new Set());
  const toggle = (id: string) =>
    setFolded((f) => {
      const next = new Set(f);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const row = (b: Branch, spec: boolean) => {
    const on = b.ids.filter((id) => ticked.has(id)).length;
    const shut = folded.has(b.id);
    return (
      <li key={b.id}>
        <div className="flex items-center gap-1.5 rounded px-1 py-0.5 hover:bg-surface-2">
          <button
            type="button"
            className="flex size-5 items-center justify-center text-muted transition-colors hover:text-accent [&_svg]:size-3.5"
            aria-label={`${shut ? "Unfold" : "Fold"} ${b.label}`}
            title={shut ? "Unfold" : "Fold"}
            onClick={() => toggle(b.id)}
          >
            {shut ? <IconShowDetails aria-hidden /> : <IconHideDetails aria-hidden />}
          </button>
          <Checkbox
            ariaLabel={`Tick ${b.label}`}
            checked={on > 0 && on === b.ids.length}
            indeterminate={on > 0 && on < b.ids.length}
            onCheckedChange={(v) => onTick(b.ids, v)}
          />
          <span className={cn("min-w-0 truncate text-sm", spec ? "font-mono text-xs text-text" : "font-medium text-text")}>
            {b.label}
          </span>
          <span className={cn("ml-auto shrink-0 text-xs tabular-nums", on ? "text-accent" : "text-faint")}>
            {on ? `${on}/${b.ids.length}` : b.ids.length}
          </span>
        </div>
        {!shut && (
          <ul className={NESTED}>
            {b.children.map((c) => row(c, c.tests.length > 0))}
            {b.tests.map((t) => {
              const r = t.last_result ? RESULT[t.last_result] : null;
              return (
                <li key={t.id} className="flex items-center gap-2 rounded px-1 py-0.5 hover:bg-surface-2">
                  <span className="w-5 shrink-0" aria-hidden />
                  <Checkbox ariaLabel={`Tick ${t.title}`} checked={ticked.has(t.id)} onCheckedChange={(v) => onTick([t.id], v)} />
                  <span className="min-w-0 flex-1 truncate text-sm text-text" title={t.title}>
                    {t.title}
                  </span>
                  {t.ado_id !== null && <span className="shrink-0 font-mono text-xs text-faint">{`ADO ${t.ado_id}`}</span>}
                  <span className="flex w-16 shrink-0 items-center justify-end gap-1.5">
                    {r && (
                      <>
                        <span aria-hidden className={cn("size-2 rounded-full", r.dot)} />
                        <span className={`text-xs ${r.cls}`}>{r.text}</span>
                      </>
                    )}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </li>
    );
  };

  return <ul aria-label="Tests">{branches.map((b) => row(b, b.tests.length > 0))}</ul>;
}
