import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type TestCaseFull } from "../bindings";
import { unwrapStr } from "../lib/ipc";
import { Button } from "./ui/button";
import { Input, Textarea } from "./ui/input";

/** Prefills title + repro from the case's failed steps, then POSTs a Bug
 * (or Issue) linked to the test case and PBI, with the runner's screenshots. */
export default function BugDialog({
  org,
  project,
  testCase,
  pbiId,
  screenshots,
  onClose,
  onFiled,
}: {
  org: string;
  project: string;
  testCase: TestCaseFull;
  pbiId: number;
  screenshots: string[];
  onClose: () => void;
  onFiled: (bugId: number) => void;
}) {
  const defaultRepro = [
    `Test case #${testCase.id}: ${testCase.title}`,
    "",
    "Steps:",
    ...testCase.steps.map((s, i) => `${i + 1}. ${s.action}${s.expected ? ` -> expected: ${s.expected}` : ""}`),
  ].join("\n");

  const [title, setTitle] = useState(`Bug: ${testCase.title}`);
  const [repro, setRepro] = useState(defaultRepro);

  const file = useMutation({
    mutationFn: () =>
      unwrapStr(commands.fileBug(org, project, title.trim(), repro, testCase.id, pbiId, screenshots)),
    onSuccess: (bug) => {
      toast.success(`Filed bug #${bug.id}`);
      onFiled(bug.id);
    },
    onError: (e) => toast.error(`Could not file bug: ${e.message}`),
  });

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4" onClick={onClose}>
      <div
        className="w-full max-w-lg space-y-3 rounded-lg border border-border bg-surface p-4 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="text-sm font-semibold text-text">File a bug</h2>
        <Input
          aria-label="Bug title"
          className="w-full"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
        <Textarea
          aria-label="Repro steps"
          className="h-40 w-full font-mono text-xs"
          value={repro}
          onChange={(e) => setRepro(e.target.value)}
        />
        <p className="text-xs text-muted">
          Links to test case #{testCase.id} and PBI #{pbiId}
          {screenshots.length > 0 && ` · ${screenshots.length} screenshot(s) attached`}.
        </p>
        <div className="flex justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={onClose}>
            Cancel
          </Button>
          <Button size="sm" disabled={!title.trim() || file.isPending} onClick={() => file.mutate()}>
            {file.isPending ? "Filing" : "File bug"}
          </Button>
        </div>
      </div>
    </div>
  );
}
