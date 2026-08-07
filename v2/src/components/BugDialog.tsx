import { useMutation } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type TestCaseFull } from "../bindings";
import { blobToB64 } from "../lib/blob";
import { unwrapStr } from "../lib/ipc";
import { Button } from "./ui/button";
import { Input, Textarea } from "./ui/input";
import { Modal } from "./ui/modal";
import { IconBug, IconCancel } from "../lib/actionIcons";

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
  // Screenshots pasted INTO the dialog (Ctrl+V) - evidence of the bug that
  // was never attached to the run, e.g. a capture sitting on the clipboard.
  const [pasted, setPasted] = useState<string[]>([]);

  const onPaste = (e: React.ClipboardEvent) => {
    const item = [...e.clipboardData.items].find((i) => i.type.startsWith("image/"));
    if (!item) return;
    const blob = item.getAsFile();
    if (!blob) return;
    void blobToB64(blob).then((b64) => {
      setPasted((x) => [...x, b64]);
      toast.success("Screenshot added to the bug");
    });
  };

  /** What gets filed, gathered at CLICK time rather than read out of the
   * mutation's own closure.
   *
   * react-query updates the observer's options in a passive effect, which
   * runs AFTER the commit that put the pasted thumbnail on screen. A click
   * landing in that gap - paste, then file before React flushes effects -
   * ran the PREVIOUS render's mutationFn and filed the bug with no
   * screenshot at all, silently: the toast still said "Filed bug #N".
   * `onClick` is updated during the commit itself, so a payload built
   * there is always the one the person can see. */
  type BugPayload = { title: string; repro: string; shots: string[] };

  const file = useMutation({
    mutationFn: (p: BugPayload) =>
      unwrapStr(
        commands.fileBug(org, project, p.title.trim(), p.repro, testCase.id, pbiId, p.shots),
      ),
    onSuccess: (bug) => {
      // The bug exists either way - re-filing over a failed upload would
      // leave a duplicate - but a tester who attached screenshots of the
      // failure has to be told when they did not arrive.
      if (bug.screenshots_failed > 0) {
        toast.warning(
          `Filed bug #${bug.id}, but ${bug.screenshots_failed} of ${bug.screenshots_total} screenshot(s) did not attach - add them in Azure DevOps.`,
          { duration: 20_000 },
        );
      } else {
        toast.success(`Filed bug #${bug.id}`);
      }
      onFiled(bug.id);
    },
    onError: (e) => toast.error(`Could not file bug: ${e.message}`),
  });

  return (
    <Modal onClose={onClose} className="w-full max-w-lg p-4">
      <div className="space-y-3" onPaste={onPaste}>
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
      {pasted.length > 0 && (
        <div className="flex flex-wrap gap-2">
          {pasted.map((b64, i) => (
            <span key={i} className="relative">
              <img
                src={`data:image/png;base64,${b64}`}
                alt={`Pasted screenshot ${i + 1}`}
                className="h-14 rounded border border-border object-cover"
              />
              <button
                aria-label={`Remove pasted screenshot ${i + 1}`}
                className="absolute -right-1.5 -top-1.5 rounded-full border border-border bg-surface p-0.5 text-muted hover:text-danger"
                onClick={() => setPasted((x) => x.filter((_, j) => j !== i))}
              >
                <X size={11} />
              </button>
            </span>
          ))}
        </div>
      )}
      <p className="text-xs text-muted">
        Links to test case #{testCase.id} and PBI #{pbiId}
        {screenshots.length + pasted.length > 0 &&
          ` · ${screenshots.length + pasted.length} screenshot(s) attached`}
        . Paste (Ctrl+V) to add more.
      </p>
      <div className="flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button
          size="sm"
          disabled={!title.trim() || file.isPending}
          onClick={() => file.mutate({ title, repro, shots: [...screenshots, ...pasted] })}
        >
          <IconBug aria-hidden />
          {file.isPending ? "Filing" : "File bug"}
        </Button>
      </div>
      </div>
    </Modal>
  );
}
