import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type BoardData, type BoardItem } from "../bindings";
import { Badge } from "../components/ui/badge";
import { Skeleton } from "../components/ui/skeleton";
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

function Card({ item, onDragStart }: { item: BoardItem; onDragStart: () => void }) {
  return (
    <div
      draggable
      onDragStart={onDragStart}
      className="cursor-grab space-y-1 rounded-md border border-border bg-surface p-2 text-sm hover:border-border-strong"
    >
      <div className="flex items-center gap-2">
        <Badge color={typeColor[item.work_item_type] ?? "#9ca3af"}>
          {item.work_item_type}
        </Badge>
        <span className="text-xs text-faint">#{item.id}</span>
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

  const board = useQuery({
    queryKey: ["board", org, project],
    queryFn: () => unwrap(commands.fetchBoard(org, project)),
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
      await qc.cancelQueries({ queryKey: ["board", org, project] });
      const prev = qc.getQueryData<BoardData>(["board", org, project]);
      if (prev) {
        qc.setQueryData<BoardData>(["board", org, project], {
          ...prev,
          items: prev.items.map((i) => (i.id === item.id ? { ...i, column } : i)),
        });
      }
      return { prev };
    },
    onError: (e, { item }, ctx) => {
      if (ctx?.prev) qc.setQueryData(["board", org, project], ctx.prev);
      toast.error(`Could not move #${item.id}: ${e.message}`);
    },
    onSuccess: ({ item, state }) => {
      const data = qc.getQueryData<BoardData>(["board", org, project]);
      if (data) {
        const color =
          data.states_by_type[item.work_item_type]?.find((s) => s.name === state)?.color ?? "";
        qc.setQueryData<BoardData>(["board", org, project], {
          ...data,
          items: data.items.map((i) =>
            i.id === item.id ? { ...i, state, state_color: color } : i,
          ),
        });
      }
      toast.success(`#${item.id} moved to ${state}`);
    },
  });

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above first.
      </p>
    );
  }

  return (
    <div className="space-y-3">
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

      {board.data && (
        <div className="grid grid-cols-3 gap-3">
          {COLUMNS.map((col) => {
            const items = board.data.items.filter((i) => i.column === col);
            return (
              <div
                key={col}
                data-testid={`col-${col}`}
                className="space-y-2 rounded-md border border-border bg-bg p-2"
                onDragOver={(e) => e.preventDefault()}
                onDrop={() => {
                  if (dragging && dragging.column !== col) {
                    move.mutate({ item: dragging, column: col });
                  }
                  setDragging(null);
                }}
              >
                <h3 className="px-1 text-xs font-semibold uppercase tracking-wide text-muted">
                  {col} <span className="text-faint">{items.length}</span>
                </h3>
                {items.map((item) => (
                  <Card key={item.id} item={item} onDragStart={() => setDragging(item)} />
                ))}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
