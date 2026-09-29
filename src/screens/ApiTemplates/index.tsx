import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { commands, events, type Flow, type SavedTemplate } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Collapse } from "../../components/ui/collapse";
import { Input } from "../../components/ui/input";
import { apiWritesSnapshot, subscribeApiWrites } from "../../lib/apiTemplates";
import { IconCollapseAll, IconExpandAll } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import { unwrapStr } from "../../lib/ipc";
import { pagePalette } from "../../lib/reportTheme";
import { sidebarCollapsedSnapshot, stickyLeftPx, subscribeSidebar } from "../../lib/sidebarState";
import { toast } from "../../lib/toast";
import FlowMap from "./FlowMap";
import RemoveFlow from "./RemoveFlow";
import RemoveTemplate from "./RemoveTemplate";
import TemplateRow, { hostOf } from "./TemplateRow";

/** The query's key prefix: the change event invalidates every project's. */
const KEY = "api-templates";

/** A flow no template performs a stage of: one array, so its map's memo holds. */
const NONE: SavedTemplate[] = [];

/** The saved flow and stage a template's `stage` names - either absent
 * when it is no longer saved. */
function stageOf(t: SavedTemplate["template"], flows: Map<string, Flow>) {
  const flow = t.stage ? flows.get(t.stage.flow) : undefined;
  const stage = flow?.stages.find((s) => s.id === t.stage?.id);
  return { flow, stage };
}

/** A template's stage, spelled out for its row: "Evaluation rules
 * (Performance cycle wizard)", or - when its flow or its stage is no longer
 * saved - the ids it names, marked missing. */
