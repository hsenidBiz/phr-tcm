import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { cn } from "../lib/cn";

export type ContextAction = {
  label: string;
  icon?: ReactNode;
  onSelect: () => void;
};

/**
 * A menu opened where the pointer was right-clicked, or beside a row the
 * keyboard opened it on (Shift+F10, the context-menu key). It is
 * MoreActionsMenu's list without the chip that opens it: the same surface,
 * the same entrance, the same keys.
 *
 * Portalled to <body> for the reason MoreActionsMenu is - a row inside a
 * scrolling or animated region would clip it or turn `fixed` into that
 * region. It closes on Escape, a click anywhere else, any scroll, a resize,
 * or an item being picked; the focus goes back to `returnTo` (the row it
 * was opened from) unless an item moved it on.
 */
export default function ContextMenu({
  x,
  y,
  label,
  actions,
  onClose,
  returnTo,
}: {
  /** Where it opens, in viewport pixels. */
  x: number;
  y: number;
  /** The menu's accessible name, e.g. "Actions for sl/admin". */
  label: string;
  actions: ContextAction[];
  onClose: () => void;
  /** What gets the focus back when the menu closes without a pick. */
  returnTo?: HTMLElement | null;
}) {
  const menu = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<CSSProperties>({ left: x, top: y, visibility: "hidden" });

  // Kept inside the window: shifted left or up by however much it overhangs.
  useLayoutEffect(() => {
    const el = menu.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const left = Math.max(8, Math.min(x, window.innerWidth - r.width - 8));
    const top = Math.max(8, Math.min(y, window.innerHeight - r.height - 8));
    setPos({ left, top });
    el.querySelector<HTMLButtonElement>("[role=menuitem]")?.focus();
  }, [x, y]);

  const closeAndReturn = () => {
    onClose();
    returnTo?.focus();
  };
  const closeRef = useRef(closeAndReturn);
  closeRef.current = closeAndReturn;

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (menu.current?.contains(e.target as Node)) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      closeRef.current();
    };
    const onAway = (e: Event) => {
      // A scroll of the window itself has no node as its target.
      if (e.target instanceof Node && menu.current?.contains(e.target)) return;
      onClose();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey, true);
    window.addEventListener("scroll", onAway, true);
    window.addEventListener("resize", onAway);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey, true);
      window.removeEventListener("scroll", onAway, true);
      window.removeEventListener("resize", onAway);
    };
  }, [onClose]);

  return createPortal(
    <div
      ref={menu}
      role="menu"
      aria-label={label}
      className="t-dropdown fixed z-50 min-w-40 rounded-md border border-border bg-surface p-1 shadow-2xl"
      style={pos}
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={(e) => {
        e.stopPropagation();
        const items = [...(menu.current?.querySelectorAll<HTMLButtonElement>("[role=menuitem]:not(:disabled)") ?? [])];
        const at = items.indexOf(document.activeElement as HTMLButtonElement);
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          e.preventDefault();
          const step = e.key === "ArrowDown" ? 1 : -1;
          items[(at + step + items.length) % items.length]?.focus();
        } else if (e.key === "Home" || e.key === "End") {
          e.preventDefault();
          items[e.key === "Home" ? 0 : items.length - 1]?.focus();
        } else if (e.key === "Tab") {
          // A menu is not a place to tab through: leave it, as a native one does.
          e.preventDefault();
          closeAndReturn();
        }
      }}
    >
      {actions.map((a) => (
        <button
          key={a.label}
          type="button"
          role="menuitem"
          className={cn(
            "flex w-full items-center gap-2 whitespace-nowrap rounded px-3 py-1.5 text-left text-xs font-medium text-text transition-colors focus-visible:outline-none [&_svg]:size-3.5 [&_svg]:shrink-0",
            "hover:bg-accent-soft hover:text-accent focus-visible:bg-accent-soft focus-visible:text-accent",
          )}
          onClick={() => {
            onClose();
            a.onSelect();
          }}
        >
          {a.icon}
          {a.label}
        </button>
      ))}
    </div>,
    document.body,
  );
}
