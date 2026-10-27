import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, MessageSquare } from "lucide-react";
import { memo, useEffect, useMemo, useRef, useState, useSyncExternalStore, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { toast } from "../../lib/toast";
import { commands, type PbiHit, type TestCaseFull } from "../../bindings";
import CountTo from "../../components/CountTo";
import PickPbiEmpty from "../../components/PickPbiEmpty";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Collapse, useRegroupMotion, useSettled } from "../../components/ui/collapse";
import { ListToolbar, ToolbarSwitch } from "../../components/ui/list-toolbar";
import { PageHeader } from "../../components/ui/page-header";
import { SearchInput } from "../../components/ui/search-input";
import { Skeleton } from "../../components/ui/skeleton";
import { useFieldRefs } from "../../hooks/useFieldRefs";
import { loadNotes, saveNote } from "../../lib/caseNotes";
import { cn } from "../../lib/cn";
import { usePersistedStringSet } from "../../lib/collapsedGroups";
import {
  sidebarCollapsedSnapshot,
  stickyLeftPx,
  subscribeSidebar,
} from "../../lib/sidebarState";
import { groupIndices } from "../../lib/grouping";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { exportPathFor, rememberExportPath } from "../../lib/exportDir";
import { pagePalette } from "../../lib/reportTheme";
import { toTestCase } from "../../lib/testCaseConvert";
import { save } from "@tauri-apps/plugin-dialog";
import CaseDetail from "./CaseDetail";
import CommentModal from "./CommentModal";
import {
  IconClear,
  IconCollapseAll,
  IconExport,
  IconOpenInBrowser,
} from "../../lib/actionIcons";



/** The View Test Cases tab, laid out like Update Test Cases: compact rows
 * (chevron/double-click expands a read-only detail), Group by Title,
 * click/ctrl/shift selection - and View in browser renders the selection
 * (or everything the filter shows when nothing is selected). */
/** One case of the list. Memo: its handlers are made once by the list and
 * reach the list's current state themselves, so a click on one case draws
 * only the rows whose selection, open state or comment changed. */