function stageLine(t: SavedTemplate["template"], flows: Map<string, Flow>) {
  const ref = t.stage;
  if (!ref) return undefined;
  const { flow, stage } = stageOf(t, flows);
  if (flow && stage) return { text: `${stage.title} (${flow.title})`, missing: false };
  return { text: `${ref.id} (${flow ? flow.title : ref.flow})`, missing: true };
}

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
  const sidebarCollapsed = useSyncExternalStore(subscribeSidebar, sidebarCollapsedSnapshot);
  // Folded module groups, remembered across sessions like the test case
  // screens' own.
  const [folded, toggleFolded, foldGroups, unfoldGroups] = usePersistedStringSet(
    "tcm-v2-api-templates-collapsed-groups",
  );
  const [search, setSearch] = useState("");
  const [removing, setRemoving] = useState<SavedTemplate["template"] | null>(null);
  const [removingFlow, setRemovingFlow] = useState<Flow | null>(null);
  // Rows opened by their own toggle or from a flow map. A set, so opening
  // one from a map does not fold the others a person had open.
  const [openIds, setOpenIds] = useState<ReadonlySet<string>>(new Set());

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
  // An overview from before flows existed has no `flows` at all.
  const flows = useMemo(() => overview.data?.flows ?? [], [overview.data]);
  const flowsById = useMemo(() => new Map(flows.map((f) => [f.id, f])), [flows]);
  const q = search.trim().toLowerCase();
  const groups = useMemo(() => {
    const hit = (...texts: Array<string | undefined>) => texts.some((s) => s?.toLowerCase().includes(q));
    // A template matches on its own title, module or id, or on the title
    // of its stage or of its flow.
    const templateHit = ({ template: t }: SavedTemplate) => {
      const { flow, stage } = stageOf(t, flowsById);
      return hit(t.title, t.module, t.id, stage?.title, flow?.title);
    };
    const shown = q ? templates.filter(templateHit) : templates;
    // A flow shows on its title, module or a stage title - or when one of
    // its templates does.
    const shownFlows = q
      ? flows.filter(
          (f) =>
            hit(f.title, f.module, ...f.stages.map((s) => s.title)) ||
            shown.some(({ template: t }) => t.stage?.flow === f.id),
        )
      : flows;

    const byModule = new Map<string, { flows: Flow[]; list: SavedTemplate[] }>();
    const entry = (module: string) => {
      const e = byModule.get(module) ?? { flows: [], list: [] };
      byModule.set(module, e);
      return e;
    };
    for (const f of shownFlows) entry(f.module).flows.push(f);
    for (const s of shown) entry(s.template.module).list.push(s);
    return [...byModule.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([module, e]) => ({
        module,
        flows: [...e.flows].sort((a, b) => a.title.localeCompare(b.title)),
        list: [...e.list].sort((a, b) => a.template.title.localeCompare(b.template.title)),
      }));
  }, [templates, flows, flowsById, q]);

  const setRowOpen = (id: string, open: boolean) =>
    setOpenIds((prev) => {
      const next = new Set(prev);
      if (open) next.add(id);
      else next.delete(id);
      return next;
    });

  const openFromMap = (id: string) => {
    setRowOpen(id, true);
    document.getElementById(`api-template-${id}`)?.scrollIntoView({ block: "nearest" });
  };

  // The saved templates that name each flow in their `stage` - one array per
  // flow across renders, so a map's layout is not worked out again for nothing.
  const byFlow = useMemo(() => {
    const m = new Map<string, SavedTemplate[]>();
    for (const s of templates) {
      const id = s.template.stage?.flow;
      if (id) m.set(id, [...(m.get(id) ?? []), s]);
    }
    return m;
  }, [templates]);
  const onFlow = (flowId: string) => byFlow.get(flowId) ?? NONE;

  // A search opens every group, so a match is never hidden in a fold.
  const isFolded = (module: string) => !q && folded.has(module);
  const moduleNames = groups.map((g) => g.module);
  const openGroups = moduleNames.filter((m) => !isFolded(m));

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

      {overview.data && templates.length === 0 && flows.length === 0 && (
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

      {(templates.length > 0 || flows.length > 0) && (
        <>
          <Input
            aria-label="Search templates"
            placeholder="Search by title, module, id or stage"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="w-full max-w-sm"
          />
          {groups.length === 0 ? (
            <p className="text-sm text-muted">No template matches “{search.trim()}”.</p>
          ) : (
            groups.map(({ module, flows: moduleFlows, list }) => (
              <section key={module} aria-label={module} className="space-y-1">
                <div className="flex w-full items-center gap-3 pb-1 pt-2">
                  {/* The chevron and the title both fold the group - clicking
                      the name is how people expect to open one. */}
                  <button
                    aria-label={`${isFolded(module) ? "Expand" : "Collapse"} group ${module}`}
                    aria-expanded={!isFolded(module)}
                    title={isFolded(module) ? "Expand group" : "Collapse group"}
                    className="text-muted transition-colors hover:text-accent"
                    onClick={() => toggleFolded(module)}
                  >
                    {isFolded(module) ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
                  </button>
                  <button
                    className="group"
                    title={isFolded(module) ? "Expand group" : "Collapse group"}
                    onClick={() => toggleFolded(module)}
                  >
                    <h2 className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                      {module}
                      {list.length > 0 && ` (${list.length})`}
                    </h2>
                  </button>
                  <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border to-transparent" />
                </div>
                <Collapse open={!isFolded(module)}>
                {moduleFlows.length > 0 && (
                  <div className="space-y-2 pb-2">
                    {moduleFlows.map((f) => (
                      <FlowMap
                        key={f.id}
                        flow={f}
                        templates={onFlow(f.id)}
                        onOpenTemplate={openFromMap}
                        onView={() => {
                          // Rust writes the page and opens it, as the review
                          // page's tree view does; a failure is a sentence.
                          unwrapStr(commands.apiTemplatesOpenFlow(org, project, f.id, pagePalette())).catch(
                            (e: unknown) => toast.error(e instanceof Error ? e.message : String(e)),
                          );
                        }}
                        onRemove={() => setRemovingFlow(f)}
                      />
                    ))}
                  </div>
                )}
                {list.length > 0 && (
                  <ul className="space-y-1">
                    {list.map((s) => (
                      <TemplateRow
                        key={s.template.id}
                        saved={s}
                        open={openIds.has(s.template.id)}
                        onOpenChange={(open) => setRowOpen(s.template.id, open)}
                        stage={stageLine(s.template, flowsById)}
                        onRemove={() => setRemoving(s.template)}
                      />
                    ))}
                  </ul>
                )}
                </Collapse>
              </section>
            ))
          )}
        </>
      )}

      {/* Collapse all, stuck bottom left like the test case screens' own: one
          button that folds every open group, and opens them all again once
          every group is folded. Portalled so it pins to the window. */}
      {groups.length > 0 &&
        createPortal(
          <div
            className="fixed bottom-6 z-40 rounded-full border border-accent bg-bg shadow-2xl transition-[left] duration-200"
            style={{ left: stickyLeftPx(sidebarCollapsed) }}
          >
            <Button
              size="sm"
              variant="ghost"
              className="rounded-full text-text hover:bg-surface-2 hover:text-text"
              onClick={() => (openGroups.length > 0 ? foldGroups(openGroups) : unfoldGroups(moduleNames))}
            >
              {openGroups.length > 0 ? (
                <>
                  <IconCollapseAll aria-hidden />
                  Collapse all ({openGroups.length})
                </>
              ) : (
                <>
                  <IconExpandAll aria-hidden />
                  Expand all ({moduleNames.length})
                </>
              )}
            </Button>
          </div>,
          document.body,
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

      {removingFlow && (
        <RemoveFlow
          org={org}
          project={project}
          flow={removingFlow}
          templates={onFlow(removingFlow.id).length}
          onClose={() => setRemovingFlow(null)}
          // Removing a flow emits no change event: read the files again here.
          onRemoved={() => void qc.invalidateQueries({ queryKey: [KEY] })}
        />
      )}
    </div>
  );
}
