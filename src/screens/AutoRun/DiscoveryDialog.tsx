// What the assistant's discovery has mapped of the live app, area by area:
// when it was explored and as which account, how many pages and elements it
// saw, whether the map is stale and why, and the save requests the page
// sent while it was explored. Forget map takes an area's map away (behind a
// confirm), so the area is explored again before new saves there. A map
// that cannot be read offers Reset map, which moves the damaged file aside
// (never deleting it) so discovery starts an empty one. Below the areas,
// the last menu mapping run: when it ran, the modules, what it added,
// updated, found unchanged and could not reach, and the saves it blocked.
// Opened from Auto Run's Setup card; the map and the summary are Rust's
// (`autorun/discovery_map.rs`, `autorun/mapping_summary.rs`).

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands, type MappingSummary, type MapView } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconClear } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { MODAL_LARGE } from "./modalWidths";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The query key the Setup row and this dialog share. */
export const discoveryMapKey = (org: string, project: string) => ["autorun-map", org, project];

/** The project's discovery map. A plain local file read through Rust. */
export function useDiscoveryMap(org: string, project: string) {
  return useQuery({
    queryKey: discoveryMapKey(org, project),
    queryFn: async () => (await unwrapStr(commands.autoRunLoadMap(org, project))) ?? { areas: [] },
    enabled: Boolean(org && project),
    retry: false,
  });
}

/** The Setup summary's line: how many areas are explored and how many of
 * those are stale. `null` until the map has answered. What belongs to no
 * area is not an area, so it is never counted. */
export function discoverySummary(map: MapView | undefined): string | null {
  if (!map) return null;
  const explored = map.areas.filter((a) => a.area !== "" && a.explored_at != null);
  if (explored.length === 0) return "Not explored yet";
  const stale = explored.filter((a) => a.stale).length;
  return `${plural(explored.length, "area")} explored, ${stale} stale`;
}

/** The project's last menu mapping run, `null` when none has ended. */
export function useMappingSummary(org: string, project: string) {
  return useQuery({
    queryKey: ["autorun-mapping-summary", org, project],
    queryFn: async () => (await unwrapStr(commands.autoRunLoadMappingSummary(org, project))) ?? null,
    enabled: Boolean(org && project),
    retry: false,
  });
}

/** One list of the last mapping, folded away; "none" when empty. */
function MappingList({ title, lines }: { title: string; lines: string[] }) {
  if (lines.length === 0) return <p className="text-xs text-muted">{title}: none</p>;
  return (
    <details className="text-xs text-muted">
      <summary className="cursor-pointer select-none text-muted hover:text-text">
        {title} ({lines.length})
      </summary>
      <ul aria-label={title} className="mt-1 space-y-0.5 pl-4">
        {lines.map((line, i) => (
          <li key={`${i}-${line}`} className="break-words text-text">
            {line}
          </li>
        ))}
      </ul>
    </details>
  );
}

/** The last mapping run: when, over which modules, and each list. */
function LastMapping({ summary }: { summary: MappingSummary }) {
  const headingId = useId();
  const ran = summary.ran_at != null ? new Date(summary.ran_at).toLocaleDateString() : null;
  return (
    <section aria-labelledby={headingId} className="space-y-1 border-t border-border/60 pt-2">
      <h3 id={headingId} className="text-xs font-semibold text-text">
        Last menu mapping
      </h3>
      <p className="text-xs text-muted">
        {ran ? `Ran ${ran}` : "Ran"} over {summary.modules.join(", ")}
      </p>
      <MappingList title="Added" lines={summary.added} />
      <MappingList
        title="Updated"
        lines={summary.updated.map((u) =>
          u.old_path === u.new_path
            ? `${u.name}: Same menu path, arrived on a different page`
            : `${u.name}: ${u.old_path} → ${u.new_path}`,
        )}
      />
      <MappingList title="Unchanged" lines={summary.unchanged} />
      <MappingList title="Could not reach" lines={summary.unreached.map((u) => `${u.name}: ${u.reason}`)} />
      <p className="text-xs text-muted">{plural(summary.blocked_writes, "save request")} blocked</p>
    </section>
  );
}

/** The name an area is shown under; what belongs to no area has none. */
const shownName = (area: string) => area || "Not in an area";

