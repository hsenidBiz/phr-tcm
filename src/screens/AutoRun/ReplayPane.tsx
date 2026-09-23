// Driving a whole selection unattended: the app opens its own browser per
// case, works through the script alone, and proposes a verdict. Nobody is
// watching by default - the person can leave and come back once it says
// "case N of M" is done.
//
// Nothing here is a verdict. `proposed` is the machine's best guess at what
// a person would have picked; it becomes a real verdict only once someone
// reviews it (Task 7), and only a reviewed run can ever reach Azure DevOps.

import { useEffect, useRef, useState } from "react";
import { commands, events } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { IconCancel, IconStop, IconUnattended } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";

// Same two browsers, same values, as the supervised pane's picker - and the
// same storage key, so a person's choice there is their choice here too.
const BROWSERS = [
  { value: "edge", label: "Microsoft Edge" },
  { value: "chrome", label: "Google Chrome" },
];

/** The one line a row shows for the phase an unattended step is in - a pure
 * function so a test can drive it directly instead of through an event. */
export function statusOf(p: {
  phase: string;
  step_number: number;
  steps: number;
  proposed: string;
}): string {
  if (p.phase === "opening") return "Opening the browser";
  if (p.phase === "signing_in") return "Signing in";
  if (p.phase === "step") return `Step ${p.step_number} of ${p.steps}`;
  if (p.phase === "done") return p.proposed ? `Proposed: ${p.proposed}` : "Nothing proposed";
  return "Waiting";
}

export default function ReplayPane({
  org,
  project,
  pbiId,
  cases,
  onClose,
  onFinished,
}: {
  org: string;
  project: string;
  pbiId: number;
  /** The selection, run in this order - same contract as the supervised
   * pane's `cases` prop. */
  cases: { id: number; title: string }[];
  onClose: () => void;
  /** Called with the finished run's id once `auto_run_replay` resolves.
   * Task 7's review screen opens from it. */
  onFinished: (runId: string) => void;
}) {
  const [phase, setPhase] = useState<"setup" | "running" | "failed">("setup");
  const [error, setError] = useState("");

  /** Same key the supervised pane uses - a stored value the picker cannot
   * show falls back to Edge, same reasoning as there. */
  const [browserName, setBrowserName] = useState(() => {
    const saved = localStorage.getItem("tcm-v2-autorun-browser");
    return BROWSERS.some((b) => b.value === saved) ? (saved as string) : "edge";
  });
  /** Off by default: an unattended run's whole point is that nobody has to
   * sit in front of it. */
  const [watch, setWatch] = useState(() => localStorage.getItem("tcm-v2-autorun-watch") === "1");

  /** Status text per case id, filled in as `ReplayProgress` events arrive. */
  const [rows, setRows] = useState<Record<number, string>>({});
  /** The selection's own "case N of M" line - null until the first event. */
  const [position, setPosition] = useState<{ index: number; total: number } | null>(null);
  const [stopping, setStopping] = useState(false);

  /** The run this pane's own Start kicked off. Only set from the FIRST
   * progress event seen after Start - a stray event from a run this pane
   * did not start (a previous one that outlived its pane, say) must not
   * overwrite what is on screen for the run actually in progress. */
  const runId = useRef<string | null>(null);

  useEffect(() => {
    const un = events.replayProgress.listen((e) => {
      const p = e.payload;
      if (runId.current === null) runId.current = p.run_id;
      if (p.run_id !== runId.current) return;
      setRows((prev) => ({ ...prev, [p.case_id]: statusOf(p) }));
      setPosition({ index: p.index, total: p.total });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const start = async () => {
    setPhase("running");
    setError("");
    setRows({});
    setPosition(null);
    setStopping(false);
    runId.current = null;
    try {
      const r = await commands.autoRunReplay(
        org,
        project,
        pbiId,
        cases.map((c) => ({ case_id: c.id, title: c.title })),
        browserName,
        watch,
      );
      if (r.status === "error") {
        setError(r.error);
        setPhase("failed");
        return;
      }
      onFinished(r.data.id);
    } catch (e) {
      // Same rethrow hazard as the supervised pane's own IPC calls - the
      // generated wrapper rethrows an Error rather than resolving to
      // {status: "error"} when the call itself rejects.
      setError(e instanceof Error ? e.message : String(e));
      setPhase("failed");
    }
  };

  const stop = async () => {
    // Said and disabled the instant the person presses it - the run itself
    // only stops after its current step, and there is nothing else useful
    // to tell them until it does.
    setStopping(true);
    await commands.autoRunReplayCancel().catch(() => {});
  };

  /** The dialog closes on Escape/backdrop everywhere except while a run is
   * actually going - closing the window would not stop it, only Stop does,
   * so letting it look closeable there would be a lie. */
  const closeIfIdle = () => {
    if (phase === "running") return;
    onClose();
  };

  return (
    <Modal onClose={closeIfIdle} className="w-full max-w-2xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">Unattended run</h2>

      {phase !== "running" ? (
        <div className="space-y-3">
          {error && <p className="text-xs text-danger">{error}</p>}
          <label className="flex items-center gap-2 text-xs text-muted">
            Browser
            <Select
              aria-label="Browser to run in"
              className="w-40"
              value={browserName}
              onChange={(e) => {
                setBrowserName(e.target.value);
                try {
                  localStorage.setItem("tcm-v2-autorun-browser", e.target.value);
                } catch {
                  // storage unavailable - the choice lasts this session
                }
              }}
            >
              {BROWSERS.map((b) => (
                <option key={b.value} value={b.value}>
                  {b.label}
                </option>
              ))}
            </Select>
          </label>
          {/* XiodUI's checkbox takes its name from the wrapping label - no
              separate ariaLabel, or it would only echo back as a duplicate. */}
          <label className="flex cursor-pointer items-center gap-2 text-xs text-muted">
            <Checkbox
              checked={watch}
              onCheckedChange={(on) => {
                setWatch(on);
                try {
                  localStorage.setItem("tcm-v2-autorun-watch", on ? "1" : "0");
                } catch {
                  // storage unavailable - the choice lasts this session
                }
              }}
            />
            Watch the browser
          </label>
          <p className="text-xs text-faint">
            Off: the browser runs in the background and you can keep working. On: a window opens
            for every case.
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={onClose}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" onClick={start}>
              <IconUnattended aria-hidden />
              Start
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-3">
          {position && (
            <p className="text-xs text-muted">
              case {position.index + 1} of {position.total}
            </p>
          )}
          <ul className="max-h-72 space-y-1 overflow-y-auto">
            {cases.map((c) => (
              <li
                key={c.id}
                className="flex items-center gap-2 rounded-md border border-border px-2 py-1.5 text-xs"
              >
                <span className="id-mono text-faint">#{c.id}</span>
                <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
                <span className={cn("text-muted", rows[c.id]?.startsWith("Proposed") && "text-text")}>
                  {rows[c.id] ?? "Waiting"}
                </span>
              </li>
            ))}
          </ul>
          <div className="flex justify-end">
            <Button size="sm" variant="outline" disabled={stopping} onClick={stop}>
              <IconStop aria-hidden />
              {stopping ? "Stopping after this step" : "Stop"}
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}
