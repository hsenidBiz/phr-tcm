import type { AlertList } from "../lib/assignedAlerts";

/**
 * The body of an "N work items assigned to you" / "N new mentions" toast:
 * one block per item with a hairline between them, instead of one run of
 * text. Spans, not divs, because the toast puts it inside a paragraph.
 * Each title is clamped to two lines so one long title cannot fill the
 * toast.
 */
export function AlertListView({ list }: { list: AlertList }) {
  return (
    <span className="mt-1 flex w-full min-w-0 flex-col">
      {list.items.map((it, n) => (
        <span
          key={`${it.id ?? "x"}-${n}`}
          data-testid="alert-item"
          className={`flex min-w-0 flex-col gap-0.5 pt-1.5 first:pt-0 ${n === list.items.length - 1 && list.more === 0 ? "" : "pb-1.5"} ${n > 0 ? "border-t border-border" : ""}`}
        >
          {(it.id !== undefined || it.type) && (
            <span className="flex min-w-0 items-baseline gap-1.5 text-xs text-muted-foreground">
              {it.id !== undefined && <span className="font-mono">#{it.id}</span>}
              {it.type && <span>{it.type}</span>}
            </span>
          )}
          <span className="line-clamp-2 break-words">{it.text}</span>
        </span>
      ))}
      {list.more > 0 && (
        <span className="border-t border-border pt-1.5 text-xs text-muted-foreground">and {list.more} more</span>
      )}
    </span>
  );
}
