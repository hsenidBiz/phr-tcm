import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { GitPullRequest, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { commands, type BoardData, type BoardItem, type PbiHit, type PrLink } from "../bindings";
import PbiPicker from "../components/PbiPicker";
import WorkItemDrawer from "../components/WorkItemDrawer";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Checkbox } from "../components/ui/checkbox";
import Combobox from "../components/ui/combobox";
import { Input } from "../components/ui/input";
import MultiSelect from "../components/ui/multiselect";
import { Select } from "../components/ui/select";
import { Skeleton } from "../components/ui/skeleton";
import { cn } from "../lib/cn";
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
                {l.status === "active" ? "●" : "✓"}
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
  // Hiding Done frees a third of the board for the detail drawer.
  const [hideDone, setHideDone] = useState(
    () => localStorage.getItem("tcm-v2-hide-done") === "on",
  );
  // Server-side @CurrentIteration filter (default-team context), persisted.
  const [thisSprint, setThisSprint] = useState(
    () => localStorage.getItem("tcm-v2-this-sprint") === "on",
  );
  const [openItem, setOpenItem] = useState<number | null>(null);
  const [quickTitle, setQuickTitle] = useState("");
  const [quickType, setQuickType] = useState("Task");
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
    onError: (e, { item }, ctx) => {
      if (ctx?.prev) qc.setQueryData(boardKey, ctx.prev);
      // Re-sync with the server: the snapshot may itself be stale by now.
      qc.invalidateQueries({ queryKey: boardKey });
      toast.error(`Could not move #${item.id}: ${e.message}`);
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

  const quickCreate = useMutation({
    mutationFn: () =>
      unwrap(commands.quickCreateItem(org, project, quickType, quickTitle.trim(), true)),
    onSuccess: (id) => {
      toast.success(`Created ${quickType} #${id}`);
      setQuickTitle("");
      qc.invalidateQueries({ queryKey: boardKey });
    },
    onError: (e) => toast.error(`Create failed: ${e.message}`),
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
          <label className="flex items-center gap-1.5 text-xs text-muted">
            <Checkbox
              checked={hideDone}
              onCheckedChange={(v) => {
                setHideDone(v);
                try {
                  localStorage.setItem("tcm-v2-hide-done", v ? "on" : "off");
                } catch {
                  // session-only
                }
              }}
            />
            Hide Done
          </label>
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

          <div className="ml-auto flex items-center gap-2">
            <Select
              aria-label="New item type"
              className="py-1.5"
              value={quickType}
              onChange={(e) => setQuickType(e.target.value)}
            >
              <option>Task</option>
              <option>Bug</option>
            </Select>
            <Input
              aria-label="New item title"
              className="w-56 py-1.5"
              placeholder="Quick create title"
              value={quickTitle}
              onChange={(e) => setQuickTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && quickTitle.trim()) quickCreate.mutate();
              }}
            />
            <Button
              size="sm"
              variant="outline"
              disabled={!quickTitle.trim() || quickCreate.isPending}
              onClick={() => quickCreate.mutate()}
            >
              Create
            </Button>
          </div>
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
          // The Done column stays mounted and collapses smoothly (animating
          // grid-template-columns + fade) instead of vanishing, so the other
          // two columns glide wider. Same easing as the drawer/modal.
          <div
            className="grid gap-3 transition-[grid-template-columns] duration-300 ease-out"
            style={{
              gridTemplateColumns: hideDone ? "1fr 1fr 0fr" : "1fr 1fr 1fr",
              // Hide: fade the cards out first, THEN collapse the track (and
              // the reverse when showing) so text never squishes mid-shrink.
              transitionDelay: hideDone ? "140ms" : "0ms",
            }}
          >
            {COLUMNS.map((col) => {
              const collapsed = hideDone && col === "Done";
              const items = visible.filter((i) => i.column === col);
              return (
                <div
                  key={col}
                  data-testid={`col-${col}`}
                  aria-hidden={collapsed}
                  className={cn(
                    "min-w-0 rounded-md border border-border bg-bg transition-[opacity,padding] duration-150 ease-out",
                    collapsed
                      ? // max-h-0 matters: a 0fr-wide column still sets the grid
                        // row's height, and its cards wrapping at ~0px width made
                        // the board scroll far past the visible items.
                        "pointer-events-none max-h-0 overflow-hidden border-transparent p-0 opacity-0"
                      : "space-y-2 p-2",
                  )}
                  style={{ transitionDelay: collapsed ? "0ms" : "280ms" }}
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={() => {
                    if (!collapsed && dragging && dragging.column !== col) {
                      move.mutate({ item: dragging, column: col });
                    }
                    setDragging(null);
                  }}
                >
                  <h3 className="whitespace-nowrap px-1 text-xs font-semibold uppercase tracking-wide text-muted">
                    {col} <span className="text-faint">{items.length}</span>
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
          onClose={() => setOpenItem(null)}
          onSaved={() => qc.invalidateQueries({ queryKey: boardKey })}
        />
      )}
    </div>
  );
}
