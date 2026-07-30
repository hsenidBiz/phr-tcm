import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ExternalLink, X } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { marked } from "marked";
import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { toast } from "sonner";
import { commands, type WorkItemDetail } from "../bindings";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";
import { cached } from "../lib/localCache";
import { renderMarkdown } from "../lib/markdown";
import { htmlToMd } from "../lib/richText";
import { Button } from "./ui/button";
import DateField from "./ui/datefield";
import MarkdownField from "./MarkdownField";
import { Input } from "./ui/input";
import { Select } from "./ui/select";
import { Skeleton } from "./ui/skeleton";
import CommentsPanel from "./CommentsPanel";
import HistoryPanel from "./HistoryPanel";
import { IconCancel, IconConfirm } from "../lib/actionIcons";

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
  /** Every field on the extra form pages (Bug: RCA / Preventive Measures),
   * keyed by reference name - markdown for html fields, raw otherwise. */
  extras: Record<string, string>;
};

/** A field's editor-facing value: markdown for rich text, raw otherwise. */
function extraValue(f: { kind: string; value: string }): string {
  return f.kind === "html" ? htmlToMd(f.value) : f.value;
}

function toDraft(d: WorkItemDetail): Draft {
  return {
    extras: Object.fromEntries(
      // ?? []: tolerate cached details from before this field existed.
      (d.extra_pages ?? []).flatMap((p) =>
        p.fields.map((f) => [f.reference_name, extraValue(f)]),
      ),
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
      ? htmlToMd(d.description_html)
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
  highlightFields = [],
  onClose,
  onSaved,
}: {
  org: string;
  project: string;
  itemId: number;
  states: string[];
  /** Fields ADO named in a rule error (e.g. a blocked board move) -
   * shown in a banner and ringed on their editors. */
  highlightFields?: string[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const qc = useQueryClient();
  // Loose two-way match: ADO says "Remaining Work", our label says
  // "Remaining"; either containing the other counts.
  const isFlagged = (label: string) =>
    highlightFields.some((f) => {
      const a = f.toLowerCase();
      const b = label.toLowerCase();
      return a.includes(b) || b.includes(a);
    });
  const detail = useQuery({
    queryKey: ["wi-detail", org, project, itemId],
    queryFn: () => unwrap(commands.workItemDetail(org, project, itemId)),
    retry: false,
  });

  const members = useQuery({
    queryKey: ["members", org, project],
    // v1 cached members for 24h; the local cache carries that across
    // restarts too (big orgs, slow endpoint).
    queryFn: () =>
      cached(`members:${org}/${project}`, 24 * 60 * 60_000, () =>
        unwrap(commands.listTeamMembers(org, project)),
      ),
    staleTime: 24 * 60 * 60_000,
  });

  const activities = useQuery({
    queryKey: ["activities", org, project, detail.data?.work_item_type],
    queryFn: () => unwrap(commands.activityValues(org, project, detail.data!.work_item_type)),
    enabled: Boolean(detail.data),
    staleTime: Infinity,
  });

  const [draft, setDraft] = useState<Draft | null>(null);
  // Rich text opens rendered (like ADO's own form); Write is for editing.
  const [descMode, setDescMode] = useState<"write" | "preview">("preview");
  // Per-field editing, so clicking ONE rendered block opens that one -
  // the header Write/Preview buttons still flip everything at once.
  // Keyed by "" for Description, else the field's reference name.
  const [editingFields, setEditingFields] = useState<Set<string>>(new Set());
  const isEditing = (id: string) => descMode === "write" || editingFields.has(id);
  const startEditing = (id: string) =>
    setEditingFields((prev) => new Set(prev).add(id));
  const setMode = (m: "write" | "preview") => {
    setDescMode(m);
    // Leaving Write closes the per-field editors too, or "Preview" would
    // appear to do nothing on a field opened by clicking it.
    if (m === "preview") setEditingFields(new Set());
  };
  // Which rich-text tab is active: "" = Description, else the extra
  // section's reference name (Bug: RCA / Preventive Measures).
  const [docTab, setDocTab] = useState("");
  const [bottomTab, setBottomTab] = useState<"discussion" | "history">("discussion");

  // Esc closes (X too); no overlay-click close so edits can't be lost by a
  // stray click.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    if (detail.data) {
      setDraft(toDraft(detail.data));
      setDocTab(""); // back to Description when a different item loads
      setDescMode("preview");
      setEditingFields(new Set());
      setBottomTab("discussion");
    }
  }, [detail.data]);

  /** Preview-only: swap authenticated attachment URLs for the data: URIs
   * Rust downloaded (a plain <img> gets 401). The markdown drafts keep the
   * real URLs so saves round-trip them, not megabytes of base64. */
  const withInlineImages = (html: string) => {
    let out = html;
    for (const img of detail.data?.inline_images ?? []) {
      out = out
        .split(img.url.replace(/&/g, "&amp;"))
        .join(img.data)
        .split(img.url)
        .join(img.data);
    }
    return out;
  };
  const renderMd = (md: string) =>
    withInlineImages(renderMarkdown(md || "*Nothing to preview*"));

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
      // Extra form pages (Bug: RCA / Preventive Measures) save the same
      // way - only fields that actually changed; rich text goes back as
      // HTML, picklists and plain fields as raw values.
      for (const f of (d.extra_pages ?? []).flatMap((p) => p.fields)) {
        const now = dr.extras[f.reference_name] ?? "";
        if (now === extraValue(f)) continue;
        patches.push({
          reference_name: f.reference_name,
          value:
            f.kind === "html"
              ? `<div>${marked.parse(now, { async: false, breaks: true })}</div>`
              : now,
        });
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

  // Portalled for the same reason as ui/modal.tsx and CommentModal: this
  // is opened from the board, which renders inside AnimatedContent, and a
  // `fixed` overlay inside that GSAP transform is positioned against the
  // scrollable wrapper rather than the viewport.
  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-label={`Work item ${itemId}`}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-3 md:px-10 md:py-6"
    >
      <div className="modal-in flex h-full w-full flex-col overflow-hidden rounded-lg border border-border bg-surface shadow-2xl">
        {/* The modal covers the window's title-bar drag region, so its own
            header doubles as one - drag it to move the window (buttons and
            the title text opt out so they stay clickable/selectable). */}
        <header
          data-tauri-drag-region
          className="flex select-none items-center justify-between border-b border-border px-5 py-3"
        >
          <span data-tauri-drag-region={false} className="text-sm font-semibold text-text">
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

        {highlightFields.length > 0 && (
          <p className="border-b border-danger/40 bg-danger/10 px-5 py-2 text-xs text-danger">
            Azure DevOps requires these fields before the state can change:{" "}
            <span className="font-semibold">{highlightFields.join(", ")}</span> - they're
            highlighted below.
          </p>
        )}

        {(detail.isLoading || detail.isError) && (
          <div className="space-y-3 p-5">
            {detail.isLoading && <Skeleton className="h-40" />}
            {detail.isError && <p className="text-sm text-danger">{detail.error.message}</p>}
          </div>
        )}

        {detail.data && draft && (
          <>
            {/* ADO-style form head: full-width title, then the state row. */}
            <div className="space-y-2 border-b border-border px-5 py-3">
              <label className="block text-xs text-muted">
                Title
                <Input
                  className="mt-1 w-full"
                  value={draft.title}
                  onChange={(e) => setDraft({ ...draft, title: e.target.value })}
                />
              </label>
              <div className="grid grid-cols-2 gap-2 lg:grid-cols-3">
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
              </div>
            </div>

            {/* Body: rich text + comments left, planning details right. */}
            <div className="grid min-h-0 flex-1 grid-cols-1 lg:grid-cols-[1fr_280px]">
              <div className="min-h-0 space-y-3 overflow-y-auto p-5 lg:border-r lg:border-border">
                <div className="block text-xs text-muted">
              <div className="flex items-center justify-between">
                {/* Tab per form page: Description plus whatever the process
                    adds (Bug: RCA, Preventive Measures). */}
                <div className="flex flex-wrap gap-1">
                  {[
                    { key: "", label: "Description" },
                    ...(detail.data.extra_pages ?? []).map((p) => ({
                      key: p.name,
                      label: p.name,
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
                      onClick={() => setMode(m)}
                    >
                      {m === "write" ? "Write" : "Preview"}
                    </button>
                  ))}
                </div>
              </div>

              {detail.data.extra_pages_error && (
                <p className="mt-1 text-[11px] text-faint">
                  Process tabs unavailable: {detail.data.extra_pages_error}
                </p>
              )}

              {docTab === "" ? (
                <MarkdownField
                  label="Description"
                  value={draft.description}
                  onChange={(v) => setDraft({ ...draft, description: v })}
                  editing={isEditing("")}
                  onStartEditing={() => startEditing("")}
                  // Rendered from the user's own local draft only.
                  renderHtml={renderMd}
                />
              ) : (
                // An extra form page, laid out like ADO's form: each layout
                // section becomes a column (stacking on narrow windows).
                <div
                  className="extra-grid mt-1"
                  style={{
                    ["--cols" as string]: Math.min(
                      3,
                      new Set(
                        (detail.data.extra_pages ?? [])
                          .find((p) => p.name === docTab)
                          ?.fields.map((f) => f.section) ?? [],
                      ).size || 1,
                    ),
                  }}
                >
                  {[
                    ...new Set(
                      (detail.data.extra_pages ?? [])
                        .find((p) => p.name === docTab)
                        ?.fields.map((f) => f.section) ?? [],
                    ),
                  ].map((sec) => (
                    <div key={sec} className="space-y-3">
                      {(detail.data!.extra_pages ?? [])
                        .find((p) => p.name === docTab)
                        ?.fields.filter((f) => f.section === sec)
                        .map((f) => {
                      const value = draft.extras[f.reference_name] ?? "";
                      const setValue = (v: string) =>
                        setDraft({
                          ...draft,
                          extras: { ...draft.extras, [f.reference_name]: v },
                        });
                      if (f.kind === "pick") {
                        return (
                          <label key={f.reference_name} className="flex flex-col gap-1">
                            {f.label}
                            <Select
                              value={value}
                              onChange={(e) => setValue(e.target.value)}
                              aria-label={f.label}
                              className={cn(isFlagged(f.label) && "ring-2 ring-danger")}
                            >
                              <option value="">(none)</option>
                              {!f.allowed.includes(value) && value && <option>{value}</option>}
                              {f.allowed.map((a) => (
                                <option key={a}>{a}</option>
                              ))}
                            </Select>
                          </label>
                        );
                      }
                      if (f.kind === "text") {
                        return (
                          <label key={f.reference_name} className="flex flex-col gap-1">
                            {f.label}
                            <Input
                              aria-label={f.label}
                              className={cn(isFlagged(f.label) && "ring-2 ring-danger")}
                              value={value}
                              onChange={(e) => setValue(e.target.value)}
                            />
                          </label>
                        );
                      }
                      return (
                        <div key={f.reference_name}>
                          <span>{f.label}</span>
                          <MarkdownField
                            label={f.label}
                            value={value}
                            onChange={setValue}
                            editing={isEditing(f.reference_name)}
                            onStartEditing={() => startEditing(f.reference_name)}
                            renderHtml={renderMd}
                            rows="h-24"
                            previewMinHeight="min-h-16"
                            flagged={isFlagged(f.label)}
                          />
                        </div>
                      );
                        })}
                    </div>
                  ))}
                </div>
              )}
            </div>

                <div className="space-y-2">
                  {/* Discussion and History share this slot: both are
                      "what happened to this item", and stacking them
                      both would push the fields off-screen. */}
                  <div className="flex gap-1 border-b border-border pb-1">
                    {(["discussion", "history"] as const).map((t) => (
                      <button
                        key={t}
                        aria-pressed={bottomTab === t}
                        className={
                          bottomTab === t
                            ? "rounded px-2 py-0.5 text-[11px] font-medium bg-accent-soft text-accent"
                            : "rounded px-2 py-0.5 text-[11px] text-faint hover:text-text"
                        }
                        onClick={() => setBottomTab(t)}
                      >
                        {t === "discussion" ? "Discussion" : "History"}
                      </button>
                    ))}
                  </div>
                  {bottomTab === "discussion" ? (
                    <CommentsPanel org={org} project={project} itemId={itemId} />
                  ) : (
                    <HistoryPanel org={org} project={project} itemId={itemId} />
                  )}
                </div>
              </div>

              <div className="space-y-3 overflow-y-auto p-5">
                <h3 className="text-xs font-semibold uppercase tracking-wide text-faint">
                  Planning
                </h3>
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
                      className={cn(
                        "mt-1 w-full px-2",
                        // "Remaining" flags for ADO's "Remaining Work" etc.
                        isFlagged(`${label} Work`) && "ring-2 ring-danger",
                      )}
                      type="number"
                      min="0"
                      // "any", not a 0.5 grid: these map to ADO Double
                      // fields that happily hold 6.8, and a step the value
                      // does not land on makes the input :invalid - which
                      // is what raised the browser's own "Please enter a
                      // valid value" bubble over our tooltip.
                      step="any"
                      value={draft[key]}
                      onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}
                    />
                  </label>
                ))}
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
                <h3 className="pt-2 text-xs font-semibold uppercase tracking-wide text-faint">
                  Classification
                </h3>
                <p className="break-words text-xs text-faint">
                  {detail.data.area_path}
                  <br />
                  {detail.data.iteration_path}
                </p>
              </div>
            </div>

            <footer className="flex items-center justify-end gap-2 border-t border-border px-5 py-3">
              <Button variant="ghost" size="sm" onClick={onClose}>
                <IconCancel aria-hidden />
                Close
              </Button>
              <Button size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
                <IconConfirm aria-hidden />
                {save.isPending ? "Saving" : "Save changes"}
              </Button>
            </footer>
          </>
        )}
      </div>
    </div>,
    document.body,
  );
}
