import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, GitPullRequest, RefreshCw } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { commands, type BoardData, type BoardItem, type PbiHit, type PrLink } from "../bindings";
import PbiPicker from "../components/PbiPicker";
import WorkItemDrawer from "../components/WorkItemDrawer";
import { Badge } from "../components/ui/badge";
import { Checkbox } from "../components/ui/checkbox";
import Combobox from "../components/ui/combobox";
import { Input } from "../components/ui/input";
import MultiSelect from "../components/ui/multiselect";
import { Skeleton } from "../components/ui/skeleton";
import { cn } from "../lib/cn";
import { requiredFieldsFromError } from "../lib/adoFieldErrors";
import { unwrap } from "../lib/ipc";

const COLUMNS = ["To Do", "In Progress", "Done"] as const;

const typeColor: Record<string, string> = {
  Bug: "#e15b64",
  Task: "#d99e2b",
  "Product Backlog Item": "#2aa5e0",
  "User Story": "#2aa5e0",
  Feature: "#9a74d8",
  Epic: "#e0873c",
};

/** Cards untouched for this long get the amber "stale" edge. */
const STALE_DAYS = 7;

function staleDays(changed: string): number {
  const t = Date.parse(changed);
  if (Number.isNaN(t)) return 0;
  return Math.floor((Date.now() - t) / 86_400_000);
}

function Card({
  item,
  prLinks = [],
  onDragStart,
  onOpen,
}: {
  item: BoardItem;
  prLinks?: PrLink[];
  onDragStart: () => void;
  onOpen: () => void;
}) {
  const stale = staleDays(item.changed_date);
  const isStale = stale >= STALE_DAYS && item.column !== "Done";
  return (
    <div
      draggable
      onDragStart={onDragStart}
      onClick={onOpen}
      title={isStale ? `No changes in ${stale} days` : undefined}
      className={cn(
        "cursor-pointer space-y-1 rounded-md border border-border bg-surface p-2 text-sm hover:border-accent",
        isStale && "border-l-2 border-l-warning",
      )}
    >
      <div className="flex items-center gap-2">
        <Badge color={typeColor[item.work_item_type] ?? "#9ca3af"}>
          {item.work_item_type}
        </Badge>
        <span className="id-mono text-xs text-faint">#{item.id}</span>
        {item.assigned_to && (
          <span className="ml-auto truncate text-[10px] text-faint">{item.assigned_to}</span>
        )}
      </div>
      <div className="text-text">{item.title}</div>
      <div className="flex items-center gap-2 text-xs text-muted">
        <span
          className="inline-block h-2 w-2 rounded-full"
          style={{ backgroundColor: item.state_color ? `#${item.state_color}` : "#9ca3af" }}
        />
        {item.state}
        {item.tags && <span className="text-faint">{item.tags}</span>}
        {/* PR chips: the item's linked pull requests, labelled by REPO so
            "database ●" and "web ✓" read apart when one item carries PRs
            in several repos; active outranks completed. Click opens the PR
            without opening the card. */}
        {prLinks.length > 0 && (
          <span className="ml-auto flex flex-wrap items-center justify-end gap-1">
            {prLinks.map((l) => (
              <button
                key={l.pr_id}
                className={cn(
                  "flex max-w-32 items-center gap-1 rounded-full px-1.5 py-0.5 text-[10px] font-medium",
                  l.status === "active"
                    ? "bg-accent-soft text-accent"
                    : "bg-surface-2 text-muted",
                )}
                title={`${l.status === "active" ? "Active" : "Completed"} PR !${l.pr_id} in ${l.repo}: ${l.title}`}
                onClick={(e) => {
                  e.stopPropagation();
                  openUrl(l.web_url).catch(() => toast.error("Could not open the browser."));
                }}
              >
                <GitPullRequest size={10} className="shrink-0" />
                <span className="truncate">{l.repo}</span>
                {/* The status mark is geometry, not a glyph. As text, "●"
                    carries its ink about 1.5px below the repo name's at
                    this size and "✓" about half that - they sit low next
                    to the label however the line is aligned, because the
                    offset is inside the glyph, not the box. A shaped span
                    and an SVG both centre exactly on the flex line. */}
                {l.status === "active" ? (
                  <span aria-hidden className="h-1.5 w-1.5 shrink-0 rounded-full bg-current" />
                ) : (
                  <Check size={11} aria-hidden className="shrink-0" />
                )}
              </button>
            ))}
          </span>
        )}
      </div>
    </div>
  );
}

