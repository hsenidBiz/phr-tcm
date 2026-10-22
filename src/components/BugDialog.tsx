import { useMutation, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useState } from "react";
import { toast } from "../lib/toast";
import { commands, type Step, type TestCaseFull } from "../bindings";
import { imageMime } from "../lib/blob";
import { firstValidClipboardImage, imageOfPasteFile, NOT_ON_CLIPBOARD_YET, type ClipboardImage } from "../lib/clipboardImage";
import { unwrapStr } from "../lib/ipc";
import { sharedStepQueryKey, sharedStepText } from "../lib/sharedSteps";
import { Button } from "./ui/button";
import { Input, Textarea } from "./ui/input";
import { Modal } from "./ui/modal";
import { IconBug, IconCancel } from "../lib/actionIcons";

/** A line that opens (or closes) a fenced code block. */
const FENCE = /^ {0,3}(`{3,}|~{3,})/;

/** One step of the repro as a Markdown list item. The repro goes to Azure
 * DevOps as rich text, so the step's other lines are indented under its
 * number (a list item's continuation): a fenced query in a step then opens
 * and closes inside that step, and the expected result and the steps after
 * it stay out of the code block. The expected result is a line of its own. */
function reproStep(n: number, action: string, expected: string): string {
  const marker = `${n}. `;
  const pad = " ".repeat(marker.length);
  const lines = action.split(/\r?\n/);
  if (expected) {
    const exp = expected.split(/\r?\n/);
    // "Expected: ```sql" would not open the fence, and its closing line
    // would then open one that never closes.
    if (FENCE.test(exp[0])) lines.push("Expected:", ...exp);
    else lines.push(`Expected: ${exp[0]}`, ...exp.slice(1));
  }
  return lines.map((l, i) => (i === 0 ? marker + l : l.trim() === "" ? "" : pad + l)).join("\n");
}

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
  const qc = useQueryClient();
  // A Shared Steps reference has no text of its own: it is named, with its
  // title when a screen has already read it.
  const stepLine = (s: Step, i: number) =>
    s.shared != null
      ? `${i + 1}. ${sharedStepText(s.shared, qc.getQueryData<string>(sharedStepQueryKey(org, s.shared)))}`
      : reproStep(i + 1, s.action, s.expected);
  const defaultRepro = [
    `Test case #${testCase.id}: ${testCase.title}`,
    "",
    "Steps:",
    ...testCase.steps.map(stepLine),
  ].join("\n");

  const [title, setTitle] = useState(`Bug: ${testCase.title}`);
  const [repro, setRepro] = useState(defaultRepro);
  // Screenshots pasted INTO the dialog (Ctrl+V) - evidence of the bug that
  // was never attached to the run, e.g. a capture sitting on the clipboard.
  const [pasted, setPasted] = useState<ClipboardImage[]>([]);

  // The same check and retry the runner's paste has: a clipboard that offers
  // the picture before it is ready gives nothing or its front half, and that
  // must not reach the bug. Nothing is added, and the person is told, until
  // a whole picture arrives.
  const onPaste = (e: React.ClipboardEvent) => {
    const item = [...e.clipboardData.items].find((i) => i.type.startsWith("image/"));
    if (!item) return;
    // Taken now: the event's item list closes when this handler returns.
    const blob = item.getAsFile();
    void (async () => {
      const first = await imageOfPasteFile(blob, item.type);
      const img = await firstValidClipboardImage(first, () => imageOfPasteFile(blob, item.type));
      if (img) {
        setPasted((x) => [...x, img]);
        toast.success("Screenshot added to the bug");
      } else {
        toast.info(NOT_ON_CLIPBOARD_YET);
      }
    })();
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
          {pasted.map((img, i) => (
            <span key={i} className="relative">
              <img
                src={`data:${imageMime(img.mime) ?? "image/png"};base64,${img.b64}`}
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
          onClick={() => file.mutate({ title, repro, shots: [...screenshots, ...pasted.map((p) => p.b64)] })}
        >
          <IconBug aria-hidden />
          {file.isPending ? "Filing" : "File bug"}
        </Button>
      </div>
      </div>
    </Modal>
  );
}
