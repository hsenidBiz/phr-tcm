// The Runner's folding tree: country, side, module, feature, spec, test, as
// the repo's own `npm run qa` shows it. The shared tree (common-components)
// has no side or module. Every branch and test has a tick box; a branch's
// box ticks or clears everything under it.

import { useState } from "react";
import type { LastResult, ListedTest } from "../../bindings";
import { Checkbox } from "../../components/ui/checkbox";
import { IconHideDetails, IconShowDetails } from "../../lib/actionIcons";

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

/** The levels a test hangs from, as [id, label] pairs. */
function levels(t: ListedTest): [string, string][] {
  if (t.country === "common-components") {
    return [
      ["common-components", "Common Components"],
      [`common-components/${t.feature}`, toTitle(t.feature)],
      [t.file, t.spec],
    ];
  }
  const c = t.country;
  return [
    [c, c.toUpperCase()],
    [`${c}/${t.side}`, toTitle(t.side)],
    [`${c}/${t.side}/${t.module}`, toTitle(t.module)],
    [`${c}/${t.side}/${t.module}/${t.feature}`, toTitle(t.feature)],
    [t.file, t.spec],
  ];
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

const RESULT: Record<LastResult, { text: string; cls: string }> = {
  Passed: { text: "passed", cls: "text-success" },
  Failed: { text: "failed", cls: "text-danger" },
  Flaky: { text: "flaky", cls: "text-warning" },
  Skipped: { text: "skipped", cls: "text-muted" },
};

type Props = {
  branches: Branch[];
  ticked: Set<string>;
  onTick: (ids: string[], on: boolean) => void;
};

export default function RunnerTree({ branches, ticked, onTick }: Props) {
  const [folded, setFolded] = useState<Set<string>>(() => new Set());
  const toggle = (id: string) =>
    setFolded((f) => {
      const next = new Set(f);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const row = (b: Branch, depth: number) => {
    const on = b.ids.filter((id) => ticked.has(id)).length;
    const shut = folded.has(b.id);
    return (
      <li key={b.id}>
        <div className="flex items-center gap-2 py-0.5" style={{ paddingLeft: `${depth * 1}rem` }}>
          <button
            type="button"
            className="text-muted transition-colors hover:text-accent"
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
          <span className="text-sm text-text">{b.label}</span>
          <span className="text-xs text-muted">{b.ids.length}</span>
        </div>
        {!shut && (
          <ul>
            {b.children.map((c) => row(c, depth + 1))}
            {b.tests.map((t) => (
              <li key={t.id} className="flex items-center gap-2 py-0.5" style={{ paddingLeft: `${(depth + 1) * 1 + 1.5}rem` }}>
                <Checkbox ariaLabel={`Tick ${t.title}`} checked={ticked.has(t.id)} onCheckedChange={(v) => onTick([t.id], v)} />
                <span className="min-w-0 flex-1 text-sm text-text">{t.title}</span>
                {t.ado_id !== null && <span className="text-xs text-muted">{`ADO ${t.ado_id}`}</span>}
                {t.last_result && (
                  <span className={`text-xs ${RESULT[t.last_result].cls}`}>{RESULT[t.last_result].text}</span>
                )}
              </li>
            ))}
          </ul>
        )}
      </li>
    );
  };

  return <ul aria-label="Tests">{branches.map((b) => row(b, 0))}</ul>;
}
