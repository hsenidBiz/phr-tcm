import { Minus, Square, X } from "lucide-react";
import { useCallback } from "react";
import FlaskLogo from "./FlaskLogo";

/** v1-style custom title bar: drag region, flask mark, dynamic title,
 * min/max/close. Rendered on frameless windows (main + runner). */
export default function TitleBar({
  title,
  compact = false,
}: {
  title: string;
  compact?: boolean;
}) {
  const win = useCallback(async () => {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    return getCurrentWindow();
  }, []);

  return (
    <header
      data-tauri-drag-region
      className="flex h-9 shrink-0 select-none items-center gap-2 border-b border-border bg-surface pl-3"
    >
      <span className="pointer-events-none flex items-center gap-2 text-accent">
        <FlaskLogo size={15} />
        <span className="text-xs font-semibold text-text">{title}</span>
      </span>
      <div className="ml-auto flex h-full">
        <button
          aria-label="Minimize"
          className="flex h-full w-11 items-center justify-center text-muted hover:bg-surface-2 hover:text-text"
          onClick={async () => (await win()).minimize().catch(() => {})}
        >
          <Minus size={14} />
        </button>
        {!compact && (
          <button
            aria-label="Maximize"
            className="flex h-full w-11 items-center justify-center text-muted hover:bg-surface-2 hover:text-text"
            onClick={async () => (await win()).toggleMaximize().catch(() => {})}
          >
            <Square size={12} />
          </button>
        )}
        <button
          aria-label="Close window"
          className="flex h-full w-11 items-center justify-center text-muted hover:bg-danger hover:text-white"
          onClick={async () => (await win()).close().catch(() => {})}
        >
          <X size={15} />
        </button>
      </div>
    </header>
  );
}
