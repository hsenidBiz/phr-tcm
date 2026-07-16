import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, MessageSquare, MessageSquarePlus, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { commands, type PbiHit, type TestCase, type TestCaseFull } from "../bindings";
import PickPbiEmpty from "../components/PickPbiEmpty";
import { Button } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import { Input, Textarea } from "../components/ui/input";
import { Skeleton } from "../components/ui/skeleton";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { loadNotes, saveNote } from "../lib/caseNotes";
import { cn } from "../lib/cn";
import { usePersistedStringSet } from "../lib/collapsedGroups";
import { groupIndices } from "../lib/grouping";
import { unwrap, unwrapStr } from "../lib/ipc";

function toTestCase(c: TestCaseFull): TestCase {
  return {
    title: c.title,
    steps: c.steps,
    tags: c.tags,
    automation_status: c.automation_status,
    module_value: c.module_value,
    preconditions: c.preconditions,
    update_id: c.id,
  };
}

/** The expanded (read-only) detail: preconditions, steps, and the personal
 * local comment - a scratchpad saved on this machine only, never written
 * to Azure DevOps. */
function CaseDetail({
  c,
  note,
  onSaveNote,
}: {
  c: TestCaseFull;
  note: string;
  onSaveNote: (text: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(note);

  return (
    <div className="border-t border-border" onClick={(e) => e.stopPropagation()}>
      {c.tags && (
        <div className="flex flex-wrap items-center gap-1 border-b border-border/60 px-3 py-2">
          {c.tags
            .split(";")
            .map((t) => t.trim())
            .filter(Boolean)
            .map((t) => (
              <span
                key={t}
                className="rounded-full bg-surface-2 px-2 py-0.5 text-[11px] text-muted"
              >
                {t}
              </span>
            ))}
        </div>
      )}
      {c.preconditions && (
        <p className="whitespace-pre-wrap border-b border-border/60 px-3 py-2 text-xs text-muted">
          <span className="font-semibold">Preconditions: </span>
          {c.preconditions}
        </p>
      )}

      {c.steps.length > 0 ? (
        <table className="w-full border-collapse text-xs">
          <thead>
            <tr className="text-left text-faint">
              <th className="w-8 px-3 py-1 font-medium">#</th>
              <th className="px-3 py-1 font-medium">Action</th>
              <th className="px-3 py-1 font-medium">Expected</th>
            </tr>
          </thead>
          <tbody>
            {c.steps.map((s, i) => (
              <tr key={i} className="border-t border-border/40 align-top">
                <td className="px-3 py-1 text-faint">{i + 1}</td>
                <td className="whitespace-pre-wrap px-3 py-1 text-text">{s.action}</td>
                <td className="whitespace-pre-wrap px-3 py-1 text-muted">{s.expected}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p className="px-3 py-2 text-xs text-muted">This test case has no steps.</p>
      )}

      <div className="border-t border-border/60 px-3 py-2">
        {editing ? (
          <div className="space-y-2">
            <Textarea
              aria-label={`Comment for #${c.id}`}
              className="h-20 w-full"
              placeholder="e.g. Step 3 needs the new confirmation dialog"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
            />
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                onClick={() => {
                  onSaveNote(draft);
                  setEditing(false);
                }}
              >
                Save comment
              </Button>
              <Button
                variant="ghost"
                size="sm"
                onClick={() => {
                  setDraft(note);
                  setEditing(false);
                }}
              >
                Cancel
              </Button>
              <span className="text-[11px] text-faint">
                Saved on this device only — never sent to Azure DevOps.
              </span>
            </div>
          </div>
        ) : note ? (
          <div className="flex items-start gap-2 rounded-md bg-accent-soft/60 px-2.5 py-1.5">
            <p className="whitespace-pre-wrap text-xs text-text">{note}</p>
            <button
              className="ml-auto shrink-0 text-xs text-accent hover:underline"
              onClick={() => {
                setDraft(note);
                setEditing(true);
              }}
            >
              Edit
            </button>
          </div>
        ) : (
          <button
            className="flex items-center gap-1.5 rounded px-1 py-0.5 text-xs text-faint transition-colors hover:text-accent"
            onClick={() => {
              setDraft("");
              setEditing(true);
            }}
          >
            <MessageSquarePlus size={13} />
            Add comment
          </button>
        )}
      </div>
    </div>
  );
}

/** The View Test Cases tab, laid out like Edit Test Cases: compact rows
 * (chevron/double-click expands a read-only detail), Group by Title,
 * click/ctrl/shift selection - and View in browser renders the selection
 * (or everything the filter shows when nothing is selected). */
export default function ViewCases({
  org,
  project,
  pbi,
  onPickPbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  onPickPbi?: (pbi: PbiHit) => void;
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const [search, setSearch] = useState("");
  const [openId, setOpenId] = useState<number | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [notes, setNotes] = useState<Record<string, string>>(() => loadNotes(org));
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-group-view") === "on",
  );
  const [collapsedGroups, toggleCollapsed] = usePersistedStringSet(
    "tcm-v2-view-collapsed-groups",
  );

  const pbiId = pbi?.id ?? null;
  // Same key as Edit Test Cases so tab switches reuse the cached fetch.
  const queryKey = ["pbi-tcs", org, pbiId, prefs.moduleRef, prefs.preconditionsRef];
  const cases = useQuery({
    queryKey,
    queryFn: () =>
      unwrap(commands.pbiTestCasesFull(org, pbiId!, prefs.moduleRef, prefs.preconditionsRef)),
    enabled: Boolean(org && pbiId != null),
    retry: false,
  });

  const list = cases.data ?? [];
  const q = search.trim().toLowerCase();
  const visible = useMemo(
    () =>
      q
        ? list.filter(
            (c) =>
              c.title.toLowerCase().includes(q) ||
              `#${c.id}`.includes(q) ||
              String(c.id).includes(q) ||
              c.tags.toLowerCase().includes(q),
          )
        : list,
    [list, q],
  );

  const ordered = useMemo(() => {
    if (!grouped) return [{ group: "", items: visible }];
    return groupIndices(visible.map((c) => c.title)).map(({ name, indices }) => ({
      group: name || "Ungrouped",
      items: indices.map((i) => visible[i]),
    }));
  }, [visible, grouped]);
  const flat = useMemo(() => ordered.flatMap((g) => g.items), [ordered]);

  const handleCardClick = (c: TestCaseFull, e: React.MouseEvent) => {
    const idx = flat.findIndex((x) => x.id === c.id);
    if (e.shiftKey && anchor != null) {
      const a = flat.findIndex((x) => x.id === anchor);
      if (a >= 0 && idx >= 0) {
        const [lo, hi] = a < idx ? [a, idx] : [idx, a];
        const range = flat.slice(lo, hi + 1).map((x) => x.id);
        setSelected((s) => (e.ctrlKey || e.metaKey ? new Set([...s, ...range]) : new Set(range)));
        return;
      }
    }
    if (e.ctrlKey || e.metaKey) {
      setSelected((s) => {
        const next = new Set(s);
        if (next.has(c.id)) next.delete(c.id);
        else next.add(c.id);
        return next;
      });
    } else {
      setSelected(new Set([c.id]));
    }
    setAnchor(c.id);
  };

  /** Header click: select every case in the group (click again to clear). */
  const toggleGroup = (items: TestCaseFull[]) => {
    const ids = items.map((c) => c.id);
    setSelected((s) => {
      const all = ids.every((id) => s.has(id));
      const next = new Set(s);
      if (all) ids.forEach((id) => next.delete(id));
      else ids.forEach((id) => next.add(id));
      return next;
    });
    if (ids.length) setAnchor(ids[0]);
  };

  const chosen = selected.size > 0 ? visible.filter((c) => selected.has(c.id)) : visible;
  const viewHtml = useMutation({
    mutationFn: () =>
      unwrapStr(
        commands.viewQueueHtml(
          chosen.map(toTestCase),
          pbiId != null ? `PBI #${pbiId}` : "",
          org,
          notes,
        ),
      ),
    onError: (e) => toast.error(`Could not open the report: ${e.message ?? e}`),
  });

  // Comments typed in the browser report autosave into localStorage via the
  // App-level listener - re-read them when the user comes back to the app.
  useEffect(() => {
    const refresh = () => setNotes(loadNotes(org));
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [org]);

  if (!org || !project || !pbi) {
    return (
      <PickPbiEmpty
        message="Pick an organization, project and PBI in the bar above to view its test cases."
        org={org}
        project={project}
        onPickPbi={onPickPbi}
      />
    );
  }

  return (
    <section className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-sm font-semibold text-muted">
          {q ? `${visible.length} of ${list.length} Test Cases` : `${list.length} Total Test Cases`}
        </h2>
        <button
          aria-label="Refresh"
          title="Refresh"
          className="rounded p-1 text-muted hover:text-accent"
          onClick={() => qc.invalidateQueries({ queryKey })}
        >
          <RefreshCw size={14} />
        </button>
        <div className="ml-auto">
          <Button
            variant="outline"
            size="sm"
            disabled={chosen.length === 0 || viewHtml.isPending}
            onClick={() => viewHtml.mutate()}
          >
            {selected.size > 0 ? `View ${selected.size} in browser` : "View in browser"}
          </Button>
        </div>
      </div>

      {list.length > 0 && (
        <div className="flex gap-2">
          <Input
            aria-label="Search test cases"
            className="w-56 px-2 py-1"
            placeholder="Filter by name or id"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          <label className="flex items-center gap-1.5 text-xs text-muted">
            <Checkbox
              checked={grouped}
              onCheckedChange={(v) => {
                setGrouped(v);
                try {
                  localStorage.setItem("tcm-v2-group-view", v ? "on" : "off");
                } catch {
                  // session-only
                }
              }}
            />
            Group by title
          </label>
        </div>
      )}

      {selected.size > 0 && (
        <div className="flex items-center gap-2 rounded-md border border-accent/40 bg-accent-soft px-3 py-1.5 text-sm">
          <span className="font-medium text-accent">{selected.size} selected</span>
          <Button variant="ghost" size="sm" onClick={() => setSelected(new Set())}>
            Clear
          </Button>
          <span className="ml-auto text-xs text-faint">Ctrl+click to toggle · Shift+click for range</span>
        </div>
      )}

      {cases.isLoading && <Skeleton className="h-24" />}
      {cases.isError && <p className="text-sm text-danger">{cases.error.message}</p>}
      {cases.data && list.length === 0 && (
        <p className="text-sm text-muted">No test cases here yet.</p>
      )}
      {list.length > 0 && visible.length === 0 && (
        <p className="text-sm text-muted">No test cases match "{search.trim()}".</p>
      )}

      {ordered.map(({ group, items }) => (
        <div key={group || "__all"} className="space-y-1">
          {group && (
            <div className="flex w-full items-center gap-3 pb-1 pt-2">
              <span aria-hidden className="h-px flex-1 bg-border" />
              <button
                aria-label={`${collapsedGroups.has(group) ? "Expand" : "Collapse"} group ${group}`}
                title={collapsedGroups.has(group) ? "Expand group" : "Collapse group"}
                className="text-muted transition-colors hover:text-accent"
                onClick={() => toggleCollapsed(group)}
              >
                {collapsedGroups.has(group) ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
              </button>
              <button
                className="group"
                title="Select all test cases in this group"
                onClick={() => toggleGroup(items)}
              >
                <span className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                  {group} ({items.length})
                </span>
              </button>
              <span aria-hidden className="h-px flex-1 bg-border" />
            </div>
          )}
          {group && collapsedGroups.has(group) ? null : (
            <ul className="space-y-1">
              {items.map((c) => (
                <li
                  key={c.id}
                  className={cn(
                    "cursor-pointer select-none rounded-md border transition-colors",
                    selected.has(c.id)
                      ? "border-accent bg-accent-soft"
                      : "border-border hover:border-border-strong",
                  )}
                  onClick={(e) => handleCardClick(c, e)}
                  onDoubleClick={() => setOpenId((o) => (o === c.id ? null : c.id))}
                >
                  <div className="flex items-center gap-2 px-3 py-2 text-sm">
                    <button
                      aria-label={`Expand #${c.id}`}
                      className="text-muted hover:text-accent"
                      onClick={(e) => {
                        e.stopPropagation();
                        setOpenId((o) => (o === c.id ? null : c.id));
                      }}
                    >
                      {openId === c.id ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                    </button>
                    <span className="id-mono text-faint">#{c.id}</span>
                    <span className="text-text">{c.title}</span>
                    {notes[String(c.id)] && (
                      <span
                        aria-label="Has a local comment"
                        title={notes[String(c.id)]}
                        className="flex shrink-0 items-center gap-1 rounded-full bg-accent-soft px-2 py-0.5 text-[11px] font-medium text-accent"
                      >
                        <MessageSquare size={11} />
                        Comment
                      </span>
                    )}
                    <span className="ml-auto text-xs text-faint">
                      {c.steps.length} steps · {c.automation_status}
                    </span>
                  </div>
                  {openId === c.id && (
                    <CaseDetail
                      c={c}
                      note={notes[String(c.id)] ?? ""}
                      onSaveNote={(text) => setNotes(saveNote(org, c.id, text))}
                    />
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      ))}
    </section>
  );
}