export default function DiscoveryDialog({
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
  const map = useDiscoveryMap(org, project);
  const mapping = useMappingSummary(org, project);
  // The area whose Forget map is waiting for its confirm.
  const [forgetting, setForgetting] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const forget = async (area: string) => {
    setProblem(null);
    setBusy(true);
    try {
      await unwrapStr(commands.autoRunForgetMapArea(org, project, area));
    } catch (e) {
      setProblem(message(e));
    }
    setBusy(false);
    setForgetting(null);
    await qc.invalidateQueries({ queryKey: discoveryMapKey(org, project) });
  };

  // Moves a damaged map aside: no confirm, since nothing is deleted.
  const [movedTo, setMovedTo] = useState<string | null>(null);
  const reset = async () => {
    setProblem(null);
    setBusy(true);
    try {
      setMovedTo(await unwrapStr(commands.autoRunResetMap(org, project)));
    } catch (e) {
      setProblem(message(e));
    }
    setBusy(false);
    await qc.invalidateQueries({ queryKey: discoveryMapKey(org, project) });
  };

  const areas = map.data?.areas ?? [];

  return (
    <Modal onClose={onClose} labelledBy={headingId} className={`${MODAL_LARGE} flex flex-col gap-3 p-5`}>
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Discovery
      </h2>
      <p className="text-xs text-muted">
        What the assistant has seen of the live app, area by area. A script is saved only against what it has seen.
      </p>

      {map.isLoading && <p className="text-xs text-muted">Loading…</p>}
      {map.isError && (
        <div className="flex flex-wrap items-center gap-2">
          <p className="min-w-0 flex-1 text-xs text-danger">{map.error.message}</p>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void reset()}>
            <IconClear aria-hidden />
            Reset map
          </Button>
        </div>
      )}
      {movedTo && (
        <p className="text-xs text-muted">
          The damaged map was kept as <span className="id-mono break-all">{movedTo}</span>.
        </p>
      )}
      {map.isSuccess && areas.length === 0 && (
        <p className="text-sm text-muted">
          Not explored yet. An assistant&apos;s <span className="id-mono">/tcm:discover</span> command explores an
          area.
        </p>
      )}
      {areas.length > 0 && (
        <ul aria-label="Explored areas" className="min-h-0 flex-1 divide-y divide-border/60 overflow-y-auto">
          {areas.map((a) => {
            const name = shownName(a.area);
            return (
              <li key={a.area} aria-label={name} className="space-y-1 py-2">
                <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                  <span className="min-w-0 flex-1 break-words text-sm font-medium text-text">{name}</span>
                  {a.stale && (
                    <Badge className="bg-warning/15 text-warning" title={a.stale_reason ?? undefined}>
                      Stale
                    </Badge>
                  )}
                  {forgetting === a.area ? (
                    <span
                      role="group"
                      aria-label={`Forget the map for ${name}?`}
                      className="flex flex-wrap items-center gap-2"
                    >
                      <span className="text-xs text-muted">
                        Forget the map for {name}? Scripts keep running; new saves there need the area explored
                        again.
                      </span>
                      <Button size="sm" variant="ghost" disabled={busy} onClick={() => setForgetting(null)}>
                        <IconCancel aria-hidden />
                        Keep
                      </Button>
                      <Button size="sm" variant="danger" disabled={busy} onClick={() => void forget(a.area)}>
                        <IconClear aria-hidden />
                        Forget map
                      </Button>
                    </span>
                  ) : (
                    <Button
                      size="sm"
                      variant="ghost"
                      aria-label={`Forget map for ${name}`}
                      disabled={busy}
                      onClick={() => setForgetting(a.area)}
                    >
                      <IconClear aria-hidden />
                      Forget map
                    </Button>
                  )}
                </div>
                <p className="text-xs text-muted">
                  {a.explored_at != null
                    ? `Explored ${new Date(a.explored_at).toLocaleDateString()}${a.account ? ` as ${a.account}` : ""}`
                    : "Not explored yet"}
                </p>
                <p className="text-xs text-muted">
                  {plural(a.pages, "page")}, {plural(a.elements, "element")}
                </p>
                {/* The badge's reason is hovered; this says it in words. */}
                {a.stale && a.stale_reason && <p className="text-xs text-warning">{a.stale_reason}</p>}
                {a.writes.length > 0 && (
                  <details className="text-xs text-muted">
                    <summary className="cursor-pointer select-none text-muted hover:text-text">
                      {plural(a.writes.length, "save request")}
                    </summary>
                    <ul aria-label={`Save requests in ${name}`} className="mt-1 space-y-0.5 pl-4">
                      {a.writes.map((w, i) => (
                        <li key={`${i}-${w.method}-${w.path}`} className="id-mono break-all text-text">
                          {w.method} {w.path}
                        </li>
                      ))}
                    </ul>
                  </details>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {mapping.data && <LastMapping summary={mapping.data} />}
      {mapping.isError && <p className="text-xs text-danger">{mapping.error.message}</p>}

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
