// Driving one case while a person watches.
//
// The action outcomes are EVIDENCE, never a vote. Nothing here
// pre-selects a verdict: a green run can still be a failure the person
// spotted with their eyes, and a red action can be the harness's fault
// rather than the app's. The human presses the button.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { commands, type ActionOutcome, type CaseRecord } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Textarea } from "../../components/ui/input";
import { Select } from "../../components/ui/select";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";

const VERDICTS = ["Passed", "Failed", "Blocked"] as const;

const verdictTone: Record<string, string> = {
  Passed: "bg-success/20 text-success",
  Failed: "bg-danger/20 text-danger",
  Blocked: "bg-warning/20 text-warning",
};

export default function RunPane({
  pbiId,
  cases,
  onClose,
}: {
  pbiId: number;
  /** The selection, run one after another in this order. One case is a
   * selection of one - there is no separate single-case path. */
  cases: { id: number; title: string }[];
  onClose: () => void;
}) {
  const queryClient = useQueryClient();

  // Where we are in the selection.
  const [idx, setIdx] = useState(0);
  const current = cases[idx] ?? cases[0];
  const caseId = current?.id ?? 0;
  const title = current?.title ?? "";
  const isLast = idx >= cases.length - 1;

  /** Verdicts marked so far. Kept here rather than written per case so a
   * bulk run is ONE run in the results view, the way a person thinks of
   * it - and flushed on close as well as on finish, so walking away
   * half-way through does not throw away the verdicts already given. */
  const [records, setRecords] = useState<CaseRecord[]>([]);

  /** Which browser to watch in. Remembered, because a person who prefers
   * Chrome prefers it every time. */
  const [browserName, setBrowserName] = useState(
    () => localStorage.getItem("tcm-v2-autorun-browser") ?? "edge",
  );

  const script = useQuery({
    queryKey: ["autorun-script", caseId],
    queryFn: () => unwrapStr(commands.autoRunLoadScript(caseId)),
    retry: false,
  });

  const [opened, setOpened] = useState(false);
  const [busy, setBusy] = useState(false);
  const [results, setResults] = useState<Record<number, ActionOutcome[]>>({});
  const [verdict, setVerdict] = useState("");
  const [note, setNote] = useState("");

  // The browser is a real Edge/Chrome process with a temp profile directory - it
  // has to be closed on every path that ends this session, not just the
  // ones the brief spells out (Close, Save). If the person navigates away
  // instead, this component unmounts without either firing, and the
  // process + its temp dir would otherwise leak for the rest of the app's
  // lifetime. `openedRef` (not state) means this effect's cleanup always
  // sees the latest answer without re-subscribing on every render, and the
  // `closedRef` guard stops it from ever firing twice (unmount racing an
  // in-flight explicit close).
  const openedRef = useRef(false);
  const closedRef = useRef(false);
  useEffect(() => {
    openedRef.current = opened;
  }, [opened]);
  useEffect(() => {
    return () => {
      if (openedRef.current && !closedRef.current) {
        closedRef.current = true;
        void commands.autoRunCloseBrowser().catch(() => {});
      }
    };
  }, []);

  const openBrowser = async () => {
    setBusy(true);
    try {
      const r = await commands.autoRunOpenBrowser(browserName);
      if (r.status === "error") {
        toast.error(`Could not open the browser: ${r.error}`);
        return;
      }
      setOpened(true);
    } catch (e) {
      // The generated `typedError` wrapper rethrows when the IPC call itself
      // rejects with an Error (rather than resolving to {status: "error"}) -
      // without this catch that propagates as an unhandled rejection AND,
      // because setBusy(false) below never runs, leaves this button
      // permanently disabled with no explanation.
      toast.error(`Could not open the browser: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const runStep = async (stepNumber: number) => {
    const step = script.data?.steps.find((s) => s.step_number === stepNumber);
    if (!step) return;
    setBusy(true);
    try {
      const r = await commands.autoRunStep(step);
      if (r.status === "error") {
        toast.error(r.error);
        return;
      }
      setResults((prev) => ({ ...prev, [stepNumber]: r.data }));
    } catch (e) {
      // See openBrowser above: a rethrown Error here would otherwise wedge
      // every "Run step N" button disabled for the rest of the session.
      toast.error(`Could not run step ${stepNumber}: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    const record: CaseRecord = {
      case_id: caseId,
      title,
      verdict,
      note,
      steps: (script.data?.steps ?? []).map((s) => ({
        step_number: s.step_number,
        outcomes: results[s.step_number] ?? [],
      })),
    };

    // More cases to go: bank this verdict and move on WITHOUT writing or
    // closing the browser. One selection is one run, and the browser
    // stays open so the next case does not pay for a fresh launch.
    if (!isLast) {
      setRecords((r) => [...r, record]);
      setIdx((i) => i + 1);
      setResults({});
      setVerdict("");
      setNote("");
      return;
    }

    if (!(await writeRun([...records, record]))) return;
    closedRef.current = true;
    await commands.autoRunCloseBrowser().catch(() => {});
    onClose();
  };

  /** The one place a run reaches disk. Returns false when it did not, so
   * callers can leave the pane open rather than closing over a failure. */
  const writeRun = async (all: CaseRecord[]): Promise<boolean> => {
    if (all.length === 0) return true;
    let id: string;
    try {
      id = await commands.autoRunNewId();
    } catch (e) {
      toast.error(`Could not save the result: ${e instanceof Error ? e.message : String(e)}`);
      return false;
    }
    try {
      const r = await commands.autoRunSaveRun({
        id,
        pbi_id: pbiId,
        started_at: String(Date.now()),
        cases: all,
      });
      if (r.status === "error") {
        toast.error(`Could not save the result: ${r.error}`);
        return false;
      }
    } catch (e) {
      // Same rethrow hazard as openBrowser/runStep: an Error out of the IPC
      // call would otherwise reject unhandled and leave the person staring
      // at a "Save result" click that appeared to do nothing.
      toast.error(`Could not save the result: ${e instanceof Error ? e.message : String(e)}`);
      return false;
    }
    toast.success(
      all.length === 1
        ? "Result saved on this machine."
        : `${all.length} results saved on this machine.`,
    );
    await queryClient.invalidateQueries({ queryKey: ["autorun-runs"] });
    return true;
  };

  const close = async () => {
    // Verdicts already marked are not thrown away by walking off.
    await writeRun(records);
    closedRef.current = true;
    await commands.autoRunCloseBrowser().catch(() => {});
    onClose();
  };

  return (
    <Modal onClose={close} className="w-full max-w-2xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">
        <span className="id-mono text-faint">#{caseId}</span> {title}
        {cases.length > 1 && (
          <span className="ml-2 text-xs font-normal text-faint">
            case {idx + 1} of {cases.length}
          </span>
        )}
      </h2>

      {!opened ? (
        <div className="space-y-2">
          <p className="text-xs text-muted">
            A real browser window opens with a fresh profile. Keep it beside this one and
            watch each step as it runs.
          </p>
          <div className="flex items-center gap-2">
            <label className="text-xs text-muted">
              Browser
              <Select
                aria-label="Browser to run in"
                className="ml-2 w-40"
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
                <option value="edge">Microsoft Edge</option>
                <option value="chrome">Google Chrome</option>
              </Select>
            </label>
            <Button size="sm" disabled={busy} onClick={openBrowser}>
              Open browser
            </Button>
          </div>
        </div>
      ) : (
        <ul className="max-h-72 space-y-2 overflow-y-auto">
          {(script.data?.steps ?? []).map((s) => (
            <li key={s.step_number} className="rounded-md border border-border p-2">
              <div className="flex items-center gap-2">
                <span className="text-xs font-medium text-muted">Step {s.step_number}</span>
                <span className="text-[11px] text-faint">
                  {s.actions.length} action{s.actions.length === 1 ? "" : "s"}
                </span>
                <Button
                  className="ml-auto"
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  onClick={() => runStep(s.step_number)}
                >
                  Run step {s.step_number}
                </Button>
              </div>
              {(results[s.step_number] ?? []).map((o, i) => (
                <p
                  key={i}
                  className={cn("mt-1 text-xs", o.ok ? "text-muted" : "text-danger")}
                >
                  {o.detail}
                </p>
              ))}
            </li>
          ))}
        </ul>
      )}

      <div className="space-y-2 border-t border-border pt-3">
        <span className="text-xs font-medium text-muted">Your verdict</span>
        <div className="flex gap-2">
          {VERDICTS.map((v) => (
            <button
              key={v}
              aria-pressed={verdict === v}
              className={cn(
                "rounded-md border border-border px-3 py-1.5 text-xs font-medium transition-colors",
                verdict === v ? verdictTone[v] : "text-muted hover:border-border-strong",
              )}
              onClick={() => setVerdict(v)}
            >
              {v}
            </button>
          ))}
        </div>
        <Textarea
          aria-label="Result note"
          className="h-16 w-full text-xs"
          placeholder="What you saw (optional)"
          value={note}
          onChange={(e) => setNote(e.target.value)}
        />
      </div>

      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={close}>
          <IconCancel aria-hidden />
          Close
        </Button>
        <Button size="sm" disabled={!verdict} onClick={save}>
          <IconConfirm aria-hidden />
          {isLast ? "Save result" : "Save and next case"}
        </Button>
      </div>
    </Modal>
  );
}
