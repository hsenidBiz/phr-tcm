import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useState } from "react";
import { toast } from "../lib/toast";
import {
  commands,
  type ExtraField,
  type FieldViolation,
  type FileBugError,
  type Step,
  type TestCaseFull,
} from "../bindings";
import { CACHE, cacheKeys, cacheRead, cacheWrite } from "../lib/cache";
import { cn } from "../lib/cn";
import { imageMime } from "../lib/blob";
import { firstValidClipboardImage, imageOfPasteFile, NOT_ON_CLIPBOARD_YET, type ClipboardImage } from "../lib/clipboardImage";
import { describeAdoError, unwrap } from "../lib/ipc";
import { sharedStepQueryKey, sharedStepText } from "../lib/sharedSteps";
import { Button } from "./ui/button";
import DateField from "./ui/datefield";
import { Input, Textarea } from "./ui/input";
import { Select } from "./ui/select";
import { Modal } from "./ui/modal";
import { IconBug, IconCancel } from "../lib/actionIcons";

/** A line that opens (or closes) a fenced code block. */
const FENCE = /^ {0,3}(`{3,}|~{3,})/;

/** What a failed file-a-bug call says. An unknown shape (the dev fault
 * injector hands back a bare AdoError or a string) is shown as it is. */
export function fileBugErrorText(e: FileBugError): string {
  if (e && typeof e === "object" && "kind" in e) {
    if (e.kind === "Ado") return describeAdoError(e.detail);
    if (e.kind === "RuleViolations") {
      return `Azure DevOps needs: ${e.detail.map((v) => v.reference_name).join(", ")}`;
    }
  }
  return String(e);
}

/** The "YYYY-MM-DD" a date field holds, as the instant Azure DevOps takes:
 * local midnight of that day. */
export function toAdoDate(ymd: string): string {
  const [y, m, d] = ymd.split("-").map(Number);
  return new Date(y, m - 1, d).toISOString();
}

/** References of the fields still blank (whitespace counts as blank). */
export function missingRequired(fields: ExtraField[], values: Record<string, string>): string[] {
  return fields.filter((f) => !(values[f.reference_name] ?? "").trim()).map((f) => f.reference_name);
}

/** The process rules named fields: nothing was created. */
class RuleViolationsError extends Error {
  constructor(public violations: FieldViolation[]) {
    super("Azure DevOps rejected the bug's fields");
  }
}

async function unwrapFileBug<T>(
  p: Promise<{ status: "ok"; data: T } | { status: "error"; error: FileBugError }>,
): Promise<T> {
  const r = await p;
  if (r.status === "error") {
    const e = r.error;
    if (e && typeof e === "object" && "kind" in e && e.kind === "RuleViolations") {
      throw new RuleViolationsError(e.detail);
    }
    throw new Error(fileBugErrorText(e));
  }
  return r.data;
}

/** One step of the repro as a Markdown list item. The repro goes to Azure
 * DevOps as rich text, so the step's other lines are indented under its
 * number (a list item's continuation): a fenced query in a step then opens
 * and closes inside that step, and the expected result and the steps after
 * it stay out of the code block. The expected result is a line of its own. */
export function reproStep(n: number, action: string, expected: string): string {
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
  initialTitle,
  initialRepro,
  onClose,
  onFiled,
}: {
  org: string;
  project: string;
  testCase: TestCaseFull;
  pbiId: number;
  screenshots: string[];
  /** A caller that already knows the failure (an Auto Run review) hands in
   * its own title and repro; the manual runner passes neither and gets the
   * case's own steps. */
  initialTitle?: string;
  initialRepro?: string;
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

  const [title, setTitle] = useState(initialTitle ?? `Bug: ${testCase.title}`);
  const [repro, setRepro] = useState(initialRepro ?? defaultRepro);
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
  type BugPayload = {
    title: string;
    repro: string;
    shots: string[];
    fieldValues: { reference_name: string; value: string }[];
    remember: Record<string, string>;
  };

  const [values, setValues] = useState<Record<string, string>>(
    () => cacheRead<Record<string, string>>(cacheKeys.bugFieldValues(org, project), CACHE.picks.ttlMs) ?? {},
  );
  const [violations, setViolations] = useState<Record<string, string>>({});
  const [extraRefs, setExtraRefs] = useState<string[]>([]);
  // The fields this project's Bug requires. A failed lookup means no extra
  // fields: the bug can still be filed with a title, as before.
  const lookup = useQuery({
    queryKey: ["bug-fields", org, project, extraRefs],
    queryFn: () => unwrap(commands.bugFields(org, project, extraRefs)),
    staleTime: 30 * 60_000,
    retry: false,
    placeholderData: keepPreviousData,
  });
  const fields: ExtraField[] = lookup.data?.fields ?? [];

  const file = useMutation({
    mutationFn: (p: BugPayload) =>
      unwrapFileBug(
        commands.fileBug(org, project, p.title.trim(), p.repro, testCase.id, pbiId, p.shots, p.fieldValues),
      ),
    onSuccess: (bug, p) => {
      cacheWrite(cacheKeys.bugFieldValues(org, project), p.remember);
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
    onError: (e) => {
      if (e instanceof RuleViolationsError) {
        setViolations(Object.fromEntries(e.violations.map((v) => [v.reference_name, v.message])));
        const shown = new Set(fields.map((f) => f.reference_name));
        const fresh = e.violations.map((v) => v.reference_name).filter((r) => !shown.has(r));
        if (fresh.length > 0) setExtraRefs((x) => [...new Set([...x, ...fresh])]);
        toast.error("Azure DevOps needs more fields - fill in the highlighted ones and file again.");
      } else {
        toast.error(`Could not file bug: ${e.message}`);
      }
    },
  });

  const setValue = (ref: string, v: string) => {
    setValues((x) => ({ ...x, [ref]: v }));
    setViolations((x) => {
      if (!(ref in x)) return x;
      const rest = { ...x };
      delete rest[ref];
      return rest;
    });
  };
  const flagged = (ref: string) => ref in violations;
  const ring = (ref: string) => (flagged(ref) ? "ring-2 ring-danger" : undefined);
  const missing = missingRequired(fields, values);
  const labelOf = (ref: string) => fields.find((f) => f.reference_name === ref)?.label ?? ref;
  const lookupSettled = !lookup.isLoading && !lookup.isPlaceholderData;
  // A rejection on a field the lookup still does not return is said in
  // words, so the person learns why it failed.
  const unshown = lookupSettled
    ? Object.entries(violations).filter(([ref]) => !fields.some((f) => f.reference_name === ref))
    : [];

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
      {fields.map((f) => {
        const ref = f.reference_name;
        const v = values[ref] ?? "";
        return (
          <div key={ref} className="text-xs text-muted">
            {f.label}
            {f.kind === "pick" ? (
              <Select
                aria-label={f.label}
                className="mt-1"
                triggerClassName={cn(ring(ref))}
                value={v}
                onChange={(e) => setValue(ref, e.target.value)}
              >
                <option value="">Choose...</option>
                {f.allowed.map((a) => (
                  <option key={a} value={a}>
                    {a}
                  </option>
                ))}
              </Select>
            ) : f.kind === "date" ? (
              <DateField
                className="mt-1"
                ariaLabel={f.label}
                flagged={flagged(ref)}
                value={v}
                onChange={(d) => setValue(ref, d)}
              />
            ) : f.kind === "html" ? (
              <Textarea
                aria-label={f.label}
                className={cn("mt-1 h-20 w-full", ring(ref))}
                value={v}
                onChange={(e) => setValue(ref, e.target.value)}
              />
            ) : (
              <Input
                aria-label={f.label}
                className={cn("mt-1 w-full", ring(ref))}
                value={v}
                onChange={(e) => setValue(ref, e.target.value)}
              />
            )}
            {flagged(ref) && <div className="mt-1 text-xs text-danger">{violations[ref]}</div>}
          </div>
        );
      })}
      {unshown.map(([ref, msg]) => (
        <div key={ref} className="text-xs text-danger">
          {ref}: {msg}
        </div>
      ))}
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
          disabled={!title.trim() || file.isPending || !lookupSettled || missing.length > 0}
          title={missing.length > 0 ? `Fill in: ${missing.map(labelOf).join(", ")}` : undefined}
          onClick={() =>
            file.mutate({
              title,
              repro,
              shots: [...screenshots, ...pasted.map((p) => p.b64)],
              fieldValues: fields.map((f) => {
                const v = (values[f.reference_name] ?? "").trim();
                return { reference_name: f.reference_name, value: f.kind === "date" ? toAdoDate(v) : v };
              }),
              remember: values,
            })
          }
        >
          <IconBug aria-hidden />
          {file.isPending ? "Filing" : "File bug"}
        </Button>
      </div>
      </div>
    </Modal>
  );
}
