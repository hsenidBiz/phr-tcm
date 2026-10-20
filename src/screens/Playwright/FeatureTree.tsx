// The Overview's folder tree: what sits under the clone's suites folder, one
// level per folder (any depth), each feature beneath its folder with its
// spec and test case counts. Anything with something under it (a folder, or
// a feature that holds features) shows the totals of everything under it,
// and folds like a branch of the Runner's tree; each level hangs from the
// same guide line (RunnerTree's NESTED). A nested list, as the Runner's is:
// a screen reader hears each level, and the fold buttons are in the Tab
// order, each named by its folder's full path (two folders can share a
// name) with aria-expanded saying whether it is open.

import { useState } from "react";
import type { RepoStructure } from "../../bindings";
import { IconHideDetails, IconRepoFeature, IconRepoFolder, IconShowDetails } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { NESTED } from "./RunnerTree";

type Feature = RepoStructure["features"][number];

/** What a feature that is the suites folder itself (no folders under
 * suites) is called, at the root. */
export const SUITES = "suites";

export type FolderNode = {
  /** The path under suites, `/`-joined ("" for suites itself). */
  id: string;
  name: string;
  /** Set when this folder is a feature. */
  feature: Feature | null;
  children: FolderNode[];
  /** Specs and test cases at or below this folder. */
  specs: number;
  cases: number;
};

/** The features, in their order, as folders nested under their parents. */
export function buildFolders(features: Feature[]): FolderNode[] {
  const root: FolderNode = { id: "", name: "", feature: null, children: [], specs: 0, cases: 0 };
  for (const f of features) {
    if (f.path.length === 0) {
      root.children.push({ id: "", name: SUITES, feature: f, children: [], specs: f.spec_count, cases: f.case_count });
      continue;
    }
    let at = root;
    f.path.forEach((name, i) => {
      const id = f.path.slice(0, i + 1).join("/");
      let next = at.children.find((c) => c.id === id);
      if (!next) {
        next = { id, name, feature: null, children: [], specs: 0, cases: 0 };
        at.children.push(next);
      }
      next.specs += f.spec_count;
      next.cases += f.case_count;
      at = next;
    });
    at.feature = f;
  }
  return root.children;
}

const plural = (n: number, one: string, many: string) => (n === 1 ? one : many);

/** A count in its column; a screen reader hears what it counts. */
function Count({ n, one, many, className }: { n: number; one: string; many: string; className: string }) {
  return (
    <span className={cn("w-14 shrink-0 text-right text-xs tabular-nums", className)}>
      {n}
      <span className="sr-only">{` ${plural(n, one, many)}`}</span>
    </span>
  );
}

export default function FeatureTree({ features }: { features: Feature[] }) {
  const [folded, setFolded] = useState<Set<string>>(() => new Set());
  const toggle = (id: string) =>
    setFolded((f) => {
      const next = new Set(f);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const row = (n: FolderNode) => {
    const shut = folded.has(n.id);
    const isFeature = n.feature !== null;
    /** A feature on its own shows its counts; anything with something under
     * it shows the totals, faint. */
    const leaf = isFeature && n.children.length === 0;
    const Glyph = isFeature ? IconRepoFeature : IconRepoFolder;
    return (
      <li key={n.id}>
        <div className="flex items-center gap-1.5 rounded px-1 py-0.5 hover:bg-surface-2">
          {n.children.length > 0 ? (
            <button
              type="button"
              className="flex size-5 shrink-0 items-center justify-center text-muted transition-colors hover:text-accent [&_svg]:size-3.5"
              aria-label={n.id}
              aria-expanded={!shut}
              title={shut ? "Unfold" : "Fold"}
              onClick={() => toggle(n.id)}
            >
              {shut ? <IconShowDetails aria-hidden /> : <IconHideDetails aria-hidden />}
            </button>
          ) : (
            <span className="w-5 shrink-0" aria-hidden />
          )}
          <Glyph aria-hidden className={cn("size-4 shrink-0", isFeature ? "text-accent" : "text-muted")} />
          <span
            className={cn("min-w-0 flex-1 truncate text-sm text-text", isFeature && "font-medium")}
            title={n.id || SUITES}
          >
            {n.name}
          </span>
          <Count n={n.specs} one="spec" many="specs" className={leaf ? "text-text" : "text-faint"} />
          <Count
            n={n.cases}
            one="test case"
            many="test cases"
            className={leaf && n.cases ? "text-text" : "text-faint"}
          />
        </div>
        {n.children.length > 0 && !shut && <ul className={NESTED}>{n.children.map(row)}</ul>}
      </li>
    );
  };

  return (
    <div className="overflow-hidden rounded-lg border border-border bg-surface">
      <div aria-hidden className="flex items-center gap-1.5 border-b border-border bg-surface-2 px-3 py-2 text-xs font-medium text-muted">
        <span className="min-w-0 flex-1">Folder</span>
        <span className="w-14 shrink-0 text-right">Specs</span>
        <span className="w-14 shrink-0 text-right">Cases</span>
      </div>
      <ul aria-label="Features" className="p-2">
        {buildFolders(features).map(row)}
      </ul>
    </div>
  );
}
