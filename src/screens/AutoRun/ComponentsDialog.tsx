// The project's components: steps an assistant saved once, after trying
// them on the live app, that scripts use by name. Grouped by the area each
// was tried in (first appearance, "Other" last), one line each; opening one
// shows its inputs, when it was tried, its changes and the cases using it.
// Remove is held while a script uses one and asks first otherwise. A file
// that cannot be read offers Reset components, which moves the damaged
// file aside (never deleting it). Opened from Auto Test's Setup card; the
// components themselves are Rust's (`autorun/components.rs`).

import { useCloseOnEnvironmentChange } from "../../hooks/useCloseOnEnvironmentChange";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState, type ReactNode } from "react";
import { commands, type ComponentsView, type ComponentView } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Term } from "../../components/Term";
import { Modal } from "../../components/ui/modal";
import { trimLabels } from "../../components/ui/labelText";
import { SearchInput } from "../../components/ui/search-input";
import {
  IconCancel,
  IconClear,
  IconCollapseAll,
  IconExpandAll,
  IconHideDetails,
  IconRemove,
  IconScriptArea,
  IconShowDetails,
} from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { plural } from "../../lib/plural";
import { MODAL_LARGE } from "./modalWidths";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The query key the Setup row and this dialog share. */
export const componentsKey = (org: string, project: string) => ["autorun-components", org, project];

/** The project's components. A plain local file read through Rust. */
export function useComponents(org: string, project: string) {
  return useQuery({
    queryKey: componentsKey(org, project),
    queryFn: async () => (await unwrapStr(commands.autoRunLoadComponents(org, project))) ?? { components: [] },
    enabled: Boolean(org && project),
    retry: false,
  });
}

/** The Setup summary's line. `null` until the file has answered. */
export function componentsSummary(view: ComponentsView | undefined): string | null {
  if (!view) return null;
  return view.components.length === 0 ? "None yet" : plural(view.components.length, "component");
}

/** The group a component with no area falls into; always last. */
const OTHER = "Other";

/** The query appears in the name, description, an input name or a case id. */
function matches(c: ComponentView, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return (
    c.name.toLowerCase().includes(q) ||
    c.description.toLowerCase().includes(q) ||
    c.inputs.some((i) => i.name.toLowerCase().includes(q)) ||
    c.used_by_cases.some((id) => String(id).includes(q))
  );
}

/** By the area each was tried in, in order of first appearance; "Other" last. */
function groupByArea(items: ComponentView[]): { area: string; items: ComponentView[] }[] {
  const map = new Map<string, ComponentView[]>();
  for (const c of items) {
    const key = c.tried_area || OTHER;
    map.set(key, [...(map.get(key) ?? []), c]);
  }
  const other = map.get(OTHER);
  map.delete(OTHER);
  const groups = [...map].map(([area, list]) => ({ area, items: list }));
  if (other) groups.push({ area: OTHER, items: other });
  return groups;
}

/** A small pill; `warn` is the amber one. */
function Chip({ warn, mono, children }: { warn?: boolean; mono?: boolean; children: ReactNode }) {
  const tone = warn ? "bg-warning/15 text-warning" : "bg-surface-2 text-muted";
  return (
    <span className={`rounded-full px-2 py-0.5 text-[11px] font-medium ${tone}${mono ? " id-mono" : ""}`}>
      {trimLabels(children)}
    </span>
  );
}
const FIRST_CASES = 5;

