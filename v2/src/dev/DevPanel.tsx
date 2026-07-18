import { useQueryClient } from "@tanstack/react-query";
import { Bug, ChevronDown, ChevronUp } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import type { PbiHit } from "../bindings";
import { START_TOUR_EVENT } from "../components/UiTour";
import { Button } from "../components/ui/button";
import { setPbiGlow } from "../lib/pbiGlow";
import { isDemoMode, toggleDemoMode } from "./demo";

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
    <div className="fixed bottom-4 left-4 z-50 w-72 rounded-lg border border-warning/60 bg-surface text-xs shadow-2xl">
      <button
        className="flex w-full items-center gap-2 px-3 py-2 font-semibold text-warning"
        onClick={() => setOpen((o) => !o)}
      >
        <Bug size={13} />
        DEV BUILD
        <span className="ml-auto">{open ? <ChevronDown size={13} /> : <ChevronUp size={13} />}</span>
      </button>

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
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
