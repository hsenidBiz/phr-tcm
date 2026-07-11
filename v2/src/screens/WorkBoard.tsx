import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type BoardData, type BoardItem } from "../bindings";
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

const inputCls =
  "rounded-md border border-neutral-700 bg-neutral-900 px-3 py-2 text-sm focus:border-blue-500 focus:outline-none";

function Card({ item, onDragStart }: { item: BoardItem; onDragStart: () => void }) {
  return (
    <div
      draggable
      onDragStart={onDragStart}
      className="cursor-grab space-y-1 rounded-md border border-neutral-800 bg-neutral-900 p-2 text-sm hover:border-neutral-600"
    >
      <div className="flex items-center gap-2">
        <span
          className="rounded px-1.5 py-0.5 text-[10px] font-semibold text-black"
          style={{ backgroundColor: typeColor[item.work_item_type] ?? "#9ca3af" }}
        >
          {item.work_item_type}
        </span>
        <span className="text-xs text-neutral-500">#{item.id}</span>
      </div>
      <div>{item.title}</div>
      <div className="flex items-center gap-2 text-xs text-neutral-400">
        <span
          className="inline-block h-2 w-2 rounded-full"
          style={{ backgroundColor: item.state_color ? `#${item.state_color}` : "#9ca3af" }}
        />
        {item.state}
        {item.tags && <span className="text-neutral-600">{item.tags}</span>}
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
      // Optimistic column move; state text updates when the PATCH returns.
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
    onError: (_e, _v, ctx) => {
      if (ctx?.prev) qc.setQueryData(["board", org, project], ctx.prev);
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
    },
  });

  if (!org || !project) {
    return <p className="text-sm text-neutral-400">Pick an organization and project first.</p>;
  }

  return (
    <div className="space-y-3">
      {board.isLoading && <p className="text-sm text-neutral-400">Loading your work items...</p>}
      {board.isError && <p className="text-sm text-red-400">{board.error.message}</p>}
      {move.isError && <p className="text-sm text-red-400">{move.error.message}</p>}

      {board.data && (
        <div className="grid grid-cols-3 gap-3">
          {COLUMNS.map((col) => {
            const items = board.data.items.filter((i) => i.column === col);
            return (
              <div
                key={col}
                data-testid={`col-${col}`}
                className="space-y-2 rounded-md border border-neutral-800 bg-neutral-950 p-2"
                onDragOver={(e) => e.preventDefault()}
                onDrop={() => {
                  if (dragging && dragging.column !== col) {
                    move.mutate({ item: dragging, column: col });
                  }
                  setDragging(null);
                }}
              >
                <h3 className="px-1 text-xs font-semibold uppercase tracking-wide text-neutral-400">
                  {col} <span className="text-neutral-600">{items.length}</span>
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

export function boardInputCls() {
  return inputCls;
}