export default function WorkBoard({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const [dragging, setDragging] = useState<BoardItem | null>(null);
  const [scope, setScope] = useState(""); // "" = my work, else an area path
  // Third scope mode: everything parented under one PBI (plus the PBI
  // itself). pbiMode without a picked PBI shows the picker and waits.
  const [pbiMode, setPbiMode] = useState(false);
  const [pbiScope, setPbiScope] = useState<PbiHit | null>(null);
  const [filterText, setFilterText] = useState("");
  // Multi-select type filter (empty = all), persisted across sessions.
  const [typeFilter, setTypeFilter] = useState<string[]>(() => {
    try {
      const raw = JSON.parse(localStorage.getItem("tcm-v2-type-filter") ?? "[]");
      return Array.isArray(raw) ? raw.filter((t) => typeof t === "string") : [];
    } catch {
      return [];
    }
  });
  const changeTypeFilter = (v: string[]) => {
    setTypeFilter(v);
    try {
      localStorage.setItem("tcm-v2-type-filter", JSON.stringify(v));
    } catch {
      // session-only
    }
  };
  // Per-column visibility: any column can hide (collapsing to a slim rail
  // that restores it), but at least ONE must stay visible - Hide on the
  // last open column disables once two are hidden. Persisted; migrates the
  // old "Hide Done" checkbox's key.
  const [hiddenCols, setHiddenCols] = useState<Set<string>>(() => {
    try {
      const raw = localStorage.getItem("tcm-v2-hidden-cols");
      if (raw) {
        const arr = JSON.parse(raw);
        if (Array.isArray(arr)) return new Set(arr.filter((c) => typeof c === "string"));
      }
      if (localStorage.getItem("tcm-v2-hide-done") === "on") {
        localStorage.removeItem("tcm-v2-hide-done");
        localStorage.setItem("tcm-v2-hidden-cols", JSON.stringify(["Done"]));
        return new Set(["Done"]);
      }
    } catch {
      // session-only
    }
    return new Set();
  });
  // Two-phase hide/show so card text never visibly squishes mid-resize:
  // hiding fades the content out FIRST (column still full width), then the
  // empty column shrinks; showing widens the empty column first, then the
  // content fades in. `colAnim` holds the transient phase per column.
  const [colAnim, setColAnim] = useState<Record<string, "fadeOut" | "grow" | "fadeIn">>({});
  const animTimers = useRef<Record<string, number[]>>({});
  const setAnim = (col: string, phase: "fadeOut" | "grow" | "fadeIn" | null) =>
    setColAnim((prev) => {
      const next = { ...prev };
      if (phase) next[col] = phase;
      else delete next[col];
      return next;
    });
  const clearAnimTimers = (col: string) => {
    (animTimers.current[col] ?? []).forEach(clearTimeout);
    animTimers.current[col] = [];
  };
  const after = (col: string, ms: number, run: () => void) => {
    animTimers.current[col] = [...(animTimers.current[col] ?? []), window.setTimeout(run, ms)];
  };

  const FADE_MS = 150;
  const SLIDE_MS = 300;

  const toggleCol = (col: string) => {
    const hiding = !hiddenCols.has(col);
    if (hiding && hiddenCols.size >= COLUMNS.length - 1) return; // keep one visible
    clearAnimTimers(col);

    setHiddenCols((prev) => {
      const next = new Set(prev);
      if (hiding) next.add(col);
      else next.delete(col);
      try {
        localStorage.setItem("tcm-v2-hidden-cols", JSON.stringify([...next]));
      } catch {
        // session-only
      }
      return next;
    });

    if (hiding) {
      // Full width + invisible content, then the shrink runs against the rail.
      setAnim(col, "fadeOut");
      after(col, FADE_MS, () => setAnim(col, null));
    } else {
      // Widen with content invisible, then fade it in.
      setAnim(col, "grow");
      after(col, SLIDE_MS, () => setAnim(col, "fadeIn"));
      after(col, SLIDE_MS + FADE_MS, () => setAnim(col, null));
    }
  };
  // Server-side @CurrentIteration filter (default-team context), persisted.
  const [thisSprint, setThisSprint] = useState(
    () => localStorage.getItem("tcm-v2-this-sprint") === "on",
  );
  const [openItem, setOpenItem] = useState<number | null>(null);
  // Fields ADO said were blocking a move - the drawer rings them.
  const [highlightFields, setHighlightFields] = useState<string[]>([]);
  // Per-area, session-only (an assignee list rarely transfers between areas).
  const [assigneeFilter, setAssigneeFilter] = useState<string[]>([]);

  const boardKey = [
    "board",
    org,
    project,
    scope,
    pbiMode ? (pbiScope?.id ?? "none") : "",
    thisSprint,
  ];

  // Areas (the classification tree) instead of the project's team list:
  // teams accumulate forever in ADO project settings, while areas are what
  // boards actually scope by - and what users recognize.
  const areas = useQuery({
    queryKey: ["areas", org, project],
    queryFn: () => unwrap(commands.classificationPaths(org, project, "areas")),
    enabled: Boolean(org && project),
    staleTime: 60 * 60_000,
  });

  const board = useQuery({
    queryKey: boardKey,
    queryFn: () =>
      unwrap(
        commands.fetchBoard(
          org,
          project,
          pbiMode ? null : scope || null,
          pbiMode && pbiScope ? pbiScope.id : null,
          thisSprint,
        ),
      ),
    enabled: Boolean(org && project) && (!pbiMode || pbiScope !== null),
    retry: false,
  });

  // Work-item -> PR chips, resolved PR-side (one list + one small call per
  // PR). Best-effort decoration: failures just mean no chips.
  const prLinks = useQuery({
    queryKey: ["board-prs", org, project],
    queryFn: () => unwrap(commands.boardPrLinks(org, project)),
    enabled: Boolean(org && project),
    staleTime: 5 * 60_000,
    retry: false,
  });
  const prByItem = useMemo(() => {
    const m = new Map<number, PrLink[]>();
    for (const l of prLinks.data ?? []) {
      const list = m.get(l.work_item_id) ?? [];
      list.push(l);
      m.set(l.work_item_id, list);
    }
    // Active PRs outrank completed ones on the card.
    for (const list of m.values()) {
      list.sort((a, b) => (a.status === b.status ? a.pr_id - b.pr_id : a.status === "active" ? -1 : 1));
    }
    return m;
  }, [prLinks.data]);

  const move = useMutation({
    mutationFn: async ({ item, column }: { item: BoardItem; column: string }) => {
      const state = await unwrap(
        commands.moveBoardItem(org, project, item.id, item.work_item_type, column),
      );
      return { item, column, state };
    },
    onMutate: async ({ item, column }) => {
      await qc.cancelQueries({ queryKey: boardKey });
      const prev = qc.getQueryData<BoardData>(boardKey);
      if (prev) {
        qc.setQueryData<BoardData>(boardKey, {
          ...prev,
          items: prev.items.map((i) => (i.id === item.id ? { ...i, column } : i)),
        });
      }
      return { prev };
    },
    onError: (e, { item, column }, ctx) => {
      if (ctx?.prev) qc.setQueryData(boardKey, ctx.prev);
      // Re-sync with the server: the snapshot may itself be stale by now.
      qc.invalidateQueries({ queryKey: boardKey });
      // ADO rule errors name the fields blocking the transition - say so,
      // open the item, and highlight them instead of a generic failure.
      const fields = requiredFieldsFromError(e.message);
      if (fields.length > 0) {
        toast.error(
          `#${item.id} can't move to ${column}: fill ${fields.join(", ")} first - opening the item.`,
        );
        setHighlightFields(fields);
        setOpenItem(item.id);
      } else {
        toast.error(`Could not move #${item.id}: ${e.message}`);
      }
    },
    onSuccess: ({ item, state }) => {
      const data = qc.getQueryData<BoardData>(boardKey);
      if (data) {
        const color =
          data.states_by_type[item.work_item_type]?.find((s) => s.name === state)?.color ?? "";
        qc.setQueryData<BoardData>(boardKey, {
          ...data,
          items: data.items.map((i) =>
            i.id === item.id ? { ...i, state, state_color: color } : i,
          ),
        });
      }
      toast.success(`#${item.id} moved to ${state}`);
    },
  });


  const types = useMemo(
    () => [...new Set((board.data?.items ?? []).map((i) => i.work_item_type))].sort(),
    [board.data],
  );

  // Assignee filter, offered on area boards only (My work is already one
  // person; a PBI board is small). Options come from the items themselves -
  // no team-membership API, so it always matches what's actually shown.
  const UNASSIGNED = "Unassigned";
  const assignees = useMemo(() => {
    const names = new Set((board.data?.items ?? []).map((i) => i.assigned_to || UNASSIGNED));
    return [...names].sort((a, b) =>
      a === UNASSIGNED ? 1 : b === UNASSIGNED ? -1 : a.localeCompare(b),
    );
  }, [board.data]);

  const visible = (board.data?.items ?? []).filter((i) => {
    if (typeFilter.length > 0 && !typeFilter.includes(i.work_item_type)) return false;
    if (
      scope &&
      !pbiMode &&
      assigneeFilter.length > 0 &&
      !assigneeFilter.includes(i.assigned_to || UNASSIGNED)
    )
      return false;
    if (filterText) {
      const t = filterText.toLowerCase();
      if (
        !i.title.toLowerCase().includes(t) &&
        !String(i.id).includes(t) &&
        !i.tags.toLowerCase().includes(t)
      )
        return false;
    }
    return true;
  });

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above first.
      </p>
    );
  }

  return (
    <div className="flex h-full min-h-0 gap-0">
      <div className="min-w-0 flex-1 space-y-3 overflow-y-auto pr-3">
        <div className="flex flex-wrap items-center gap-2">
          {/* Searchable scope: type to find a team in long team lists. */}
          <Combobox
            ariaLabel="Board scope"
            className="w-56"
            placeholder="My work"
            value={pbiMode ? "By PBI…" : scope ? `Area: ${scope}` : "My work"}
            options={[
              "My work",
              "By PBI…",
              // Retired teams get parked under a "Scrum Archive" area node in
              // this org - hide that whole subtree from the picker.
              ...(areas.data ?? [])
                .filter((a) => !a.split("\\").some((seg) => seg.trim().toLowerCase() === "scrum archive"))
                .map((a) => `Area: ${a}`),
            ]}
            onChange={(v) => {
              if (v === "By PBI…") {
                setPbiMode(true);
                return;
              }
              setPbiMode(false);
              setScope(!v || v === "My work" ? "" : v.replace(/^Area: /, ""));
              setAssigneeFilter([]); // a different area means different people
            }}
          />
          {scope && !pbiMode && (
            <MultiSelect
              ariaLabel="Filter assignee"
              className="w-48"
              allLabel="All assignees"
              options={assignees}
              selected={assigneeFilter}
              onChange={setAssigneeFilter}
            />
          )}
          {pbiMode && (
            <div className="w-72">
              <PbiPicker org={org} project={project} pbi={pbiScope} onChange={setPbiScope} />
            </div>
          )}
          <Input
            aria-label="Filter items"
            className="w-56 py-1.5"
            placeholder="Filter by title, id, tag"
            value={filterText}
            onChange={(e) => setFilterText(e.target.value)}
          />
          <MultiSelect
            ariaLabel="Filter type"
            className="w-44"
            allLabel="All types"
            options={types}
            selected={typeFilter}
            onChange={changeTypeFilter}
          />
          <button
            aria-label="Refresh work items"
            title="Refresh work items"
            className="rounded p-1.5 text-muted transition-colors hover:text-accent"
            onClick={() => qc.invalidateQueries({ queryKey: boardKey })}
          >
            <RefreshCw size={14} className={board.isFetching ? "animate-spin" : undefined} />
          </button>
          <label className="flex items-center gap-1.5 text-xs text-muted" title="Only items in the current sprint (project default team's iteration)">
            <Checkbox
              checked={thisSprint}
              onCheckedChange={(v) => {
                setThisSprint(v);
                try {
                  localStorage.setItem("tcm-v2-this-sprint", v ? "on" : "off");
                } catch {
                  // session-only
                }
              }}
            />
            This sprint
          </label>
          {/* Creation moved to the sidebar's "New Work Item" screen - the
              board stays a read-and-move surface. */}
        </div>

        {board.isError && <p className="text-sm text-danger">{board.error.message}</p>}

        {board.isLoading && (
          <div className="grid grid-cols-3 gap-3">
            {COLUMNS.map((col) => (
              <div key={col} className="space-y-2 rounded-md border border-border p-2">
                <Skeleton className="h-4 w-20" />
                <Skeleton className="h-16" />
                <Skeleton className="h-16" />
              </div>
            ))}
          </div>
        )}

        {pbiMode && !pbiScope && (
          <p className="rounded-md border border-border p-6 text-center text-sm text-muted">
            Pick a PBI above to see everything parented under it.
          </p>
        )}

        {/* Parent links are how items land here - say so when the result is
            sparse, instead of looking silently broken. */}
        {pbiMode && pbiScope && board.data && (
          <p className="text-xs text-muted">
            {board.data.items.length} item{board.data.items.length === 1 ? "" : "s"} under{" "}
            <span className="id-mono">#{pbiScope.id}</span> (items must have this PBI as their
            parent to appear)
          </p>
        )}

        {board.data && board.data.items.length === 0 && !pbiMode && (
          <p className="rounded-md border border-border p-6 text-center text-sm text-muted">
            {scope
              ? `Nothing under the ${scope} area yet.`
              : "Nothing assigned to you in this project - quick create one above, or switch to an area board."}
          </p>
        )}

        {board.data && (
          // Hidden columns collapse to a slim rail (never to nothing) so the
          // control that restores them stays visible; the track animation
          // glides the open columns wider. Cards aren't rendered while
          // collapsed - a 0-width column's wrapped cards once made the board
          // scroll far past the visible items.
          <div
            className="grid gap-3 transition-[grid-template-columns] duration-300 ease-out"
            style={{
              // All-fr on purpose: Chromium can't interpolate fr<->px track
              // lists and leaves the transition STUCK at the start value.
              // A 0fr track still floors at its content's min size - the
              // rail's fixed w-9 - so hidden columns settle at 36px.
              // A hiding column keeps its full track while its content
              // fades ("fadeOut"); only then does the track collapse.
              gridTemplateColumns: COLUMNS.map((c) =>
                hiddenCols.has(c) && colAnim[c] !== "fadeOut" ? "0fr" : "1fr",
              ).join(" "),
            }}
          >
            {COLUMNS.map((col) => {
              const collapsed = hiddenCols.has(col) && colAnim[col] !== "fadeOut";
              const contentInvisible = colAnim[col] === "fadeOut" || colAnim[col] === "grow";
              const items = visible.filter((i) => i.column === col);
              if (collapsed) {
                return (
                  <div
                    key={col}
                    data-testid={`col-${col}`}
                    // Fixed w-9 (no min-w-0): this is the 0fr track's floor.
                    className="rail-in flex w-9 flex-col items-center gap-2 rounded-md border border-border bg-bg py-2"
                  >
                    {/* Reads top-to-bottom in the rail, same as the label
                        below it - the collapsed column is a vertical strip,
                        so the control is too. In the accent, because a
                        muted control on a 36px rail is easy to miss
                        entirely, and this is the only way back.
                        `text-center` centres the label along the axis it
                        runs down; the flex centres the box across the
                        rail.

                        The padding is spelled out PHYSICALLY on purpose.
                        Tailwind mixes the two systems - px/py are logical
                        (padding-inline/block) while pl/pr/pt/pb are
                        physical - and under vertical-rl the logical pair
                        swaps axes, so `py` silently becomes left/right.
                        Naming the sides directly means what you read is
                        where the space goes.

                        pt/pb-2 is the room above and below the word.
                        pl-0/pr-0.5 is the sliver across it, and the
                        lopsided 0/2 is measured, not eyeballed: in caps
                        the ink runs ascent 8 / descent 0 while the font
                        box is 9 / 3, so the glyphs sit 1px off the em-box
                        centre. With `leading-none` the whole control is
                        14px across - it hugs the text, which is the only
                        thing it has to fit.

                        The exact splits here and on Hide were read off the
                        RENDERED PIXELS, not derived: font metrics predict
                        the direction but not the amount, and at this size
                        half a pixel is visible. If the font or size
                        changes, measure again rather than reasoning.

                        Both stop about half a pixel short of perfect, and
                        that is a floor rather than a missing tweak: glyph
                        baselines snap to whole pixels, so fractional
                        padding below 1px moves nothing. Closing the last
                        half pixel would mean changing the box height, not
                        the padding. */}
                    <button
                      aria-label={`Open ${col}`}
                      title={`Open ${col}`}
                      className="self-center rounded border border-accent/60 pb-2 pl-[0.5px] pr-[1.5px] pt-2 text-center text-[10px] font-semibold uppercase leading-none tracking-wide text-accent transition-colors hover:bg-accent-soft"
                      style={{ writingMode: "vertical-rl" }}
                      onClick={() => toggleCol(col)}
                    >
                      Open
                    </button>
                    <span
                      className="whitespace-nowrap text-[10px] font-semibold uppercase tracking-wide text-faint"
                      style={{ writingMode: "vertical-rl" }}
                    >
                      {col} · {items.length}
                    </span>
                  </div>
                );
              }
              return (
                <div
                  key={col}
                  data-testid={`col-${col}`}
                  className="min-w-0 overflow-hidden rounded-md border border-border bg-bg p-2"
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={() => {
                    if (dragging && dragging.column !== col) {
                      move.mutate({ item: dragging, column: col });
                    }
                    setDragging(null);
                  }}
                >
                  {/* Fades as one unit: out before the column shrinks, in
                      after it finishes widening - card text never visibly
                      re-wraps while the width animates. */}
                  <div
                    className={cn(
                      "space-y-2 transition-opacity duration-150",
                      contentInvisible ? "opacity-0" : "opacity-100",
                    )}
                  >
                  <h3 className="flex items-center whitespace-nowrap px-1 text-xs font-semibold uppercase tracking-wide text-muted">
                    {col} <span className="ml-1 text-faint">{items.length}</span>
                    <button
                      aria-label={`Hide ${col}`}
                      title={
                        hiddenCols.size >= COLUMNS.length - 1
                          ? "At least one column must stay visible"
                          : `Hide ${col}`
                      }
                      disabled={hiddenCols.size >= COLUMNS.length - 1}
                      className="ml-auto rounded border border-accent/60 px-1.5 pb-px pt-[3px] text-center text-[10px] font-semibold uppercase tracking-wide text-accent transition-colors hover:bg-accent-soft disabled:cursor-not-allowed disabled:opacity-40"
                      onClick={() => toggleCol(col)}
                    >
                      Hide
                    </button>
                  </h3>
                  {items.map((item) => (
                    <Card
                      key={item.id}
                      item={item}
                      prLinks={prByItem.get(item.id)}
                      onDragStart={() => setDragging(item)}
                      onOpen={() => setOpenItem(item.id)}
                    />
                  ))}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {openItem != null && board.data && (
        <WorkItemDrawer
          org={org}
          project={project}
          itemId={openItem}
          states={(
            board.data.states_by_type[
              board.data.items.find((i) => i.id === openItem)?.work_item_type ?? ""
            ] ?? []
          ).map((s) => s.name)}
          highlightFields={highlightFields}
          onClose={() => {
            setOpenItem(null);
            setHighlightFields([]);
          }}
          onSaved={() => qc.invalidateQueries({ queryKey: boardKey })}
        />
      )}
    </div>
  );
}
