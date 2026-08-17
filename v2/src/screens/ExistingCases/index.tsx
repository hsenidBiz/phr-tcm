import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { save } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { commands, type TestCaseFull } from "../../bindings";
import BulkEditDialog from "../../components/BulkEditDialog";
import PowerRenameDialog, { type RenameTarget } from "../../components/PowerRenameDialog";
import DeleteConfirm from "../../components/DeleteConfirm";
import RelinkDialog from "../../components/RelinkDialog";
import CaseEditor from "./CaseEditor";
import CountUp from "../../components/CountUp";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
import { Skeleton } from "../../components/ui/skeleton";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { cn } from "../../lib/cn";
import { exportPathFor, rememberExportPath } from "../../lib/exportDir";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import { groupIndices } from "../../lib/grouping";
import { unwrap } from "../../lib/ipc";
import { toTestCase } from "../../lib/testCaseConvert";
import { IconBulkEdit, IconClear, IconExport, IconRemove, IconRename, IconMoveToPbi } from "../../lib/actionIcons";

/** The Edit tab: click selects a card, ctrl+click toggles, shift+click
 * ranges; the chevron (or double-click) expands the editor. Selection
 * unlocks the bulk toolbar. Grouping by module is the v1 smart grouping. */
export default function ExistingCases({
  org,
  project,
  pbiId,
  caseIds,
  label,
}: {
  org: string;
  project: string;
  pbiId: number | null;
  /** When set, edit these exact cases (suite handoff) instead of a PBI's. */
  caseIds?: number[];
  label?: string;
}) {
  const qc = useQueryClient();
  const { prefs } = useFieldRefs(org, project);
  const [openId, setOpenId] = useState<number | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [bulkOpen, setBulkOpen] = useState(false);
  const [renameOpen, setRenameOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [relinkOpen, setRelinkOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-group-cases") === "on",
  );
  const [collapsedGroups, toggleCollapsed] = usePersistedStringSet(
    "tcm-v2-edit-collapsed-groups",
  );

  const queryKey = caseIds
    ? ["cases-by-ids", org, caseIds, prefs.moduleRef, prefs.preconditionsRef]
    : ["pbi-tcs", org, pbiId, prefs.moduleRef, prefs.preconditionsRef];

  const cases = useQuery({
    queryKey,
    queryFn: () =>
      caseIds
        ? unwrap(commands.testCasesByIds(org, caseIds, prefs.moduleRef, prefs.preconditionsRef))
        : unwrap(commands.pbiTestCasesFull(org, pbiId!, prefs.moduleRef, prefs.preconditionsRef)),
    enabled: Boolean(org && (caseIds ? caseIds.length > 0 : pbiId != null)),
    retry: false,
  });

  /** Whether Azure DevOps says this user may delete here. App.tsx asks
   *  this at SIGN-IN on the same cache key, so opening this screen reads
   *  the answer that is already in rather than fetching - the button's
   *  presence is decided when the user logs in, not when they arrive
   *  here. The client fails closed on every uncertain answer, so `false`
   *  covers "not allowed", "could not ask" and "answered something
   *  unexpected" alike - and the button simply does not exist rather
   *  than failing when pressed. */
  const canDelete = useQuery({
    // Keyed by the PBI: the probe asks about the PBI's OWN area node,
    // because area permissions are per node - the root said yes to a
    // user the TCM API then refused. The sign-in-time root check in
    // App.tsx warms the coarse answer; this one decides the button.
    queryKey: ["can-delete", org, project, pbiId],
    queryFn: () => unwrap(commands.canDeleteTestCases(org, project, pbiId)),
    enabled: Boolean(org && project),
    staleTime: Infinity,
    retry: false,
  });

  const list = cases.data ?? [];
  const q = search.trim().toLowerCase();
  // Search narrows the visible cards (title, #id or tag); grouping and the
  // header count follow the filtered view.
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
    // v1 smart grouping: cluster by shared title prefixes.
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
      // Clicking the sole highlighted case again deselects it.
      setSelected((s) => (s.size === 1 && s.has(c.id) ? new Set() : new Set([c.id])));
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

  /** How many of a group's cases are highlighted - drives the collapsed
   * group's marker. Counted rather than a boolean so the label can say
   * how much is hidden in there. */
  const selectedInGroup = (items: TestCaseFull[]) =>
    items.reduce((n, c) => (selected.has(c.id) ? n + 1 : n), 0);

  const selectedCases = list.filter((c) => selected.has(c.id));

  /** Only titles change: the case's own steps_xml goes back untouched, so
   *  the save leaves Steps out of the patch entirely. The rows carry the
   *  exact strings the preview displayed - nothing is recomputed here. */
  const renameTarget: RenameTarget = {
    label: "Azure DevOps",
    cases: selectedCases.map((c) => ({ id: c.id, title: c.title })),
    otherTitles: list.filter((c) => !selected.has(c.id)).map((c) => c.title),
    undoable: true,
    apply: async (rows) => {
      const failed: typeof rows = [];
      for (const row of rows) {
        const full = list.find((c) => c.id === row.id);
        if (!full) {
          failed.push(row);
          continue;
        }
        const r = await commands.updateTestCase(
          org,
          project,
          { ...toTestCase(full), title: row.after, update_id: full.id },
          prefs.moduleRef,
          prefs.preconditionsRef,
          full.steps_xml,
        );
        if (r.status === "error") failed.push(row);
      }
      return failed;
    },
  };

  const exportJson = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: exportPathFor("test-cases.json"),
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      rememberExportPath(path);
      const r = await commands.exportQueueJson(path, selectedCases.map(toTestCase));
      if (r.status === "error") throw new Error(r.error);
      toast.success(`Exported ${selectedCases.length} case(s).`);
    },
    onError: (e) => toast.error(`Export failed: ${e.message}`),
  });

  const refresh = () => {
    qc.invalidateQueries({ queryKey });
    qc.invalidateQueries({ queryKey: ["pbi-tc-titles", org, pbiId] });
  };

  return (
    <section className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-sm font-semibold text-muted">
          {label ? (
            `${label} (${q ? `${visible.length}/${list.length}` : list.length})`
          ) : q ? (
            `${visible.length} of ${list.length} Test Cases`
          ) : (
            <>
              <CountUp to={list.length} duration={0.8} /> Total Test Cases
            </>
          )}
        </h2>
        <button
          aria-label="Refresh"
          title="Refresh"
          className="rounded p-1 text-muted hover:text-accent"
          onClick={refresh}
        >
          <RefreshCw size={14} className={cases.isFetching ? "animate-spin" : undefined} />
        </button>
        {/* Viewing (incl. the browser report) lives in the View Test Cases
            tab now - this screen stays focused on editing. */}
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
                  localStorage.setItem("tcm-v2-group-cases", v ? "on" : "off");
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
          <Button size="sm" onClick={() => setBulkOpen(true)}>
            <IconBulkEdit aria-hidden />
            Bulk edit
          </Button>
          <Button size="sm" onClick={() => setRenameOpen(true)}>
            <IconRename aria-hidden />
            Power Rename
          </Button>
          <Button variant="outline" size="sm" onClick={() => exportJson.mutate()}>
            <IconExport aria-hidden />
            Export JSON
          </Button>
          {canDelete.data === true && (
            <Button variant="danger" size="sm" onClick={() => setDeleteOpen(true)}>
              <IconRemove aria-hidden />
              Delete
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            disabled={pbiId == null}
            title="Move the selected test cases to a different PBI"
            onClick={() => setRelinkOpen(true)}
          >
            <IconMoveToPbi aria-hidden />
            Move to PBI
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setSelected(new Set())}>
            <IconClear aria-hidden />
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
                className="group flex items-center gap-2"
                title="Select all test cases in this group"
                onClick={() => toggleGroup(items)}
              >
                <span className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                  {group} ({items.length})
                </span>
                {/* Same marker as View Test Cases: folding a group hides its
                    rows and the highlight with them, so the heading has to
                    say something is still selected in there. */}
                {collapsedGroups.has(group) && selectedInGroup(items) > 0 && (
                  <span
                    className="selection-dot"
                    role="status"
                    aria-label={`${selectedInGroup(items)} of ${items.length} selected in ${group}`}
                    title={`${selectedInGroup(items)} selected in this group`}
                  />
                )}
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
                  <span className="ml-auto text-xs text-faint">
                    {c.steps.length} steps · {c.automation_status}
                  </span>
                </div>
                {openId === c.id && (
                  <CaseEditor
                    original={c}
                    org={org}
                    project={project}
                    moduleRef={prefs.moduleRef}
                    preconditionsRef={prefs.preconditionsRef}
                    onSaved={refresh}
                  />
                )}
              </li>
            ))}
          </ul>
          )}
        </div>
      ))}

      {deleteOpen && (
        <DeleteConfirm
          org={org}
          project={project}
          cases={selectedCases.map((c) => ({ id: c.id, title: c.title }))}
          onClose={() => setDeleteOpen(false)}
          onDeleted={() => {
            // An expanded editor for a case that is now gone would save it
            // straight back as a new one.
            setOpenId(null);
            setSelected(new Set());
            refresh();
          }}
        />
      )}

      {relinkOpen && pbiId != null && (
        <RelinkDialog
          org={org}
          project={project}
          fromPbi={pbiId}
          cases={selectedCases}
          onClose={() => setRelinkOpen(false)}
          onMoved={() => {
            // Same hygiene as a delete: an expanded editor for a case that
            // has left this PBI would save it straight back here.
            setOpenId(null);
            setSelected(new Set());
            refresh();
          }}
        />
      )}

      {renameOpen && (
        <PowerRenameDialog
          target={renameTarget}
          // Closing is not finishing. This used to clear the selection on
          // the way out, which meant Cancel threw away the very selection
          // the user had just built in order to rename it - and left them
          // re-picking every case to try again.
          onClose={() => setRenameOpen(false)}
          onDone={() => {
            // Same reason Bulk Edit collapses them: an expanded editor still
            // holds the pre-rename title and would put it back on save.
            if (openId != null && selected.has(openId)) setOpenId(null);
            setSelected(new Set());
            refresh();
          }}
        />
      )}

      {bulkOpen && (
        <BulkEditDialog
          org={org}
          project={project}
          cases={selectedCases}
          onClose={() => setBulkOpen(false)}
          onDone={() => {
            setBulkOpen(false);
            // A CaseEditor seeds its draft from the case ONCE, when it
            // opens. If one of the cases the bulk edit just changed was
            // left expanded, that draft is now the pre-edit version - and
            // the next save from it would put the old values back over the
            // change the user asked for. Collapse it so it re-seeds.
            if (openId != null && selected.has(openId)) setOpenId(null);
            setSelected(new Set());
            refresh();
          }}
        />
      )}
    </section>
  );
}
