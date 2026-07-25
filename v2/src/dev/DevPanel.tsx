import { useQueryClient } from "@tanstack/react-query";
import { Bug, ChevronDown, ChevronUp, GripVertical } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import type { PbiHit } from "../bindings";
import { START_TOUR_EVENT } from "../components/UiTour";
import { Button } from "../components/ui/button";
import { SHOW_CHANGELOG_EVENT } from "../lib/changelog";
import { setPbiGlow } from "../lib/pbiGlow";
import { isDemoMode, toggleDemoMode } from "./demo";

/** Remembered panel position - by default it sits bottom-left, which covers
 * the sidebar's collapse button, so it is draggable by the grip handle. */
const POS_KEY = "tcm-v2-dev-panel-pos";

function loadPos(): { x: number; y: number } | null {
  try {
    const raw = localStorage.getItem(POS_KEY);
    if (!raw) return null;
    const p = JSON.parse(raw);
    return typeof p?.x === "number" && typeof p?.y === "number" ? p : null;
  } catch {
    return null;
  }
}

/**
 * DEVELOPER PANEL - dev builds only. The mount site in App.tsx gates on
 * `import.meta.env.DEV`, a compile-time constant, so `tauri build`
 * (what release-v2.ps1 ships) dead-code-eliminates this entire module:
 * released binaries carry no trace of it. Add new debugging/testing tools
 * here freely; they can never leak into a client build.
 */