const ViewRow = memo(function ViewRow({
  c,
  selected,
  open,
  note,
  onCardClick,
  onToggleOpen,
  onComment,
  onSaveNote,
}: {
  c: TestCaseFull;
  selected: boolean;
  open: boolean;
  note: string;
  onCardClick: (c: TestCaseFull, e: MouseEvent) => void;
  onToggleOpen: (id: number) => void;
  onComment: (c: TestCaseFull) => void;
  onSaveNote: (id: number, text: string) => void;
}) {
  return (
    <li
      className={cn(
        "cv-row cursor-pointer select-none rounded-md border transition-colors",
        selected ? "border-accent bg-accent-soft" : "border-border hover:border-border-strong",
      )}
      onClick={(e) => onCardClick(c, e)}
      onDoubleClick={() => onToggleOpen(c.id)}
    >
      <div className="flex items-center gap-2 px-3 py-2 text-sm">
        <button
          aria-label={`Expand #${c.id}`}
          className="text-muted hover:text-accent"
          onClick={(e) => {
            e.stopPropagation();
            onToggleOpen(c.id);
          }}
        >
          {open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </button>
        <span className="id-mono text-faint">#{c.id}</span>
        <span className="text-text">{c.title}</span>
        {note && (
          <button
            aria-label="Has a local comment"
            title={note}
            className="flex shrink-0 items-center gap-1 rounded-full bg-accent-soft px-2 py-0.5 text-[11px] font-medium text-accent transition-colors hover:bg-accent-soft/80"
            onClick={(e) => {
              // A focused dialog for the comment - no need to
              // expand the whole case.
              e.stopPropagation();
              onComment(c);
            }}
          >
            <MessageSquare size={11} />
            <span className="label-trim">Comment</span>
          </button>
        )}
        <span className="ml-auto text-xs text-faint">
          {c.steps.length} steps · {c.automation_status}
        </span>
      </div>
      <Collapse open={open}>
        <CaseDetail c={c} note={note} onSaveNote={(text) => onSaveNote(c.id, text)} />
      </Collapse>
    </li>
  );
});

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
  // Open detail views, PLURAL: comparing two cases side by side is the
  // normal reason to expand a second one, and "Close all" only means
  // something once several can be open. An open detail also survives its
  // GROUP being collapsed - collapsing tidies the list away, and the case
  // someone is actively reading is not list, it is their work.
  const [openIds, setOpenIds] = useState<Set<number>>(new Set());
  const toggleOpen = (id: number) =>
    setOpenIds((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const [commentCase, setCommentCase] = useState<TestCaseFull | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [anchor, setAnchor] = useState<number | null>(null);
  const [notes, setNotes] = useState<Record<string, string>>(() => loadNotes(org));
  // Whether the browser report has been opened - see the re-export effect.
  const [reportOpen, setReportOpen] = useState(false);
  // Seeded once, but work item ids are org-scoped and so is the store. On
  // an org switch the previous org's notes stayed on screen - and saving
  // one then wrote it under the NEW org's key, copying notes across orgs.
  // Re-seeded during render so nothing wrong is ever shown or saved.
  const notesOrg = useRef(org);
  if (notesOrg.current !== org) {
    notesOrg.current = org;
    setNotes(loadNotes(org));
  }
  const [grouped, setGrouped] = useState(
    () => localStorage.getItem("tcm-v2-group-view") === "on",
  );
  const [collapsedGroups, toggleCollapsed, collapseGroups] = usePersistedStringSet(
    "tcm-v2-view-collapsed-groups",
  );

  const pbiId = pbi?.id ?? null;
  // Same key as Update Test Cases so tab switches reuse the cached fetch.
  const queryKey = ["pbi-tcs", org, pbiId, prefs.moduleRef, prefs.preconditionsRef];
  const cases = useQuery({
    queryKey,
    queryFn: () =>
      unwrap(commands.pbiTestCasesFull(org, pbiId!, prefs.moduleRef, prefs.preconditionsRef)),
    enabled: Boolean(org && pbiId != null),
    retry: false,
    // Cached-first: a tab flip inside a minute renders instantly from
    // cache; after that the cached list still paints while a background
    // refetch updates it. Refresh buttons and mutation invalidations
    // bypass this and always hit the network.
    staleTime: 60_000,
  });

  const list = cases.data ?? [];
  const settled = useSettled(list.length > 0);
  // Switching Group by title is one motion, not a regroup then an unfold
  // per open group (components/ui/collapse.tsx).
  const regroup = useRegroupMotion(grouped);
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
  const sidebarCollapsed = useSyncExternalStore(subscribeSidebar, sidebarCollapsedSnapshot);
  /** Groups on screen not yet folded - Collapse all folds these too. */
  const openGroupNames = ordered
    .map((g) => g.group)
    .filter((g) => g && !collapsedGroups.has(g));
  const collapsible = openIds.size + openGroupNames.length;

  const handleCardClick = (c: TestCaseFull, e: MouseEvent) => {
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

  /** The rows' handlers, made once: each reaches this render's state
   * through `rowLatest`, so a row (memo) is not drawn again for new
   * handlers and never acts on an old selection. */
  const rowLatest = useRef({ handleCardClick, toggleOpen, org });
  rowLatest.current = { handleCardClick, toggleOpen, org };
  const rowActions = useMemo(
    () => ({
      onCardClick: (c: TestCaseFull, e: MouseEvent) => rowLatest.current.handleCardClick(c, e),
      onToggleOpen: (id: number) => rowLatest.current.toggleOpen(id),
      onComment: (c: TestCaseFull) => setCommentCase(c),
      onSaveNote: (id: number, text: string) => setNotes(saveNote(rowLatest.current.org, id, text)),
    }),
    [],
  );

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

  const chosen = selected.size > 0 ? visible.filter((c) => selected.has(c.id)) : visible;
  const viewHtml = useMutation({
    mutationFn: () =>
      unwrapStr(
        commands.viewQueueHtml(
          chosen.map(toTestCase),
          pbiId != null ? `PBI #${pbiId}` : "",
          org,
          notes,
          // Read at click time so the page opens in the theme in front of
          // the user; it carries both schemes and its own switch.
          pagePalette(),
        ),
      ),
    onSuccess: () => setReportOpen(true),
    onError: (e) => toast.error(`Could not open the report: ${e.message ?? e}`),
  });

  // Same scope rule as the browser view: the selection when there is one,
  // otherwise everything the filter shows. The exported JSON is the
  // Import Test Cases format, ids included - so re-importing it updates these
  // exact cases rather than creating copies.
  const exportJson = useMutation({
    mutationFn: async () => {
      const path = await save({
        defaultPath: exportPathFor("test-cases.json"),
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      rememberExportPath(path);
      const r = await commands.exportQueueJson(path, chosen.map(toTestCase));
      if (r.status === "error") throw new Error(r.error);
      toast.success(`Exported ${chosen.length} case(s).`);
    },
    onError: (e) => toast.error(`Export failed: ${e.message}`),
  });

  // Keep an already-open report in step with what is on screen.
  //
  // The page is a file on disk, so nothing pushes to it: the app rewrites
  // the same file and bumps the revision the page polls, and the page then
  // OFFERS a refresh rather than taking one - reloading under a reviewer
  // costs them their place on a long page and any comment still inside its
  // autosave debounce.
  //
  // Only once they have opened it: re-exporting a report nobody asked for
  // would write a temp file every time the selection changed.
  useEffect(() => {
    if (!reportOpen || chosen.length === 0) return;
    // Debounced: selecting a run of cases lands as a burst, and each one
    // would otherwise rewrite the file.
    const t = window.setTimeout(() => {
      void commands
        // The refresh twin, NOT viewQueueHtml: that one ends in open_path,
        // and this effect re-runs whenever the app window regains focus
        // (the focus listener below re-reads notes and `chosen` is a fresh
        // array every render) - sharing the button's command opened a new
        // browser tab on every alt-tab back to the app.
        .refreshQueueHtml(
          chosen.map(toTestCase),
          pbiId != null ? `PBI #${pbiId}` : "",
          org,
          notes,
          pagePalette(),
        )
        // Silent: this refreshes something the user is not necessarily
        // looking at, and the report they have is still readable.
        .catch(() => {});
    }, 800);
    return () => window.clearTimeout(t);
    // Deliberately keyed on the CASES, not on `notes` - a comment typed
    // in the report must not bounce back and rewrite the page under it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chosen, reportOpen]);

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
    <section data-tour="view-list" className="space-y-2">
      <PageHeader
        title={
          q ? (
            `${visible.length} of ${list.length} Test Cases`
          ) : (
            <>
              {/* Counts up (CountTo): never in capture mode or under reduced
                  motion, and a screen reader hears only the total. */}
              <CountTo value={list.length} play /> Total Test Cases
            </>
          )
        }
        onRefresh={() => qc.invalidateQueries({ queryKey })}
        refreshing={cases.isFetching}
        actions={
          <>
            <Button
              variant="outline"
              size="sm"
              title="Save as JSON for Import Test Cases - selected cases when any are highlighted, otherwise everything shown. Ids are included, so re-importing updates these cases."
              disabled={chosen.length === 0 || exportJson.isPending}
              onClick={() => exportJson.mutate()}
            >
              <IconExport aria-hidden />
              {selected.size > 0 ? `Export ${selected.size} JSON` : "Export JSON"}
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={chosen.length === 0 || viewHtml.isPending}
              onClick={() => viewHtml.mutate()}
            >
              <IconOpenInBrowser aria-hidden />
              {selected.size > 0 ? `Open ${selected.size} in browser` : "Open in browser"}
            </Button>
          </>
        }
      />

      {list.length > 0 && (
        <ListToolbar
          search={
            <SearchInput
              aria-label="Search test cases"
              className="w-full"
              placeholder="Filter by name or id"
              value={search}
              onChange={setSearch}
            />
          }
        >
          <ToolbarSwitch
            label="Group by title"
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
        </ListToolbar>
      )}

      {selected.size > 0 && (
        <div className="flex items-center gap-2 rounded-md border border-accent/40 bg-accent-soft px-3 py-1.5 text-sm">
          <span className="font-medium text-accent">{selected.size} selected</span>
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

      <div ref={regroup.ref} className="space-y-2">
        {ordered.map(({ group, items }) => (
          <div key={group || "__all"} className="space-y-1">
            {group && (
              <div className="flex w-full items-center gap-3 pb-1 pt-2">
                {/* Left-anchored, controls at fixed x, the rule trails to the
                    right edge: rows are scanned down a shared left edge, and a
                    centered header put the chevron and checkbox at a different
                    x every row. */}
                <button
                  aria-label={`${collapsedGroups.has(group) ? "Expand" : "Collapse"} group ${group}`}
                  title={collapsedGroups.has(group) ? "Expand group" : "Collapse group"}
                  className="text-muted transition-colors hover:text-accent"
                  onClick={() => toggleCollapsed(group)}
                >
                  {collapsedGroups.has(group) ? <ChevronRight size={15} /> : <ChevronDown size={15} />}
                </button>
                {/* Selection is the checkbox's job; the TITLE toggles the
                    fold, same as the chevron - clicking the name is how
                    people expect to open a group. */}
                <Checkbox
                  ariaLabel={`Select all in ${group}`}
                  checked={items.length > 0 && selectedInGroup(items) === items.length}
                  indeterminate={selectedInGroup(items) > 0 && selectedInGroup(items) < items.length}
                  onCheckedChange={() => toggleGroup(items)}
                />
                <button
                  className="group flex items-center gap-2"
                  title={collapsedGroups.has(group) ? "Expand group" : "Collapse group"}
                  onClick={() => toggleCollapsed(group)}
                >
                  <span className="text-sm font-semibold tracking-wide text-muted transition-colors group-hover:text-accent">
                    {group} ({items.length})
                  </span>
                </button>
                {/* Solid where it meets the title, dissolved long before the
                    window edge - the rule belongs to the heading, not the
                    viewport. */}
                <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border to-transparent" />
              </div>
            )}
            {/* A collapsed group hides its LIST, not the case someone is
                reading: rows with an open detail stay rendered until the
                reader closes them - their own chevron, or the sticky Close
                all. Collapsing is tidying, and tidying must not snatch away
                the thing being studied. */}
            {(() => {
              const folded = Boolean(group) && collapsedGroups.has(group);
              const shown = folded ? items.filter((c) => openIds.has(c.id)) : items;
              // The fold animates the list away only when nothing in it is
              // held open: a row being read stays, so its list stays with it.
              return (
              <Collapse open={shown.length > 0} animateIn={settled && !regroup.regrouping}>
              <ul className="space-y-1">
                {shown.map((c) => (
                  <ViewRow
                    key={c.id}
                    c={c}
                    selected={selected.has(c.id)}
                    open={openIds.has(c.id)}
                    note={notes[String(c.id)] ?? ""}
                    {...rowActions}
                  />
                ))}
              </ul>
              </Collapse>
              );
            })()}
          </div>
        ))}
      </div>

      {/* Sticky whenever anything is open - the reader may be several
          screens deep in a long detail when they want it all gone, and a
          control that has scrolled away is no control. Bottom LEFT, and
          portalled to <body> for the same reason as Run Tests' action bar:
          this screen renders inside the screen fade, whose transform
          would make `fixed` mean the scroll region instead of the
          viewport. */}
      {collapsible > 0 &&
        createPortal(
          /* Left offset clears the sidebar at its CURRENT width - parked at
             left-6 this sat exactly on the sidebar's Close button. */
          <div
            className="fixed bottom-6 z-40 rounded-full border border-accent bg-bg shadow-2xl transition-[left] duration-200"
            style={{ left: stickyLeftPx(sidebarCollapsed) }}
          >
            {/* "Collapse", not "Close" or an eraser: nothing is deleted -
                open details AND open groups fold shut together, one press
                for a tidy screen. Accent-filled (variant=default): outline
                inside the same-colour wrapper read as a label, not a
                button. */}
            <Button
              size="sm"
              variant="ghost"
              className="rounded-full text-text hover:bg-surface-2 hover:text-text"
              onClick={() => {
                setOpenIds(new Set());
                collapseGroups(openGroupNames);
              }}
            >
              <IconCollapseAll aria-hidden />
              Collapse all ({collapsible})
            </Button>
          </div>,
          document.body,
        )}

      {commentCase && (
        <CommentModal
          c={commentCase}
          note={notes[String(commentCase.id)] ?? ""}
          onSave={(text) => setNotes(saveNote(org, commentCase.id, text))}
          onClose={() => setCommentCase(null)}
        />
      )}
    </section>
  );
}
