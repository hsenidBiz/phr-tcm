// Authoring the actions for one case, as JSON.
//
// JSON on purpose, for now: an assistant will generate these scripts
// later, and the format has to be proven by hand before anything
// generates it. The case's own steps sit alongside as the reference.

import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type StepScript } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Textarea } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { unwrapStr } from "../../lib/ipc";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";

const PLACEHOLDER = `[
  {
    "step_number": 1,
    "actions": [
      { "kind": "navigate", "url": "https://app.example/login" },
      { "kind": "wait_for", "selector": "#user", "timeout_ms": 5000 },
      { "kind": "fill", "selector": "#user", "value": "tester" },
      { "kind": "click", "selector": "text=Sign in" },
      { "kind": "check_text", "value": "Dashboard" }
    ]
  }
]`;

export default function ScriptEditor({
  caseId,
  title,
  steps,
  onClose,
}: {
  caseId: number;
  title: string;
  steps: { action: string; expected: string }[];
  onClose: () => void;
}) {
  const existing = useQuery({
    queryKey: ["autorun-script", caseId],
    queryFn: () => unwrapStr(commands.autoRunLoadScript(caseId)),
    retry: false,
  });

  const [text, setText] = useState<string | null>(null);
  const [problem, setProblem] = useState("");
  const value =
    text ?? (existing.data ? JSON.stringify(existing.data.steps, null, 2) : "");

  const save = async () => {
    let parsed: StepScript[];
    try {
      parsed = JSON.parse(value || "[]") as StepScript[];
    } catch (e) {
      setProblem(`That is not valid JSON: ${(e as Error).message}`);
      return;
    }
    if (!Array.isArray(parsed)) {
      setProblem("That is not valid JSON: the script must be an array of steps.");
      return;
    }
    setProblem("");
    const r = await commands.autoRunSaveScript({ case_id: caseId, title, steps: parsed });
    if (r.status === "error") {
      toast.error(`Could not save the script: ${r.error}`);
      return;
    }
    toast.success("Script saved.");
    onClose();
  };

  return (
    <Modal onClose={onClose} className="w-full max-w-3xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">
        <span className="id-mono text-faint">#{caseId}</span> {title}
      </h2>

      <div className="grid gap-3 lg:grid-cols-2">
        <div className="space-y-1">
          <span className="text-xs font-medium text-muted">The case's steps</span>
          <ol className="max-h-64 space-y-1 overflow-y-auto text-xs text-muted">
            {steps.map((s, i) => (
              <li key={i} className="rounded border border-border/60 px-2 py-1">
                <span className="text-text">
                  {i + 1}. {s.action}
                </span>
                {s.expected && <div className="text-faint">→ {s.expected}</div>}
              </li>
            ))}
          </ol>
        </div>

        <label className="block text-xs text-muted">
          Action script JSON
          <Textarea
            aria-label="Action script JSON"
            className="mt-1 h-64 w-full font-mono text-xs"
            placeholder={PLACEHOLDER}
            value={value}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
      </div>

      {problem && <p className="text-xs text-danger">{problem}</p>}

      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" onClick={save}>
          <IconConfirm aria-hidden />
          Save script
        </Button>
      </div>
    </Modal>
  );
}
