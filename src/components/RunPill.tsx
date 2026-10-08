import { openRunWindow, useBackgroundRun, type BackgroundRun } from "../lib/backgroundRun";
import { cn } from "../lib/cn";
import { DISCOVERY_BUSY, useDiscoveryActiveNow } from "../lib/discoveryActive";

/** What the Discovering pill says about itself. */
export const DISCOVERING_NOTE = `${DISCOVERY_BUSY}. It ends from End discovery on Auto Run's Discovery card.`;

/** What the pill says, what a click on it does, and its colour, for a run
 * in each state. Null while there is no run to show (none, or one still on
 * its setup) and no discovery holds the browser. A discovery with no run
 * to show reads Discovering: `action` is then what it means, since there
 * is nothing to click. */
export function runPillView(
  run: BackgroundRun | null,
  discovering = false,
): { text: string; action: string; tone: "accent" | "warning" | "success" | "danger" } | null {
  if (!run || run.phase === "setup") {
    return discovering ? { text: "Discovering", action: DISCOVERING_NOTE, tone: "accent" } : null;
  }
  if (run.phase === "finished") {
    return { text: "Run finished, Review", action: "Open the finished run's review in Auto Run", tone: "success" };
  }
  if (run.phase === "failed") {
    return { text: "Run stopped with an error", action: "Open the run window to see the error", tone: "danger" };
  }
  if (run.resetNeeded) {
    return { text: "Reset needed", action: "Open the run window to continue or stop the run", tone: "warning" };
  }
  const at = run.position ? run.position.index + 1 : 1;
  const of = run.position ? run.position.total : run.cases.length;
  return { text: `Auto Run ${at} of ${of}`, action: "Open the run window", tone: "accent" };
}

const TONES = {
  accent: "bg-accent/15 text-accent-fill hover:bg-accent/25",
  warning: "bg-warning/15 text-warning hover:bg-warning/25",
  success: "bg-success/15 text-success hover:bg-success/25",
  danger: "bg-danger/15 text-danger hover:bg-danger/25",
} as const;

/** The unattended run in the window's title bar, while one exists: how far
 * it has got, that it is paused at a reset point, or that it has ended.
 * A click opens its window (as an app-level dialog, wherever the person
 * is) or, once it has finished, its review. Its accessible name says what
 * the click does. With no run to show, a discovery holding the Auto Run
 * browser reads Discovering, as text: nothing here ends it. */
export default function RunPill() {
  const { run } = useBackgroundRun();
  const discovering = useDiscoveryActiveNow();
  const view = runPillView(run, discovering);
  if (!view) return null;
  if (!run || run.phase === "setup") {
    // Discovering: text with nothing to click. What ends it is the End
    // discovery button on Auto Run, which the title and the name say.
    return (
      <span
        role="status"
        aria-label={`${view.text}. ${view.action}`}
        title={view.action}
        className="rounded-full bg-accent/15 px-1.5 py-0.5 text-[10px] font-medium leading-none text-accent-fill"
      >
        <span className="label-trim">{view.text}</span>
      </span>
    );
  }
  return (
    <button
      type="button"
      aria-label={`${view.text}. ${view.action}`}
      title={view.action}
      onClick={openRunWindow}
      className={cn(
        "rounded-full px-1.5 py-0.5 text-[10px] font-medium leading-none transition-colors focus-visible:outline-2 focus-visible:outline-accent",
        TONES[view.tone],
      )}
    >
      <span className="label-trim">{view.text}</span>
    </button>
  );
}
