import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, MessageSquare, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { toast } from "sonner";
import { commands, type PbiHit, type TestCaseFull } from "../../bindings";
import CountUp from "../../components/CountUp";
import PickPbiEmpty from "../../components/PickPbiEmpty";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
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
import { pagePalette } from "../../lib/reportTheme";
import { toTestCase } from "../../lib/testCaseConvert";
import CaseDetail from "./CaseDetail";
import CommentModal from "./CommentModal";
import { IconClear, IconCollapseAll, IconOpenInBrowser } from "../../lib/actionIcons";



/** The View Test Cases tab, laid out like Update Test Cases: compact rows
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
  const [collapsedGroups, toggleCollapsed] = usePersistedStringSet(
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
  const sidebarCollapsed = useSyncExternalStore(subscribeSidebar, sidebarCollapsedSnapshot);

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
    <section className="space-y-2">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="text-sm font-semibold text-muted">
          {q ? (
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
          onClick={() => qc.invalidateQueries({ queryKey })}
        >
          {/* The refetch keeps the old rows on screen while it runs, so the
              spin is the ONLY sign the click did anything. */}
          <RefreshCw size={14} className={cases.isFetching ? "animate-spin" : undefined} />
        </button>
        <div className="ml-auto">
          <Button
            variant="outline"
            size="sm"
            disabled={chosen.length === 0 || viewHtml.isPending}
            onClick={() => viewHtml.mutate()}
          >
            <IconOpenInBrowser aria-hidden />
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
                {/* Collapsed groups hide their rows, and with them the only
                    sign that anything inside is selected - so the count
                    surfaces on the heading instead. Shown for ONE selected
                    case as much as for all of them: the question it answers
                    is "did I leave something highlighted in there". */}
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
          {/* A collapsed group hides its LIST, not the case someone is
              reading: rows with an open detail stay rendered until the
              reader closes them - their own chevron, or the sticky Close
              all. Collapsing is tidying, and tidying must not snatch away
              the thing being studied. */}
          {(group && collapsedGroups.has(group)
            ? items.filter((c) => openIds.has(c.id))
            : items
          ).length === 0 ? null : (
            <ul className="space-y-1">
              {(group && collapsedGroups.has(group)
                ? items.filter((c) => openIds.has(c.id))
                : items
              ).map((c) => (
                <li
                  key={c.id}
                  className={cn(
                    "cursor-pointer select-none rounded-md border transition-colors",
                    selected.has(c.id)
                      ? "border-accent bg-accent-soft"
                      : "border-border hover:border-border-strong",
                  )}
                  onClick={(e) => handleCardClick(c, e)}
                  onDoubleClick={() => toggleOpen(c.id)}
                >
                  <div className="flex items-center gap-2 px-3 py-2 text-sm">
                    <button
                      aria-label={`Expand #${c.id}`}
                      className="text-muted hover:text-accent"
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleOpen(c.id);
                      }}
                    >
                      {openIds.has(c.id) ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                    </button>
                    <span className="id-mono text-faint">#{c.id}</span>
                    <span className="text-text">{c.title}</span>
                    {notes[String(c.id)] && (
                      <button
                        aria-label="Has a local comment"
                        title={notes[String(c.id)]}
                        className="flex shrink-0 items-center gap-1 rounded-full bg-accent-soft px-2 py-0.5 text-[11px] font-medium text-accent transition-colors hover:bg-accent-soft/80"
                        onClick={(e) => {
                          // A focused dialog for the comment - no need to
                          // expand the whole case.
                          e.stopPropagation();
                          setCommentCase(c);
                        }}
                      >
                        <MessageSquare size={11} />
                        Comment
                      </button>
                    )}
                    <span className="ml-auto text-xs text-faint">
                      {c.steps.length} steps · {c.automation_status}
                    </span>
                  </div>
                  {openIds.has(c.id) && (
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

      {/* Sticky whenever anything is open - the reader may be several
          screens deep in a long detail when they want it all gone, and a
          control that has scrolled away is no control. Bottom LEFT, and
          portalled to <body> for the same reason as Run Tests' action bar:
          this screen renders inside AnimatedContent, whose GSAP transform
          would make `fixed` mean the scroll region instead of the
          viewport. */}
      {openIds.size > 0 &&
        createPortal(
          /* Left offset clears the sidebar at its CURRENT width - parked at
             left-6 this sat exactly on the sidebar's Close button. */
          <div
            className="fixed bottom-6 z-40 rounded-full border border-border bg-surface shadow-2xl transition-[left] duration-200"
            style={{ left: stickyLeftPx(sidebarCollapsed) }}
          >
            {/* "Collapse", not "Close" or an eraser: nothing is deleted,
                the open detail views just fold shut. */}
            <Button size="sm" variant="outline" className="rounded-full" onClick={() => setOpenIds(new Set())}>
              <IconCollapseAll aria-hidden />
              Collapse all ({openIds.size})
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
