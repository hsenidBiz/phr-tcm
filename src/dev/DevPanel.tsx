import { useQueryClient } from "@tanstack/react-query";
import { Bug, ChevronDown, ChevronUp, GripVertical } from "lucide-react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { toast } from "../lib/toast";
import { commands, type PbiHit } from "../bindings";
import { START_TOUR_EVENT } from "../tour/tourState";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { cn } from "../lib/cn";
import { SHOW_CHANGELOG_EVENT } from "../lib/changelog";
import { setPbiGlow } from "../lib/pbiGlow";
import { clearSuiteSeed } from "../lib/suiteSeed";
import { isDemoMode, toggleDemoMode } from "./demo";
import {
  armFault,
  disarmFault,
  faultSnapshot,
  subscribeFaults,
  toggleUpdateBlocked,
  FAULTS,
  type FaultId,
  type FaultMode,
} from "./faults";
import { latencyMs, setLatencyMs, LATENCY_STEPS } from "./latency";

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

/** One of each toast the app raises: the four kinds, a plain one, one with
 * an action (the Undo a discard offers) and one that stays until dismissed. */
const TOAST_SAMPLES: [string, () => void][] = [
  ["Success", () => toast.success("[dev] Saved 3 test cases.")],
  ["Error", () => toast.error("[dev] Could not reach Azure DevOps. Check your connection and try again.")],
  ["Info", () => toast.info("[dev] PR #21691 is one of yours. Turn on Your Pull Requests to see it.")],
  ["Warning", () => toast.warning("[dev] 2 cases have no expected result.")],
  ["Plain", () => toast("[dev] A plain toast with no kind.")],
  [
    "With action",
    () =>
      toast.success("[dev] Discarded the draft.", {
        action: { label: "Undo", onClick: () => toast.info("[dev] Undo pressed.") },
      }),
  ],
  [
    "Sticky",
    () =>
      toast.info("[dev] This one stays until dismissed.", {
        description: "A second line under the message.",
        duration: Infinity,
      }),
  ],
];

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
  const [latency, setLatency] = useState(latencyMs);
  const fault = useSyncExternalStore(subscribeFaults, faultSnapshot);
  // Which failure the once/always buttons will arm. Separate from what IS
  // armed: picking a kind must not fire anything by itself.
  const [faultId, setFaultId] = useState<FaultId>("timeout");

  // The Boards suite route probe. Typed rather than taken from the
  // selection, because the PBI it needs is one that has NO suite yet and
  // a case id that already exists on it - not necessarily what is open.
  const [probePbi, setProbePbi] = useState("");
  const [probeCase, setProbeCase] = useState("");
  // 0 asks the server to make the team's sprint plan itself, which is what
  // the watched Boards save produced; an existing plan id is the other
  // thing worth trying when 0 is refused.
  const [probePlan, setProbePlan] = useState("0");
  const [probing, setProbing] = useState(false);
  const [probeReport, setProbeReport] = useState("");

  const runProbe = async () => {
    setProbing(true);
    setProbeReport("");
    try {
      const r = await commands.devProbeBoardsSuite(
        org,
        project,
        Number(probePbi),
        Number(probeCase),
        Number(probePlan || "0"),
      );
      setProbeReport(r.status === "ok" ? r.data : r.error);
    } finally {
      setProbing(false);
    }
  };

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
    // Again on open/close: the open panel is much wider than the strip, and
    // one dragged near the right edge would otherwise open half off-screen.
  }, [open]);

  const appKeys = () =>
    Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)!).filter((k) =>
      k.startsWith("tcm-v2-"),
    );

  const clearKeys = (predicate: (k: string) => boolean, what: string) => {
    const doomed = appKeys().filter(predicate);
    doomed.forEach((k) => localStorage.removeItem(k));
    toast.info(`[dev] cleared ${doomed.length} ${what} key(s) - reload to take effect`);
  };

  const armedLabel = FAULTS.find((f) => f.id === fault.armed?.id)?.label ?? "";

  return (
    <div
      ref={panelRef}
      // Wide and two columns when open - one tall column ran off the bottom
      // of the window. The collapsed strip stays narrow.
      className={cn(
        "fixed z-50 rounded-lg border border-warning/60 bg-surface text-xs shadow-2xl",
        open ? "w-[44rem] max-w-[calc(100vw-2rem)]" : "w-72",
      )}
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
        <div className="columns-2 gap-5 border-t border-border p-3 [&>*]:mb-3 [&>*]:break-inside-avoid">
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

          <div className="space-y-1">
            <p className="font-semibold text-text">Fake latency</p>
            <p className="text-muted">
              Every command answers this much later, so loading states are
              seen instead of imagined. Applies to the next request - no reload.
            </p>
            <div className="flex flex-wrap gap-1.5">
              {LATENCY_STEPS.map((ms) => (
                <Button
                  key={ms}
                  size="sm"
                  variant={latency === ms ? "danger" : "outline"}
                  aria-pressed={latency === ms}
                  onClick={() => {
                    setLatencyMs(ms);
                    setLatency(ms);
                  }}
                >
                  {ms === 0 ? "Off" : ms < 1000 ? `${ms}ms` : `${ms / 1000}s`}
                </Button>
              ))}
            </div>
          </div>

          <div className="space-y-1">
            <p className="font-semibold text-text">Force a failure</p>
            <p className="text-muted">
              {fault.armed
                ? `ARMED - ${armedLabel} on ${fault.armed.mode === "once" ? "the next command" : "every command"}.`
                : fault.fired
                  ? "Fired - the next command failed. Arm another to repeat."
                  : "Make commands come back as an error, so failure states are seen instead of imagined. Pick a failure, then arm it below."}
            </p>
            {/* Picking a kind only chooses what the buttons below will arm,
                so it wears the accent, not the red fill - red means "on",
                and a picked-but-unarmed kind in red read as a failure that
                could not be switched off. */}
            <div className="flex flex-wrap gap-1.5">
              {FAULTS.map((f) => (
                <Button
                  key={f.id}
                  size="sm"
                  variant="outline"
                  className={cn(faultId === f.id && "border-accent text-accent")}
                  aria-pressed={faultId === f.id}
                  onClick={() => setFaultId(f.id)}
                >
                  {f.label}
                </Button>
              ))}
            </div>
            <div className="flex flex-wrap gap-1.5 pt-1">
              {(["once", "always"] as FaultMode[]).map((mode) => (
                <Button
                  key={mode}
                  size="sm"
                  variant={fault.armed?.mode === mode ? "danger" : "outline"}
                  aria-pressed={fault.armed?.mode === mode}
                  onClick={() => armFault(faultId, mode)}
                >
                  {mode === "once" ? "Fail next call" : "Fail every call"}
                </Button>
              ))}
              <Button size="sm" variant="outline" disabled={!fault.armed} onClick={disarmFault}>
                Stop
              </Button>
            </div>
            {/* Its own switch: an unreachable update feed is a `blocked`
                field on a SUCCESSFUL check, not a rejected command, so the
                injector above cannot express it. */}
            <div className="flex flex-wrap gap-1.5 pt-1">
              <Button
                size="sm"
                variant={fault.updateBlocked ? "danger" : "outline"}
                aria-pressed={fault.updateBlocked}
                onClick={toggleUpdateBlocked}
              >
                {fault.updateBlocked ? "Update check blocked" : "Block update check"}
              </Button>
            </div>
          </div>

          <div className="space-y-1">
            <p className="font-semibold text-text">Boards suite route probe</p>
            <p className="text-muted">
              Takes the internal Boards route once, for a PBI that has no
              test suite yet and one test case id that is already linked to
              it. No test case is created, but this DOES create a test plan
              and a requirement suite in Azure DevOps, and pulls every case
              already linked to the PBI into it. Press it once, on purpose.
              Settings, Logs has the request and the reply in full.
            </p>
            <div className="flex flex-wrap items-center gap-1.5">
              <Input
                className="w-20 px-2 py-1 text-xs"
                inputMode="numeric"
                placeholder="PBI id"
                aria-label="PBI id to probe"
                value={probePbi}
                onChange={(e) => setProbePbi(e.target.value.replace(/\D/g, ""))}
              />
              <Input
                className="w-20 px-2 py-1 text-xs"
                inputMode="numeric"
                placeholder="Case id"
                aria-label="Test case id to add"
                value={probeCase}
                onChange={(e) => setProbeCase(e.target.value.replace(/\D/g, ""))}
              />
              <Input
                className="w-20 px-2 py-1 text-xs"
                inputMode="numeric"
                placeholder="Plan id (0)"
                aria-label="Test plan id, 0 to let the server choose"
                value={probePlan}
                onChange={(e) => setProbePlan(e.target.value.replace(/\D/g, ""))}
              />
              <Button
                size="sm"
                variant="outline"
                disabled={probing || !org || !project || !probePbi || !probeCase}
                onClick={runProbe}
              >
                {probing ? "Probing" : "Probe"}
              </Button>
            </div>
            {probeReport && (
              <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-words rounded border border-border bg-surface-2 p-2 text-muted">
                {probeReport}
              </pre>
            )}
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
                onClick={() => {
                  // The suite seed now lives in the app cache (keyed by
                  // org/pbi, not a raw prefix any button here can scan) -
                  // clear it for the PBI on screen, the same one every
                  // other button in this row targets.
                  if (!pbi) {
                    toast.info("[dev] no PBI selected");
                    return;
                  }
                  clearSuiteSeed(org, pbi.id);
                  toast.info("[dev] cleared suite cache for this PBI - reload to take effect");
                }}
              >
                Suite cache
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

          <div className="space-y-1">
            <p className="font-semibold text-text">Toasts</p>
            <p className="text-muted">
              Every kind the app raises, so each look can be checked in each
              theme without finding the action that raises it.
            </p>
            <div className="flex flex-wrap gap-1.5">
              {TOAST_SAMPLES.map(([label, raise]) => (
                <Button key={label} size="sm" variant="outline" aria-label={`Toast: ${label}`} onClick={raise}>
                  {label}
                </Button>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
