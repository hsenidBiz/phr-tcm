// The project's components: steps an assistant saved once, after trying
// them on the live app, that scripts use by name. Each shows its inputs,
// when and where it was last tried, its version, how many times it has
// been changed so far, and the scripts that use it.
// Remove is held while a script uses one and asks first otherwise. A file
// that cannot be read offers Reset components, which moves the damaged
// file aside (never deleting it). Opened from Auto Run's Setup card; the
// components themselves are Rust's (`autorun/components.rs`).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands, type ComponentsView } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconClear, IconRemove } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { MODAL_LARGE } from "./modalWidths";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

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

/** "Used by case 7" or "Used by cases 4, 12". */
const usedBy = (cases: number[]) =>
  cases.length === 1 ? `Used by case ${cases[0]}` : `Used by cases ${cases.join(", ")}`;

/** Why Remove is held: the scripts that use it, and what to do. */
const heldBecause = (cases: number[]) =>
  cases.length === 1
    ? `${usedBy(cases)}: change that script first.`
    : `${usedBy(cases)}: change those scripts first.`;

export default function ComponentsDialog({
  org,
  project,
  onClose,
}: {
  org: string;
  project: string;
  onClose: () => void;
}) {
  const headingId = useId();
  const qc = useQueryClient();
  const view = useComponents(org, project);
  // The component whose Remove is waiting for its confirm.
  const [removing, setRemoving] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

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

  return (
    <Modal onClose={onClose} labelledBy={headingId} className={`${MODAL_LARGE} flex flex-col gap-3 p-5`}>
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Components
      </h2>
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
        <ul aria-label="Saved components" className="min-h-0 flex-1 divide-y divide-border/60 overflow-y-auto">
          {components.map((c) => {
            const inUse = c.used_by_cases.length > 0;
            return (
              <li key={c.name} aria-label={c.name} className="space-y-1 py-2">
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <span className="id-mono min-w-0 flex-1 break-words text-sm font-medium text-text">{c.name}</span>
                  {c.changes >= c.cap && (
                    <Badge
                      className="bg-warning/15 text-warning"
                      title={`Changed ${c.cap} times or more: the assistant stops and asks you before changing it again`}
                    >
                      Needs a look
                    </Badge>
                  )}
                  {removing === c.name ? (
                    <span role="group" aria-label={`Remove ${c.name}?`} className="flex flex-wrap items-center gap-2">
                      <span className="text-xs text-muted">Remove {c.name}? Scripts no longer use it.</span>
                      <Button size="sm" variant="ghost" disabled={busy} onClick={() => setRemoving(null)}>
                        <IconCancel aria-hidden />
                        Keep
                      </Button>
                      <Button size="sm" variant="danger" disabled={busy} onClick={() => void remove(c.name)}>
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
                      title={inUse ? heldBecause(c.used_by_cases) : undefined}
                      onClick={() => setRemoving(c.name)}
                    >
                      <IconRemove aria-hidden />
                      Remove
                    </Button>
                  )}
                </div>
                {c.description && <p className="text-xs text-text">{c.description}</p>}
                {c.inputs.length > 0 && (
                  <ul aria-label={`Inputs of ${c.name}`} className="space-y-0.5 pl-4 text-xs text-muted">
                    {c.inputs.map((i) => (
                      <li key={i.name}>
                        <span className="id-mono text-text">{i.name}</span> ({i.kind}): {i.description}
                      </li>
                    ))}
                  </ul>
                )}
                <p className="text-xs text-muted">
                  {c.tried_at != null
                    ? `Tried ${new Date(c.tried_at).toLocaleDateString()}${c.tried_area ? ` in ${c.tried_area}` : ""}`
                    : "Not tried yet"}
                </p>
                <p className="flex flex-wrap gap-x-3 text-xs text-muted">
                  <span>Version {c.version}</span>
                  {c.changes > 0 && (
                    <span className={c.changes >= c.cap ? "text-warning" : undefined}>
                      Changed {c.changes} {c.changes === 1 ? "time" : "times"}
                      {c.changes >= c.cap && "; the assistant stops and asks you before changing it again"}
                    </span>
                  )}
                </p>
                <p className="text-xs text-muted">{inUse ? usedBy(c.used_by_cases) : "Not used by any script"}</p>
              </li>
            );
          })}
        </ul>
      )}

      {problem && <p className="text-xs text-danger">{problem}</p>}

      <div className="flex flex-wrap justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}
