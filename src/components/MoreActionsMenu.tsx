import { ChevronDown } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { cn } from "../lib/cn";

export type MoreAction = { label: string; onSelect: () => void };

/** How long the pointer may be between the trigger and the menu before a
 * hover-opened menu closes - the gap it crosses is a few pixels, and a
 * menu that shuts the instant the pointer leaves the chip cannot be
 * reached at all. */
const HOVER_GRACE_MS = 180;

/** The list's shrink on close - --motion-quick in index.css. It stays in
 * the page (hidden from assistive tech, unclickable) for this long. */
const EXIT_MS = 150;

const reducedMotion = () => window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

/**
 * A chip that holds secondary actions. Hovering it opens the list; clicking
 * it opens the list and keeps it open until an action is picked, the user
 * clicks elsewhere, presses Escape, or scrolls.
 *
 * The trigger is a span with role="button", like the chips beside it,
 * because it lives inside a row that is itself a <button> and a button may
 * not contain another. The list is portalled to <body>: the rows it sits in
 * use content-visibility, whose paint containment clips anything drawn
 * outside the row, and the screen's animated wrapper would make `fixed`
 * mean the scroll region. React still bubbles the list's events through the
 * row, so every handler here stops propagation - otherwise picking
 * "Report" would also fold or unfold the row underneath.
 */
export default function MoreActionsMenu({
  label,
  actions,
  disabled = false,
  text = "More",
}: {
  /** Accessible name for the trigger, e.g. "More actions for Regression". */
  label: string;
  actions: MoreAction[];
  disabled?: boolean;
  text?: string;
}) {
  const [open, setOpen] = useState<null | "hover" | "click">(null);
  // True for the few frames the list spends shrinking away after a close.
  const [closing, setClosing] = useState(false);
  const [pos, setPos] = useState<CSSProperties>({});
  const [origin, setOrigin] = useState<"top-right" | "bottom-right">("top-right");
  const trigger = useRef<HTMLSpanElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const closeTimer = useRef<number | null>(null);
  const exitTimer = useRef<number | null>(null);
  const openRef = useRef(open);
  openRef.current = open;

  const cancelClose = () => {
    if (closeTimer.current != null) window.clearTimeout(closeTimer.current);
    closeTimer.current = null;
  };
  const cancelExit = () => {
    if (exitTimer.current != null) window.clearTimeout(exitTimer.current);
    exitTimer.current = null;
    setClosing(false);
  };
  const show = (mode: "hover" | "click") => {
    cancelClose();
    cancelExit();
    setOpen((o) => (mode === "hover" ? (o ?? "hover") : "click"));
  };
  const close = () => {
    cancelClose();
    if (!openRef.current) return;
    if (reducedMotion()) {
      setOpen(null);
      return;
    }
    setClosing(true);
    exitTimer.current = window.setTimeout(() => {
      exitTimer.current = null;
      setClosing(false);
      setOpen(null);
    }, EXIT_MS);
  };
  // Only a hover-opened menu closes on its own; one the user clicked open
  // stays until they decide.
  const scheduleClose = () => {
    cancelClose();
    closeTimer.current = window.setTimeout(() => {
      if (openRef.current === "hover") close();
    }, HOVER_GRACE_MS);
  };
  useEffect(
    () => () => {
      cancelClose();
      if (exitTimer.current != null) window.clearTimeout(exitTimer.current);
    },
    [],
  );
  const shown = open != null && !closing;

  // Placed under the trigger, right edges aligned; above it when there is
  // no room below.
  useLayoutEffect(() => {
    if (!open || !trigger.current) return;
    const r = trigger.current.getBoundingClientRect();
    const below = window.innerHeight - r.bottom;
    const right = Math.max(8, window.innerWidth - r.right);
    const above = below < 140;
    setPos(above ? { right, bottom: window.innerHeight - r.top + 4 } : { right, top: r.bottom + 4 });
    // It grows out of the corner nearest the chip.
    setOrigin(above ? "bottom-right" : "top-right");
  }, [open]);

  // Clicking anywhere else, Escape, or scrolling closes it. A scroll moves
  // the trigger out from under a fixed list, so the list goes rather than
  // float beside the wrong row.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (trigger.current?.contains(t) || menu.current?.contains(t)) return;
      close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      close();
      trigger.current?.focus();
    };
    const onScroll = (e: Event) => {
      if (menu.current?.contains(e.target as Node)) return;
      close();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [open]);

  if (actions.length === 0) return null;

  return (
    <>
      <span
        ref={trigger}
        role="button"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={shown}
        className={cn(
          "inline-flex items-center gap-1 whitespace-nowrap rounded-md border border-border bg-surface-2 px-2.5 py-1 text-xs font-medium text-muted transition-colors",
          "hover:border-accent hover:bg-accent-soft hover:text-accent",
          shown && "border-accent bg-accent-soft text-accent",
          disabled && "pointer-events-none opacity-50",
        )}
        onMouseEnter={() => {
          if (disabled) return;
          show("hover");
        }}
        onMouseLeave={scheduleClose}
        onClick={(e) => {
          e.stopPropagation();
          if (disabled) return;
          // A click on a menu the pointer opened pins it; a click on a
          // pinned one closes it.
          if (open === "click" && !closing) close();
          else show("click");
        }}
        onKeyDown={(e) => {
          if (e.key !== "Enter" && e.key !== " " && e.key !== "ArrowDown") return;
          e.preventDefault();
          e.stopPropagation();
          if (disabled) return;
          show("click");
          window.setTimeout(() => menu.current?.querySelector<HTMLButtonElement>("[role=menuitem]")?.focus(), 0);
        }}
      >
        {text}
        <ChevronDown size={12} aria-hidden />
      </span>
      {open != null &&
        createPortal(
          <div
            ref={menu}
            role="menu"
            aria-label={label}
            // Shrinking away: gone for assistive tech and the pointer, still
            // painted for the length of the animation.
            aria-hidden={closing || undefined}
            data-origin={origin}
            className={cn(
              "t-dropdown fixed z-50 min-w-36 rounded-md border border-border bg-surface p-1 shadow-2xl",
              closing && "is-closing",
            )}
            style={pos}
            onMouseEnter={() => {
              if (closing) show(openRef.current ?? "hover");
              else cancelClose();
            }}
            onMouseLeave={scheduleClose}
            onClick={(e) => e.stopPropagation()}
            onMouseDown={(e) => e.stopPropagation()}
            onKeyDown={(e) => {
              e.stopPropagation();
              const items = [...(menu.current?.querySelectorAll<HTMLButtonElement>("[role=menuitem]") ?? [])];
              const at = items.indexOf(document.activeElement as HTMLButtonElement);
              if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                e.preventDefault();
                const step = e.key === "ArrowDown" ? 1 : -1;
                items[(at + step + items.length) % items.length]?.focus();
              } else if (e.key === "Escape") {
                close();
                trigger.current?.focus();
              }
            }}
          >
            {actions.map((a) => (
              <button
                key={a.label}
                type="button"
                role="menuitem"
                className="block w-full whitespace-nowrap rounded px-3 py-1.5 text-left text-xs font-medium text-text transition-colors hover:bg-accent-soft hover:text-accent focus-visible:bg-accent-soft focus-visible:text-accent focus-visible:outline-none"
                onClick={(e) => {
                  e.stopPropagation();
                  close();
                  a.onSelect();
                }}
              >
                {a.label}
              </button>
            ))}
          </div>,
          document.body,
        )}
    </>
  );
}
