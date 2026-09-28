import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { commands, events, type SavedTemplate } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { apiWritesSnapshot, subscribeApiWrites } from "../../lib/apiTemplates";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import RemoveTemplate from "./RemoveTemplate";
import TemplateRow, { hostOf } from "./TemplateRow";

/** The query's key prefix: the change event invalidates every project's. */
const KEY = "api-templates";

/**
 * API Templates: the operations an assistant has built from the
 * application's code and proven on this machine, for the owner to read and
 * remove (spec §8). Offered exactly where Auto Run is - App mounts it only
 * while Auto Run is shown.
 *
 * The data is local files read through Rust, so it is a plain query with
 * no persistent cache: reading it again costs nothing, and a copy on disk
 * could only ever be staler than the files themselves. Rust emits
 * `api-templates-changed` when a prove or run saves something, and the
 * list reloads on it.
 */
export default function ApiTemplates({
  org,
  project,
  onOpenAiBridge,
}: {
  org: string;
  project: string;
  onOpenAiBridge: () => void;
}) {
  const qc = useQueryClient();
  const writesOn = useSyncExternalStore(subscribeApiWrites, apiWritesSnapshot);
  const [search, setSearch] = useState("");
  const [removing, setRemoving] = useState<SavedTemplate["template"] | null>(null);

  const overview = useQuery({
    queryKey: [KEY, org, project],
    queryFn: () => unwrapStr(commands.apiTemplatesOverview(org, project)),
    enabled: Boolean(org && project),
  });

  useEffect(() => {
    const un = events.apiTemplatesChanged.listen(() => {
      void qc.invalidateQueries({ queryKey: [KEY] });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [qc]);

  const templates = useMemo(() => overview.data?.templates ?? [], [overview.data]);
  const q = search.trim().toLowerCase();
  const groups = useMemo(() => {
    const shown = q
      ? templates.filter(({ template: t }) =>
          [t.title, t.module, t.id].some((s) => s.toLowerCase().includes(q)),
        )
      : templates;
    const byModule = new Map<string, SavedTemplate[]>();
    for (const s of shown) {
      const list = byModule.get(s.template.module) ?? [];
      list.push(s);
      byModule.set(s.template.module, list);
    }
    return [...byModule.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([module, list]) => ({
        module,
        list: [...list].sort((a, b) => a.template.title.localeCompare(b.template.title)),
      }));
  }, [templates, q]);

  if (!org || !project) {
    return <p className="text-sm text-muted">Pick an organization and project in the bar above first.</p>;
  }

  const origin = overview.data?.origin ?? null;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs text-muted">
        <span>
          Project <span className="font-medium text-text">{project}</span>
        </span>
        {overview.data && (
          <span>
            Runs against{" "}
            <span className="font-medium text-text">{origin ? hostOf(origin) : "no sign-in recipe yet"}</span>
          </span>
        )}
        <button
          onClick={onOpenAiBridge}
          title="Turned on and off on the AI Bridge tab"
          className={cn(
            "rounded-full px-2 py-0.5 text-[10px] font-medium transition-colors hover:text-accent",
            writesOn ? "bg-success/15 text-success" : "bg-surface-2 text-muted",
          )}
        >
          API templates {writesOn ? "on" : "off"}
        </button>
      </div>

      {overview.isLoading && <p className="text-sm text-muted">Loading templates…</p>}
      {overview.isError && <p className="text-sm text-danger">{overview.error.message}</p>}

      {overview.data && templates.length === 0 && (
        <div className="space-y-3 rounded-md border border-border bg-surface p-4">
          <p className="text-sm text-muted">
            Your assistant builds these from the application's code and proves each one before it appears
            here. Connect one and turn on API templates on the AI Bridge tab.
          </p>
          <Button size="sm" variant="outline" onClick={onOpenAiBridge}>
            Open AI Bridge
          </Button>
        </div>
      )}

      {templates.length > 0 && (
        <>
          <Input
            aria-label="Search templates"
            placeholder="Search by title, module or id"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full max-w-sm"
          />
          {groups.length === 0 ? (
            <p className="text-sm text-muted">No template matches “{search.trim()}”.</p>
          ) : (
            groups.map(({ module, list }) => (
              <section key={module} aria-label={module} className="space-y-1">
                <div className="flex w-full items-center gap-3 pb-1 pt-2">
                  <h2 className="text-sm font-semibold tracking-wide text-muted">
                    {module} ({list.length})
                  </h2>
                  <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border to-transparent" />
                </div>
                <ul className="space-y-1">
                  {list.map((s) => (
                    <TemplateRow key={s.template.id} saved={s} onRemove={() => setRemoving(s.template)} />
                  ))}
                </ul>
              </section>
            ))
          )}
        </>
      )}

      {removing && (
        <RemoveTemplate
          org={org}
          project={project}
          template={removing}
          onClose={() => setRemoving(null)}
          onRemoved={() => void qc.invalidateQueries({ queryKey: [KEY] })}
        />
      )}
    </div>
  );
}