function ComponentRow({
  c,
  open,
  onToggle,
  busy,
  removing,
  setRemoving,
  remove,
}: {
  c: ComponentView;
  open: boolean;
  onToggle: () => void;
  busy: boolean;
  removing: boolean;
  setRemoving: (name: string | null) => void;
  remove: (name: string) => void;
}) {
  const bodyId = useId();
  const [allCases, setAllCases] = useState(false);
  const inUse = c.used_by_cases.length > 0;
  const shown = allCases ? c.used_by_cases : c.used_by_cases.slice(0, FIRST_CASES);
  const more = c.used_by_cases.length - shown.length;
  return (
    <li aria-label={c.name} className="py-1.5">
      <div className="flex items-center gap-2">
        <button
          type="button"
          aria-expanded={open}
          aria-controls={bodyId}
          aria-label={`${open ? "Hide" : "Show"} ${c.name}`}
          className="inline-flex size-6 shrink-0 items-center justify-center rounded text-muted hover:text-accent focus-visible:outline-2 focus-visible:outline-accent [&_svg]:size-4"
          onClick={onToggle}
        >
          {open ? <IconHideDetails aria-hidden /> : <IconShowDetails aria-hidden />}
        </button>
        <span className="min-w-0 flex-1 break-words text-sm font-medium text-text">{c.name}</span>
        <span className="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
          <Chip>{plural(c.used_by_cases.length, "case")}</Chip>
          <Chip>v{c.version}</Chip>
          {!inUse && <Chip warn>Unused</Chip>}
        </span>
      </div>
      <div id={bodyId} hidden={!open} className="space-y-1.5 py-2 pl-8 text-xs">
        {open && (
          <>
            {c.description && <p className="text-text">{c.description}</p>}
            {c.inputs.length > 0 && (
              <ul aria-label={`Inputs of ${c.name}`} className="space-y-0.5 text-muted">
                {c.inputs.map((i) => (
                  <li key={i.name}>
                    <span className="id-mono text-[11px] text-text">{i.name}</span> · {i.kind} · {i.description}
                  </li>
                ))}
              </ul>
            )}
            <p className="text-muted">
              {c.tried_at != null ? `Tried ${new Date(c.tried_at).toLocaleDateString()}` : "Not tried yet"} ·{" "}
              {c.changes > 0 ? `changed ${plural(c.changes, "time")}` : "never changed"}
            </p>
            {inUse && (
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-muted">Used by</span>
                {shown.map((id) => (
                  <Chip key={id} mono>
                    {id}
                  </Chip>
                ))}
                {more > 0 && (
                  <button
                    type="button"
                    className="text-accent hover:underline focus-visible:outline-2 focus-visible:outline-accent"
                    onClick={() => setAllCases(true)}
                  >
                    and {more} more
                  </button>
                )}
              </div>
            )}
            <div className="flex justify-end">
              {removing ? (
                <span role="group" aria-label={`Remove ${c.name}?`} className="flex flex-wrap items-center gap-2">
                  <span className="text-muted">Remove {c.name}? Scripts no longer use it.</span>
                  <Button size="sm" variant="ghost" disabled={busy} onClick={() => setRemoving(null)}>
                    <IconCancel aria-hidden />
                    Keep
                  </Button>
                  <Button size="sm" variant="danger" disabled={busy} onClick={() => remove(c.name)}>
                    <IconRemove aria-hidden />
                    Remove
                  </Button>
                </span>
              ) : (
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label={`Remove ${c.name}`}
                  disabled={busy || inUse}
                  title={inUse ? "Scripts still use it" : undefined}
                  onClick={() => setRemoving(c.name)}
                >
                  <IconRemove aria-hidden />
                  Remove
                </Button>
              )}
            </div>
          </>
        )}
      </div>
    </li>
  );
}

