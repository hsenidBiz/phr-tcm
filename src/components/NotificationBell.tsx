// The bell beside the Work Manager pill: a badge with how many things
// happened since you last looked, and a panel listing them.
//
// Opening the panel marks everything read - the badge is "unseen", not
// "unhandled" - and items stay until dismissed one by one or all at once,
// the shape most apps' notification areas take. Sources raise into the
// store in ../lib/notifications; this component only reads it.

import { Bell, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { cn } from "../lib/cn";
import {
  clearAll,
  dismiss,
  markAllRead,
  unreadCount,
  useNotifications,
  type AppNotification,
} from "../lib/notifications";

const KIND_LABEL: Record<AppNotification["kind"], string> = {
  assigned: "Assigned",
  "pr-conflict": "Conflicts",
  "pr-review": "Review",
  "pr-comments": "Comments",
};

const KIND_CLASS: Record<AppNotification["kind"], string> = {
  assigned: "bg-accent/15 text-accent",
  "pr-conflict": "bg-warning/15 text-warning",
  "pr-review": "bg-success/15 text-success",
  "pr-comments": "bg-warning/15 text-warning",
};

function ago(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "";
  const mins = Math.floor((Date.now() - d.getTime()) / 60_000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 7) return `${days}d ago`;
  return d.toLocaleDateString();
}

export default function NotificationBell({ org }: { org: string }) {
  const items = useNotifications(org);
  const unread = unreadCount(items);
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  // Close on a click anywhere else, or Escape - the panel is a glance,
  // not a place to stay.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (root.current && !root.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (next) markAllRead(org);
  };

  if (!org) return null;

  return (
    <div ref={root} className="relative">
      <button
        aria-label={unread > 0 ? `Notifications, ${unread} unread` : "Notifications"}
        aria-expanded={open}
        title="Notifications"
        className={cn(
          "relative rounded-md p-1.5 text-muted transition-colors hover:bg-surface-2 hover:text-accent",
          open && "bg-surface-2 text-accent",
        )}
        onClick={toggle}
      >
        <Bell size={16} aria-hidden />
        {unread > 0 && (
          <span
            aria-hidden
            className="absolute -right-0.5 -top-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-danger px-1 text-[10px] font-semibold leading-none text-on-accent"
          >
            {unread > 9 ? "9+" : unread}
          </span>
        )}
      </button>

      {open && (
        <div
          role="dialog"
          aria-label="Notifications"
          className="absolute right-0 top-full z-50 mt-1 w-80 rounded-md border border-border bg-surface shadow-2xl"
        >
          <div className="flex items-center justify-between border-b border-border px-3 py-2">
            <span className="text-xs font-semibold uppercase tracking-wide text-muted">Notifications</span>
            {items.length > 0 && (
              <button
                className="text-[11px] text-faint underline-offset-2 hover:text-accent hover:underline"
                onClick={() => clearAll(org)}
              >
                Clear all
              </button>
            )}
          </div>
          {items.length === 0 ? (
            <p className="px-3 py-4 text-center text-sm text-muted">You&apos;re all caught up.</p>
          ) : (
            <ul className="max-h-96 overflow-y-auto">
              {items.map((n) => (
                <li
                  key={n.id}
                  className="flex gap-2 border-b border-border/60 px-3 py-2 last:border-b-0"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span
                        className={cn(
                          "rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide",
                          KIND_CLASS[n.kind],
                        )}
                      >
                        {KIND_LABEL[n.kind]}
                      </span>
                      <span className="text-[11px] text-faint">{ago(n.at)}</span>
                    </div>
                    {n.href ? (
                      <button
                        className="mt-0.5 block max-w-full truncate text-left text-sm font-medium text-text hover:text-accent hover:underline"
                        title="Open in Azure DevOps"
                        onClick={() =>
                          openUrl(n.href!).catch(() => toast.error("Could not open the browser."))
                        }
                      >
                        {n.title}
                      </button>
                    ) : (
                      <div className="mt-0.5 truncate text-sm font-medium text-text">{n.title}</div>
                    )}
                    {n.body && <div className="truncate text-xs text-muted">{n.body}</div>}
                  </div>
                  <button
                    aria-label={`Dismiss: ${n.title}`}
                    title="Dismiss"
                    className="self-start rounded p-1 text-faint transition-colors hover:text-danger"
                    onClick={() => dismiss(org, n.id)}
                  >
                    <X size={12} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
