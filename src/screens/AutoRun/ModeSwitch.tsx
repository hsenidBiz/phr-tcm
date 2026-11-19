// The Basic / Advanced switch for Auto Test. It sits in the screen's title
// row (app/CaseScreens.tsx), at the right, so the screen starts straight
// under the title instead of under a row of its own.
import { useAppSimpleMode } from "../../lib/appSimpleMode";
import { unattendedRunGoing, useBackgroundRun } from "../../lib/backgroundRun";
import { cn } from "../../lib/cn";
import { toast } from "../../lib/toast";
import { useAutoRunMode } from "./useAutoRunMode";

export default function AutoRunModeSwitch() {
  const { mode, setMode, loading: modeLoading } = useAutoRunMode();
  const appSimple = useAppSimpleMode().on;
  const runGoing = unattendedRunGoing(useBackgroundRun());
  return (
    <div role="group" aria-label="Auto Test mode" className="flex rounded-md border border-border p-0.5">
      {/* App Basic mode holds Auto Test in Basic: only that option is offered. */}
      {(appSimple ? (["simple"] as const) : (["simple", "advanced"] as const)).map((m) => {
        const pressed = !modeLoading && mode === m;
        return (
          <button
            key={m}
            type="button"
            aria-pressed={pressed}
            disabled={runGoing}
            title={runGoing ? "Wait for the run that is going to finish before changing mode" : undefined}
            className={cn(
              "rounded px-2 py-1 text-xs transition-colors disabled:pointer-events-auto disabled:opacity-50",
              pressed ? "bg-accent-soft text-accent" : "text-muted hover:text-text",
            )}
            onClick={() => {
              if (modeLoading || mode === m) return;
              void setMode(m).catch((e: Error) => toast.error(`Could not change the mode: ${e.message}`));
            }}
          >
            <span className="label-trim">{m === "simple" ? "Basic" : "Advanced"}</span>
          </button>
        );
      })}
    </div>
  );
}
