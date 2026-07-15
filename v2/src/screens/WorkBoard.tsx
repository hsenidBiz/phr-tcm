import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";
import { commands, type BoardData, type BoardItem } from "../bindings";
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

function Card({
  item,
  onDragStart,
  onOpen,
}: {
  item: BoardItem;
  onDragStart: () => void;
  onOpen: () => void;
}) {
  return (
    <div
      draggable
      onDragStart={onDragStart}
      onClick={onOpen}
      className="cursor-pointer space-y-1 rounded-md border border-border bg-surface p-2 text-sm hover:border-accent"
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
      </div>
    </div>
  );
}

export default function WorkBoard({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const [dragging, setDragging] = useState<BoardItem | null>(null);
  const [scope, setScope] = useState(""); // "" = my work, else team name
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
  const [openItem, setOpenItem] = useState<number | null>(null);
  const [quickTitle, setQuickTitle] = useState("");
  const [quickType, setQuickType] = useState("Task");

  const boardKey = ["board", org, project, scope];

  const teams = useQuery({
    queryKey: ["teams", org, project],
    queryFn: () => unwrap(commands.listTeams(org, project)),
    enabled: Boolean(org && project),
    staleTime: 60 * 60_000,
  });

  const board = useQuery({
    queryKey: boardKey,
    queryFn: () => unwrap(commands.fetchBoard(org, project, scope || null)),
    enabled: Boolean(org && project),
    retry: false,
  });

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

  const visible = (board.data?.items ?? []).filter((i) => {
    if (typeFilter.length > 0 && !typeFilter.includes(i.work_item_type)) return false;
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
            value={scope ? `Team: ${scope}` : "My work"}
            options={["My work", ...(teams.data ?? []).map((t) => `Team: ${t.name}`)]}
            onChange={(v) => setScope(!v || v === "My work" ? "" : v.replace(/^Team: /, ""))}
          />
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

        {board.data && board.data.items.length === 0 && (
          <p className="rounded-md border border-border p-6 text-center text-sm text-muted">
            {scope
              ? `Nothing on Team ${scope}'s board yet.`
              : "Nothing assigned to you in this project - quick create one above, or switch to a team board."}
          </p>
        )}

        {board.data && (
          // The Done column stays mounted and collapses smoothly (animating
          // grid-template-columns + fade) instead of vanishing, so the other
          // two columns glide wider. Same easing as the drawer/modal.
          <div
            className="grid gap-3 transition-[grid-template-columns] duration-300 ease-out"
            style={{ gridTemplateColumns: hideDone ? "1fr 1fr 0fr" : "1fr 1fr 1fr" }}
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
                    "min-w-0 rounded-md border border-border bg-bg transition-[opacity,padding] duration-300 ease-out",
                    collapsed
                      ? "pointer-events-none overflow-hidden border-transparent p-0 opacity-0"
                      : "space-y-2 p-2",
                  )}
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
