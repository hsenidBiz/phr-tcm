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
  caseId,
  title,
  onClose,
}: {
  pbiId: number;
  caseId: number;
  title: string;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();

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

  // The browser is a real Edge process with a temp profile directory - it
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
    const r = await commands.autoRunOpenBrowser();
    setBusy(false);
    if (r.status === "error") {
      toast.error(`Could not open the browser: ${r.error}`);
      return;
    }
    setOpened(true);
  };

  const runStep = async (stepNumber: number) => {
    const step = script.data?.steps.find((s) => s.step_number === stepNumber);
    if (!step) return;
    setBusy(true);
    const r = await commands.autoRunStep(step);
    setBusy(false);
    if (r.status === "error") {
      toast.error(r.error);
      return;
    }
    setResults((prev) => ({ ...prev, [stepNumber]: r.data }));
  };

  const save = async () => {
    let idRes: string;
    try {
      idRes = await commands.autoRunNewId();
    } catch (e) {
      toast.error(`Could not save the result: ${e instanceof Error ? e.message : String(e)}`);
      return;
    }
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
    const r = await commands.autoRunSaveRun({
      id: idRes,
      pbi_id: pbiId,
      started_at: String(Date.now()),
      cases: [record],
    });
    if (r.status === "error") {
      toast.error(`Could not save the result: ${r.error}`);
      return;
    }
    toast.success("Result saved on this machine.");
    await queryClient.invalidateQueries({ queryKey: ["autorun-runs"] });
    closedRef.current = true;
    await commands.autoRunCloseBrowser().catch(() => {});
    onClose();
  };

  const close = async () => {
    closedRef.current = true;
    await commands.autoRunCloseBrowser().catch(() => {});
    onClose();
  };

  return (
    <Modal onClose={close} className="w-full max-w-2xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">
        <span className="id-mono text-faint">#{caseId}</span> {title}
      </h2>

      {!opened ? (
        <div className="space-y-2">
          <p className="text-xs text-muted">
            A real Edge window opens with a fresh profile. Keep it beside this one and watch
            each step as it runs.
          </p>
          <Button size="sm" disabled={busy} onClick={openBrowser}>
            Open browser
          </Button>
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
          Save result
        </Button>
      </div>
    </Modal>
  );
}
