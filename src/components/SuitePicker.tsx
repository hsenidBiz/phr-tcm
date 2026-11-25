import { useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { commands, type PlanWithSuites } from "../bindings";
import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";
import { IconSuite } from "../lib/actionIcons";
import { unwrap } from "../lib/ipc";
import { spansProjects } from "../lib/projectScope";
import { matchesQuery } from "../lib/search";
import type { SuiteScope } from "../lib/scope";
import { usePlansAll } from "../lib/usePlansAll";
import ProjectChip from "./ProjectChip";
import { SearchInput } from "./ui/search-input";

const SHOWN_MAX = 50;

type Entry = { project: string; planId: number; planName: string; suiteId: number; name: string; path: string; folder: boolean };

/** Every suite of the plans, each with its plan > folder > suite path. */
function entriesOf(project: string, plans: PlanWithSuites[]): Entry[] {
  return plans.flatMap(({ plan, suites }) => {
    const byId = new Map(suites.map((s) => [s.id, s]));
    const parents = new Set(suites.map((s) => s.parent_id).filter((id): id is number => id != null));
    const pathOf = (id: number): string[] => {
      const out: string[] = [];
      const seen = new Set<number>();
      for (let s = byId.get(id); s && !seen.has(s.id); s = s.parent_id == null ? undefined : byId.get(s.parent_id)) {
        seen.add(s.id);
        out.unshift(s.name);
      }
      return out;
    };
    return suites.map((s) => ({
      project,
      planId: plan.id,
      planName: plan.name,
      suiteId: s.id,
      name: s.name,
      path: [plan.name, ...pathOf(s.id)].join(" › "),
      folder: parents.has(s.id),
    }));
  });
}

/** The top bar's Suite mode: finds a test suite by name among the plans
 * Search Suites browses (the same cached plan trees, read the first time
 * the box is used) and lists each as plan › folder › suite. */
export default function SuitePicker({
  org,
  project,
  suite,
  onChange,
  allProjects,
}: {
  org: string;
  project: string;
  suite: SuiteScope | null;
  onChange: (suite: SuiteScope | null) => void;
  /** The projects show together: suites of every project are listed. */
  allProjects: boolean;
}) {
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  // Reading every plan's suites is the app's costliest call: nothing is
  // asked until the person uses the box.
  const [wanted, setWanted] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);

  const single = useQuery({
    queryKey: ["plans-suites", org, project],
    ...persistentQuery({
      key: cacheKeys.plansSuites(org, project),
      fetcher: () => unwrap(commands.listPlansWithSuites(org, project)),
      ...CACHE.structure,
    }),
    enabled: wanted && !allProjects && Boolean(org && project),
    gcTime: 60 * 60_000,
    retry: false,
  });
  const { plansAll, scanNote } = usePlansAll(org, wanted && allProjects);

  const entries = useMemo(() => {
    if (allProjects) return (plansAll.data?.groups ?? []).flatMap((g) => entriesOf(g.project, g.plans));
    return entriesOf(project, single.data ?? []);
  }, [allProjects, plansAll.data, single.data, project]);
  const loading = allProjects ? plansAll.isLoading : single.isLoading;
  const failure = allProjects ? plansAll.error : single.error;

  const q = text.trim();
  const hits = useMemo(
    () => (q ? entries.filter((e) => matchesQuery([e.name, e.path, e.suiteId], q)).slice(0, SHOWN_MAX) : []),
    [entries, q],
  );
  const span = spansProjects(hits);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!boxRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [open]);

  if (suite) {
    return (
      <span
        title={suite.planName ? `${suite.title} - ${suite.planName}` : suite.title}
        className="flex h-10 min-w-0 max-w-full items-center gap-1.5 rounded-md border border-accent/50 bg-accent-soft px-2.5 text-sm"
      >
        <IconSuite aria-hidden className="size-4 shrink-0 text-faint" />
        <span className="min-w-0 truncate text-text">{suite.title}</span>
        {suite.planName && <span className="min-w-0 shrink truncate text-xs text-muted">{suite.planName}</span>}
        <button
          aria-label="Clear suite"
          className="ml-1 shrink-0 text-muted hover:text-danger"
          onClick={() => {
            onChange(null);
            setText("");
          }}
        >
          <X size={13} />
        </button>
      </span>
    );
  }

  const ready = allProjects ? Boolean(org) : Boolean(org && project);
  return (
    <div className="relative w-full" ref={boxRef}>
      <SearchInput
        aria-label="Find suite"
        className="w-full"
        placeholder={ready ? "Find suite" : "Pick a project first"}
        disabled={!ready}
        value={text}
        onChange={(t) => {
          setText(t);
          setWanted(true);
          setOpen(true);
        }}
        onClear={() => {
          setText("");
          setOpen(false);
        }}
        onFocus={() => setWanted(true)}
        onKeyDown={(e) => {
          if (e.key === "Escape") setOpen(false);
        }}
      />
      {open && q && (
        <div className="absolute left-0 top-full z-30 mt-1 w-full min-w-[28rem] max-w-[calc(100vw-2rem)] rounded-md border border-border bg-surface shadow-xl">
          {loading && <p className="px-3 py-2 text-sm text-muted">{scanNote ?? "Reading test plans"}</p>}
          {failure && <p className="px-3 py-2 text-sm text-danger">{failure.message}</p>}
          {!loading && !failure && hits.length === 0 && (
            <p className="px-3 py-2 text-sm text-muted">No suites match "{q}".</p>
          )}
          <ul className="max-h-72 overflow-y-auto p-1">
            {hits.map((hit) => (
              <li key={`${hit.planId}:${hit.suiteId}`}>
                <button
                  className="flex w-full items-baseline gap-1.5 rounded px-2 py-1.5 text-left text-sm text-text hover:bg-accent-soft"
                  onClick={() => {
                    onChange({
                      planId: hit.planId,
                      suiteId: hit.suiteId,
                      title: hit.name,
                      planName: hit.planName,
                      project: hit.project,
                    });
                    setOpen(false);
                    setText("");
                  }}
                >
                  <span className="min-w-0 flex-1 break-words">{hit.path}</span>
                  {hit.folder && <span className="shrink-0 text-xs text-muted">folder</span>}
                  {span && <ProjectChip project={hit.project} />}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
