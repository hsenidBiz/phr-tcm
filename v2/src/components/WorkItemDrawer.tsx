import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { marked } from "marked";
import { useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";
import TurndownService from "turndown";
import { toast } from "sonner";
import { commands, type WorkItemDetail } from "../bindings";
import { unwrap } from "../lib/ipc";
import { Button } from "./ui/button";
import DateField from "./ui/datefield";
import { Input, Textarea } from "./ui/input";
import { Select } from "./ui/select";
import { Skeleton } from "./ui/skeleton";
import CommentsPanel from "./CommentsPanel";

type Draft = {
  title: string;
  state: string;
  assignedToUnique: string;
  activity: string;
  remaining: string;
  completed: string;
  original: string;
  startDate: string;
  finishDate: string;
  description: string;
  /** Extra rich-text tabs (Bug: RCA / Preventive Measures), markdown by
   * reference name. */
  extras: Record<string, string>;
};

/** ADO stores descriptions as HTML; converting to markdown here means the
 * Write tab shows the formatting ADO has (bold, lists, links) as markdown
 * source, and saving (marked: md -> HTML) round-trips it. */
const turndown = new TurndownService({
  headingStyle: "atx",
  codeBlockStyle: "fenced",
  bulletListMarker: "-",
});

function htmlToMd(html: string): string {
  return html.trim() ? turndown.turndown(html) : "";
}

function toDraft(d: WorkItemDetail): Draft {
  return {
    extras: Object.fromEntries(
      // ?? []: tolerate cached details from before this field existed.
      (d.extra_sections ?? []).map((s) => [s.reference_name, htmlToMd(s.html)]),
    ),
    title: d.title,
    state: d.state,
    assignedToUnique: d.assigned_to_unique,
    activity: d.activity,
    remaining: d.remaining_work?.toString() ?? "",
    completed: d.completed_work?.toString() ?? "",
    original: d.original_estimate?.toString() ?? "",
    startDate: d.start_date.slice(0, 10),
    finishDate: d.finish_date.slice(0, 10),
    description: d.description_html?.trim()
      ? turndown.turndown(d.description_html)
      : d.description_text,
  };
}

/** Right-side detail editor: PATCHes only the fields the user changed
 * (v1 optimistic-save rule); ADO rejections surface verbatim. */
export default function WorkItemDrawer({
  org,
  project,
  itemId,
  states,
  onClose,
  onSaved,
}: {
  org: string;
  project: string;
  itemId: number;
  states: string[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const qc = useQueryClient();
  const detail = useQuery({
    queryKey: ["wi-detail", org, project, itemId],
    queryFn: () => unwrap(commands.workItemDetail(org, project, itemId)),
    retry: false,
  });

  const members = useQuery({
    queryKey: ["members", org, project],
    queryFn: () => unwrap(commands.listTeamMembers(org, project)),
    staleTime: 24 * 60 * 60_000, // v1 cached members for 24h
  });

  const activities = useQuery({
    queryKey: ["activities", org, project, detail.data?.work_item_type],
    queryFn: () => unwrap(commands.activityValues(org, project, detail.data!.work_item_type)),
    enabled: Boolean(detail.data),
    staleTime: Infinity,
  });

  const [draft, setDraft] = useState<Draft | null>(null);
  const [descMode, setDescMode] = useState<"write" | "preview">("write");
  // Which rich-text tab is active: "" = Description, else the extra
  // section's reference name (Bug: RCA / Preventive Measures).
  const [docTab, setDocTab] = useState("");

  // The drawer's left edge is draggable; the chosen width persists.
  const MIN_W = 320;
  const MAX_W = 900;
  const [width, setWidth] = useState(() => {
    const v = Number(localStorage.getItem("tcm-v2-drawer-width"));
    return v >= MIN_W && v <= MAX_W ? v : 384;
  });
  const asideRef = useRef<HTMLElement>(null);
  const startResize = (e: ReactMouseEvent) => {
    e.preventDefault();
    const right = asideRef.current?.getBoundingClientRect().right ?? window.innerWidth;
    const prevCursor = document.body.style.cursor;
    document.body.style.cursor = "col-resize";
    const onMove = (ev: MouseEvent) => {
      setWidth(Math.min(MAX_W, Math.max(MIN_W, Math.round(right - ev.clientX))));
    };
    const onUp = () => {
      document.body.style.cursor = prevCursor;
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      setWidth((w) => {
        try {
          localStorage.setItem("tcm-v2-drawer-width", String(w));
        } catch {
          // session-only
        }
        return w;
      });
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };
  useEffect(() => {
    if (detail.data) {
      setDraft(toDraft(detail.data));
      setDocTab(""); // back to Description when a different item loads
    }
  }, [detail.data]);

  const save = useMutation({
    mutationFn: async () => {
      const d = detail.data!;
      const dr = draft!;
      const orig = toDraft(d);
      const patches: { reference_name: string; value: string }[] = [];
      const push = (ref: string, now: string, before: string) => {
        if (now !== before) patches.push({ reference_name: ref, value: now });
      };
      push("System.Title", dr.title, orig.title);
      push("System.State", dr.state, orig.state);
      push("System.AssignedTo", dr.assignedToUnique, orig.assignedToUnique);
      push("Microsoft.VSTS.Common.Activity", dr.activity, orig.activity);
      push("Microsoft.VSTS.Scheduling.RemainingWork", dr.remaining, orig.remaining);
      push("Microsoft.VSTS.Scheduling.CompletedWork", dr.completed, orig.completed);
      push("Microsoft.VSTS.Scheduling.OriginalEstimate", dr.original, orig.original);
      push("Microsoft.VSTS.Scheduling.StartDate", dr.startDate, orig.startDate);
      push("Microsoft.VSTS.Scheduling.FinishDate", dr.finishDate, orig.finishDate);
      if (dr.description !== orig.description) {
        // The description is authored as markdown and stored in ADO as the
        // rendered HTML (breaks: single newlines become <br>, like v1).
        patches.push({
          reference_name: d.description_field,
          value: `<div>${marked.parse(dr.description, { async: false, breaks: true })}</div>`,
        });
      }
      // Extra rich-text tabs (Bug: RCA / Preventive Measures) save the same
      // way - markdown -> HTML, and only when actually changed.
      for (const s of d.extra_sections ?? []) {
        const now = dr.extras[s.reference_name] ?? "";
        if (now !== htmlToMd(s.html)) {
          patches.push({
            reference_name: s.reference_name,
            value: `<div>${marked.parse(now, { async: false, breaks: true })}</div>`,
          });
        }
      }
      if (patches.length === 0) return false;
      await unwrap(commands.updateWorkItem(org, project, itemId, patches));
      return true;
    },
    onSuccess: (changed) => {
      if (changed) {
        toast.success(`Saved #${itemId}`);
        qc.invalidateQueries({ queryKey: ["wi-detail", org, project, itemId] });
        onSaved();
      } else {
        toast.info("No changes to save.");
      }
    },
    onError: (e) => toast.error(`Save failed: ${e.message}`),
  });

  return (
    <aside
      ref={asideRef}
      className="drawer-in relative flex h-full shrink-0 flex-col border-l border-border bg-surface"
      style={{ width }}
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize details panel"
        title="Drag to resize"
        className="absolute -left-0.5 top-0 z-10 h-full w-1.5 cursor-col-resize hover:bg-accent/50"
        onMouseDown={startResize}
        onDoubleClick={() => setWidth(384)}
      />
      <header className="flex items-center justify-between border-b border-border px-4 py-3">
        <span className="text-sm font-semibold text-text">
          <span className="id-mono text-faint">#{itemId}</span>{" "}
          {detail.data?.work_item_type}
        </span>
        <span className="flex items-center gap-1">
          <button
            aria-label="Open in Azure DevOps"
            title="Open in Azure DevOps"
            className="rounded p-1 text-muted transition-colors hover:text-accent"
            onClick={() =>
              openUrl(
                `https://dev.azure.com/${org}/${encodeURIComponent(project)}/_workitems/edit/${itemId}`,
              ).catch(() => toast.error("Could not open the browser."))
            }
          >
            <ExternalLink size={15} />
          </button>
          <button aria-label="Close details" className="rounded p-1 text-muted hover:text-text" onClick={onClose}>
            <X size={16} />
          </button>
        </span>
      </header>

      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
        {detail.isLoading && <Skeleton className="h-40" />}
        {detail.isError && <p className="text-sm text-danger">{detail.error.message}</p>}

        {detail.data && draft && (
          <>
            <label className="block text-xs text-muted">
              Title
              <Input
                className="mt-1 w-full"
                value={draft.title}
                onChange={(e) => setDraft({ ...draft, title: e.target.value })}
              />
            </label>

            <div className="grid grid-cols-2 gap-2">
              <label className="block text-xs text-muted">
                State
                <Select
                  className="mt-1 w-full"
                  value={draft.state}
                  onChange={(e) => setDraft({ ...draft, state: e.target.value })}
                >
                  {!states.includes(draft.state) && <option>{draft.state}</option>}
                  {states.map((s) => (
                    <option key={s}>{s}</option>
                  ))}
                </Select>
              </label>
              <label className="block text-xs text-muted">
                Assigned to
                <Select
                  className="mt-1 w-full"
                  value={draft.assignedToUnique}
                  onChange={(e) => setDraft({ ...draft, assignedToUnique: e.target.value })}
                >
                  <option value="">(unassigned)</option>
                  {detail.data.assigned_to_unique &&
                    !(members.data ?? []).some(
                      (m) => m.unique_name === detail.data!.assigned_to_unique,
                    ) && (
                      <option value={detail.data.assigned_to_unique}>
                        {detail.data.assigned_to}
                      </option>
                    )}
                  {(members.data ?? []).map((m) => (
                    <option key={m.unique_name} value={m.unique_name}>
                      {m.display_name}
                    </option>
                  ))}
                </Select>
              </label>
            </div>

            {(activities.data?.length ?? 0) > 0 && (
              <label className="block text-xs text-muted">
                Activity
                <Select
                  className="mt-1 w-full"
                  value={draft.activity}
                  onChange={(e) => setDraft({ ...draft, activity: e.target.value })}
                >
                  <option value="">(none)</option>
                  {activities.data!.map((a) => (
                    <option key={a}>{a}</option>
                  ))}
                </Select>
              </label>
            )}

            <div className="grid grid-cols-3 gap-2">
              {(
                [
                  ["Remaining", "remaining"],
                  ["Completed", "completed"],
                  ["Original", "original"],
                ] as const
              ).map(([label, key]) => (
                <label key={key} className="block text-xs text-muted">
                  {label}
                  <Input
                    className="mt-1 w-full px-2"
                    type="number"
                    min="0"
                    step="0.5"
                    value={draft[key]}
                    onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}
                  />
                </label>
              ))}
            </div>

            <div className="grid grid-cols-2 gap-2">
              <div className="text-xs text-muted">
                Start date
                <DateField
                  className="mt-1"
                  ariaLabel="Start date"
                  value={draft.startDate}
                  onChange={(v) => setDraft({ ...draft, startDate: v })}
                />
              </div>
              <div className="text-xs text-muted">
                Finish date
                <DateField
                  className="mt-1"
                  ariaLabel="Finish date"
                  value={draft.finishDate}
                  onChange={(v) => setDraft({ ...draft, finishDate: v })}
                />
              </div>
            </div>

            <div className="block text-xs text-muted">
              <div className="flex items-center justify-between">
                {/* Tab per rich-text section: Description plus whatever the
                    process adds (Bug: RCA, Preventive Measures). */}
                <div className="flex gap-1">
                  {[
                    { key: "", label: "Description" },
                    ...(detail.data.extra_sections ?? []).map((s) => ({
                      key: s.reference_name,
                      label: s.name,
                    })),
                  ].map((t) => (
                    <button
                      key={t.key || "__desc"}
                      className={
                        docTab === t.key
                          ? "rounded px-2 py-0.5 text-[11px] font-medium bg-accent-soft text-accent"
                          : "rounded px-2 py-0.5 text-[11px] text-faint hover:text-text"
                      }
                      onClick={() => setDocTab(t.key)}
                    >
                      {t.label}
                    </button>
                  ))}
                </div>
                <div className="flex gap-1">
                  {(["write", "preview"] as const).map((m) => (
                    <button
                      key={m}
                      className={
                        descMode === m
                          ? "rounded px-2 py-0.5 text-[11px] font-medium bg-accent-soft text-accent"
                          : "rounded px-2 py-0.5 text-[11px] text-faint hover:text-text"
                      }
                      onClick={() => setDescMode(m)}
                    >
                      {m === "write" ? "Write" : "Preview"}
                    </button>
                  ))}
                </div>
              </div>
              {(() => {
                const label = docTab
                  ? ((detail.data.extra_sections ?? []).find((s) => s.reference_name === docTab)
                      ?.name ?? "Section")
                  : "Description";
                const value = docTab ? (draft.extras[docTab] ?? "") : draft.description;
                const setValue = (v: string) =>
                  docTab
                    ? setDraft({ ...draft, extras: { ...draft.extras, [docTab]: v } })
                    : setDraft({ ...draft, description: v });
                return descMode === "write" ? (
                  <Textarea
                    aria-label={`${label} (markdown)`}
                    className="mt-1 h-28 w-full"
                    placeholder="Supports markdown: **bold**, - lists, `code`, [links](url)"
                    value={value}
                    onChange={(e) => setValue(e.target.value)}
                  />
                ) : (
                  <div
                    className="md-preview mt-1 min-h-28 w-full rounded-md border border-border bg-bg px-3 py-2 text-sm text-text"
                    // Rendered from the user's own local draft only.
                    dangerouslySetInnerHTML={{
                      __html: marked.parse(value || "*Nothing to preview*", {
                        async: false,
                        breaks: true,
                      }),
                    }}
                  />
                );
              })()}
            </div>

            <div className="text-xs text-faint">
              {detail.data.area_path} · {detail.data.iteration_path}
            </div>

            <Button size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
              {save.isPending ? "Saving" : "Save changes"}
            </Button>

            <CommentsPanel org={org} project={project} itemId={itemId} />
          </>
        )}
      </div>
    </aside>
  );
}