export default function ComponentsDialog({
  org,
  project,
  onClose,
}: {
  org: string;
  project: string;
  onClose: () => void;
}) {
  // What it holds is this environment's: it never outlives a switch.
  useCloseOnEnvironmentChange(onClose);
  const headingId = useId();
  const qc = useQueryClient();
  const view = useComponents(org, project);
  // The component whose Remove is waiting for its confirm.
  const [removing, setRemoving] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<Set<string>>(new Set());

  const remove = async (name: string) => {
    setProblem(null);
    setBusy(true);
    try {
      await unwrapStr(commands.autoRunRemoveComponent(org, project, name));
    } catch (e) {
      setProblem(message(e));
    }
    setBusy(false);
    setRemoving(null);
    await qc.invalidateQueries({ queryKey: componentsKey(org, project) });
  };

  // Moves a damaged file aside: no confirm, since nothing is deleted.
  const [movedTo, setMovedTo] = useState<string | null>(null);
  const reset = async () => {
    setProblem(null);
    setBusy(true);
    try {
      setMovedTo(await unwrapStr(commands.autoRunResetComponents(org, project)));
    } catch (e) {
      setProblem(message(e));
    }
    setBusy(false);
    await qc.invalidateQueries({ queryKey: componentsKey(org, project) });
  };

  const components = view.data?.components ?? [];
  const toggle = (name: string) => {
    // A row that folds takes its armed Remove confirm with it.
    if (open.has(name)) setRemoving((r) => (r === name ? null : r));
    setOpen((prev) => {
      const next = new Set(prev);
      if (!next.delete(name)) next.add(name);
      return next;
    });
  };
  const visible = components.filter((c) => matches(c, query));
  const groups = groupByArea(visible);
  const allOpen = visible.length > 0 && visible.every((c) => open.has(c.name));
  const caseCount = new Set(components.flatMap((c) => c.used_by_cases)).size;

  return (
    <Modal onClose={onClose} labelledBy={headingId} className={`${MODAL_LARGE} flex flex-col gap-3 p-5`}>
      <div className="flex flex-wrap items-baseline justify-between gap-x-3">
        <h2 id={headingId} className="text-sm font-semibold text-text">
          <Term name="component">Components</Term>
        </h2>
        {components.length > 0 && (
          <span className="text-xs text-muted">
            {plural(components.length, "component")} · used by {plural(caseCount, "case")}
          </span>
        )}
      </div>
      <p className="text-xs text-muted">
        Steps an assistant saved once, after trying them on the live app, that scripts use by name. One stays while a
        script uses it.
      </p>

      {view.isLoading && <p className="text-xs text-muted">Loading…</p>}
      {view.isError && (
        <div className="flex flex-wrap items-center gap-2">
          <p className="min-w-0 flex-1 text-xs text-danger">{view.error.message}</p>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void reset()}>
            <IconClear aria-hidden />
            Reset components
          </Button>
        </div>
      )}
      {movedTo && (
        <p className="text-xs text-muted">
          The damaged file was kept as <span className="id-mono break-all">{movedTo}</span>.
        </p>
      )}
      {view.isSuccess && components.length === 0 && (
        <p className="text-sm text-muted">None yet. An assistant saves a component after trying it on the live app.</p>
      )}
      {components.length > 0 && (
        <>
          <SearchInput
            aria-label="Search components"
            placeholder="Search components"
            value={query}
            onChange={(text) => {
              setRemoving(null);
              setQuery(text);
            }}
          />
          {groups.length === 0 ? (
            <p className="text-sm text-muted">No components match "{query.trim()}"</p>
          ) : (
            <div role="list" aria-label="Saved components" className="min-h-0 flex-1 space-y-3 overflow-y-auto">
              {groups.map((g) => (
                <section key={g.area} role="listitem" aria-label={g.area}>
                  <h3 className="flex items-center gap-1.5 border-b border-border/60 pb-1 text-xs font-semibold text-text">
                    <IconScriptArea aria-hidden className="size-3.5 shrink-0 text-muted" />
                    <span className="min-w-0 break-words">{g.area}</span>
                    <span className="text-muted">· {g.items.length}</span>
                  </h3>
                  <ul className="divide-y divide-border/60">
                    {g.items.map((c) => (
                      <ComponentRow
                        key={c.name}
                        c={c}
                        open={open.has(c.name)}
                        onToggle={() => toggle(c.name)}
                        busy={busy}
                        removing={removing === c.name}
                        setRemoving={setRemoving}
                        remove={(n) => void remove(n)}
                      />
                    ))}
                  </ul>
                </section>
              ))}
            </div>
          )}
        </>
      )}

      {problem && <p className="text-xs text-danger">{problem}</p>}

      <div className="flex flex-wrap items-center justify-between gap-2">
        {visible.length > 0 ? (
          allOpen ? (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                setRemoving(null);
                setOpen((prev) => {
                  const next = new Set(prev);
                  for (const c of visible) next.delete(c.name);
                  return next;
                });
              }}
            >
              <IconCollapseAll aria-hidden />
              Collapse all
            </Button>
          ) : (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setOpen((prev) => new Set([...prev, ...visible.map((c) => c.name)]))}
            >
              <IconExpandAll aria-hidden />
              Expand all
            </Button>
          )
        ) : (
          <span />
        )}
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}