export default function DevPanel({
  org,
  project,
  pbi,
  section,
  workMode,
  onShowSignIn,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  section: string;
  workMode: boolean;
  /// Force the sign-in screen (the panel unmounts with it; the screen's
  /// dev-only "Skip sign-in" link is the way back).
  onShowSignIn: () => void;
}) {
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);

  const [pos, setPos] = useState<{ x: number; y: number } | null>(loadPos);
  const panelRef = useRef<HTMLDivElement>(null);
  const dragOffset = useRef<{ dx: number; dy: number } | null>(null);

  const onGripDown = (e: React.PointerEvent) => {
    const r = panelRef.current?.getBoundingClientRect();
    if (!r) return;
    dragOffset.current = { dx: e.clientX - r.left, dy: e.clientY - r.top };
    (e.target as Element).setPointerCapture(e.pointerId);
  };
  const onGripMove = (e: React.PointerEvent) => {
    if (!dragOffset.current || !panelRef.current) return;
    const { offsetWidth: w, offsetHeight: h } = panelRef.current;
    setPos({
      x: Math.min(Math.max(0, e.clientX - dragOffset.current.dx), window.innerWidth - w),
      y: Math.min(Math.max(0, e.clientY - dragOffset.current.dy), window.innerHeight - h),
    });
  };
  const onGripUp = () => {
    dragOffset.current = null;
    setPos((p) => {
      try {
        if (p) localStorage.setItem(POS_KEY, JSON.stringify(p));
      } catch {
        // session-only
      }
      return p;
    });
  };

  // A shrinking window (or a stale saved position from a bigger one) must
  // never strand the panel off-screen - re-clamp on mount and every resize.
  useEffect(() => {
    const clamp = () => {
      setPos((p) => {
        if (!p) return p;
        // ceil of the fractional rect - offsetWidth/Height round DOWN and
        // leave a sub-pixel sliver hanging past the viewport edge.
        const r = panelRef.current?.getBoundingClientRect();
        const w = Math.ceil(r?.width ?? 288);
        const h = Math.ceil(r?.height ?? 36);
        const x = Math.min(Math.max(0, p.x), Math.max(0, window.innerWidth - w));
        const y = Math.min(Math.max(0, p.y), Math.max(0, window.innerHeight - h));
        return x === p.x && y === p.y ? p : { x, y };
      });
    };
    clamp();
    window.addEventListener("resize", clamp);
    return () => window.removeEventListener("resize", clamp);
  }, []);

  const appKeys = () =>
    Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!).filter((k) =>
      k.startsWith("tcm-v2-"),
    );

  const clearKeys = (predicate: (k: string) => boolean, what: string) => {
    const doomed = appKeys().filter(predicate);
    doomed.forEach((k) => localStorage.removeItem(k));
    toast.info(`[dev] cleared ${doomed.length} ${what} key(s) - reload to take effect`);
  };

  return (
    <div
      ref={panelRef}
      className="fixed z-50 w-72 rounded-lg border border-warning/60 bg-surface text-xs shadow-2xl"
      style={pos ? { left: pos.x, top: pos.y } : { left: 16, bottom: 16 }}
    >
      {/* The whole strip drags; only the chevron toggles open/closed. */}
      <div
        aria-label="Drag the dev panel"
        title="Drag to move"
        className="flex cursor-move select-none items-center gap-2 py-2 pl-2 pr-1 font-semibold text-warning"
        onPointerDown={onGripDown}
        onPointerMove={onGripMove}
        onPointerUp={onGripUp}
      >
        <GripVertical size={13} className="text-warning/70" />
        <Bug size={13} />
        DEV BUILD
        <button
          aria-label={open ? "Collapse dev panel" : "Expand dev panel"}
          className="ml-auto cursor-pointer rounded px-2 py-0.5 hover:bg-surface-2"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={() => setOpen((o) => !o)}
        >
          {open ? <ChevronDown size={13} /> : <ChevronUp size={13} />}
        </button>
      </div>

      {open && (
        <div className="space-y-3 border-t border-border p-3">
          <div className="space-y-1">
            <p className="font-semibold text-text">Demo data</p>
            <p className="text-muted">
              {isDemoMode()
                ? "ON — every screen serves the fake DemoOrg; writes land in memory only."
                : "OFF — the app talks to real Azure DevOps."}
            </p>
            <Button size="sm" variant={isDemoMode() ? "danger" : "outline"} onClick={toggleDemoMode}>
              {isDemoMode() ? "Disable demo data (reloads)" : "Enable demo data (reloads)"}
            </Button>
          </div>

          <div className="space-y-0.5 text-muted">
            <p className="font-semibold text-text">Context</p>
            <p>org: {org || "—"} · project: {project || "—"}</p>
            <p>
              pbi: {pbi ? `#${pbi.id}` : "—"} · view: {workMode ? "work" : section}
            </p>
            <p>storage: {appKeys().length} tcm-v2 keys</p>
          </div>

          <div className="space-y-1">
            <p className="font-semibold text-text">Server state</p>
            <div className="flex flex-wrap gap-1.5">
              <Button size="sm" variant="outline" onClick={() => qc.invalidateQueries()}>
                Refetch all
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  qc.clear();
                  toast.info("[dev] query cache dropped");
                }}
              >
                Drop cache
              </Button>
            </div>
          </div>

          <div className="space-y-1">
            <p className="font-semibold text-text">Local storage</p>
            <div className="flex flex-wrap gap-1.5">
              <Button
                size="sm"
                variant="outline"
                onClick={() => clearKeys((k) => k.startsWith("tcm-v2-suite:"), "suite cache")}
              >
                Suite caches
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => clearKeys((k) => k.startsWith("tcm-v2-case-notes:"), "notes")}
              >
                Notes
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => clearKeys((k) => k.startsWith("tcm-v2-draft:"), "draft")}
              >
                Drafts
              </Button>
              <Button size="sm" variant="danger" onClick={() => clearKeys(() => true, "app")}>
                Everything
              </Button>
            </div>
          </div>

          <div className="space-y-1">
            <p className="font-semibold text-text">UI triggers</p>
            <div className="flex flex-wrap gap-1.5">
              <Button
                size="sm"
                variant="outline"
                onClick={() => window.dispatchEvent(new Event(START_TOUR_EVENT))}
              >
                Tour
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  setPbiGlow(true);
                  setTimeout(() => setPbiGlow(false), 3000);
                }}
              >
                PBI glow 3s
              </Button>
              <Button size="sm" variant="outline" onClick={onShowSignIn}>
                Sign-in screen
              </Button>
              <Button
                size="sm"
                variant="outline"
                onClick={() => window.dispatchEvent(new Event(SHOW_CHANGELOG_EVENT))}
              >
                Changelog
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
