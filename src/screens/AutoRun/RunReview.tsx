// Reviewing one run: confirm or change each case's proposed verdict.
//
// `proposed` is the machine's best guess, shown as evidence next to a
// button the person still has to press themselves - nothing here is ever
// preselected. Saving writes the WHOLE run this screen loaded with only
// `verdict` and `note` replaced, so a field this screen doesn't know about
// (added by a later task) survives untouched.

import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { commands, type LocalRun_Serialize } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Textarea } from "../../components/ui/input";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { VERDICTS, verdictTone } from "./verdicts";

/** Epoch milliseconds as a string; the Rust side sends it that way because
 * specta will not carry a u64 across IPC. Same helper as PastRuns - kept
 * local rather than shared, the way the other screens' own date lines are. */
function when(ms: string): string {
  const n = Number(ms);
  if (!Number.isFinite(n) || n <= 0) return "unknown time";
  return new Date(n).toLocaleString();
}

/** The quiet line under a case's title: what the machine would say, never
 * a verdict. `reason` is one sentence explaining why - absent only for a
 * hand-typed fixture, never for a real run. */
function proposalLine(c: { proposed?: string; reason?: string }): string {
  const reason = c.reason ?? "";
  if (c.proposed) return `Proposed: ${c.proposed}${reason ? ` - ${reason}` : ""}`;
  return `Nothing proposed${reason ? ` - ${reason}` : ""}`;
}

export default function RunReview(props: {
  org: string;
  project: string;
  pbiTitle: string;
  runId: string;
  /** The case's real Azure DevOps step ids, aligned with its script steps.
   * Only Task 8's Send button reads this - accepted here so the host does
   * not change twice. */
  stepIds: Record<number, string[]>;
  onClose: () => void;
}) {
  const { runId, onClose } = props;
  const queryClient = useQueryClient();

  const query = useQuery<LocalRun_Serialize | null>({
    queryKey: ["autorun-run", runId],
    queryFn: () => unwrapStr(commands.autoRunLoadRun(runId)),
    retry: false,
  });

  /** The run this screen edits. Copied from the loaded run exactly ONCE -
   * a later refetch (the one Save itself triggers) must not clobber edits
   * already in progress with whatever the server happens to say. */
  const [run, setRun] = useState<LocalRun_Serialize | null>(null);
  useEffect(() => {
    if (run === null && query.data) setRun(query.data);
  }, [run, query.data]);

  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const toggleExpanded = (caseId: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(caseId)) next.delete(caseId);
      else next.add(caseId);
      return next;
    });

  const [shot, setShot] = useState<string | null>(null);
  const openShot = (name: string) =>
    unwrapStr(commands.autoRunShot(name))
      .then(setShot)
      .catch((e) => toast.error(`Could not open the screenshot: ${e.message ?? e}`));

  const [saving, setSaving] = useState(false);

  const gone = query.isSuccess && query.data === null;

  const setVerdict = (caseId: number, v: string) =>
    setRun((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        cases: prev.cases.map((c) =>
          c.case_id === caseId ? { ...c, verdict: c.verdict === v ? "" : v } : c,
        ),
      };
    });

  const setNote = (caseId: number, note: string) =>
    setRun((prev) => {
      if (!prev) return prev;
      return { ...prev, cases: prev.cases.map((c) => (c.case_id === caseId ? { ...c, note } : c)) };
    });

  /** Fills only the verdicts nobody has touched yet - a case already given
   * a verdict by hand is left exactly as it is, and a case with nothing
   * proposed has nothing to fill it with. */
  const acceptAll = () =>
    setRun((prev) => {
      if (!prev) return prev;
      return {
        ...prev,
        cases: prev.cases.map((c) => (!c.verdict && c.proposed ? { ...c, verdict: c.proposed } : c)),
      };
    });

  const save = async () => {
    if (!run) return;
    setSaving(true);
    try {
      const r = await commands.autoRunSaveRun(run);
      if (r.status === "error") {
        toast.error(`Could not save the review: ${r.error}`);
        return;
      }
      await queryClient.invalidateQueries({ queryKey: ["autorun-runs"] });
      await queryClient.invalidateQueries({ queryKey: ["autorun-run", runId] });
      toast.success("Review saved.");
    } catch (e) {
      // Same rethrow hazard as the supervised pane's IPC calls - an Error
      // out of the generated wrapper would otherwise reject unhandled.
      toast.error(`Could not save the review: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSaving(false);
    }
  };

  if (gone) {
    return (
      <Modal onClose={onClose} className="w-full max-w-md space-y-3 p-4">
        <p className="text-sm text-muted">This run is no longer on this machine.</p>
        <div className="flex justify-end">
          <Button variant="ghost" size="sm" onClick={onClose}>
            <IconCancel aria-hidden />
            Close
          </Button>
        </div>
      </Modal>
    );
  }

  if (!run) {
    return (
      <Modal onClose={onClose} className="w-full max-w-md space-y-3 p-4">
        <p className="text-sm text-muted">Loading…</p>
      </Modal>
    );
  }

  const readOnly = Boolean(run.published);
  const confirmed = run.cases.filter((c) => c.verdict).length;

  return (
    <Modal onClose={onClose} className="w-full max-w-3xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">
        {when(run.started_at)} - {run.cases.length} case{run.cases.length === 1 ? "" : "s"}
      </h2>

      {!readOnly && (
        <div className="flex justify-end">
          <Button size="sm" variant="outline" onClick={acceptAll}>
            Accept every proposal
          </Button>
        </div>
      )}

      <ul className="max-h-[60vh] space-y-3 overflow-y-auto">
        {run.cases.map((c) => {
          const isExpanded = expanded.has(c.case_id);
          return (
            <li
              key={c.case_id}
              aria-label={`Case #${c.case_id} ${c.title}`}
              className="space-y-2 rounded-md border border-border bg-surface p-3 text-sm"
            >
              <div className="flex items-center gap-2">
                <span className="id-mono text-faint">#{c.case_id}</span>
                <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
              </div>
              <p className="text-xs text-muted">{proposalLine(c)}</p>

              <div className="flex gap-2">
                {VERDICTS.map((v) => {
                  const pressed = c.verdict === v;
                  return (
                    <button
                      key={v}
                      type="button"
                      aria-pressed={pressed}
                      disabled={readOnly}
                      className={cn(
                        "rounded-md border border-border px-3 py-1.5 text-xs font-medium transition-colors",
                        pressed ? verdictTone[v] : "text-muted hover:border-border-strong",
                      )}
                      onClick={() => setVerdict(c.case_id, v)}
                    >
                      {v}
                    </button>
                  );
                })}
              </div>

              <Textarea
                aria-label={`Note for #${c.case_id}`}
                className="h-14 w-full text-xs"
                placeholder="What you saw (optional)"
                disabled={readOnly}
                value={c.note}
                onChange={(e) => setNote(c.case_id, e.target.value)}
              />

              <button
                type="button"
                className="flex items-center gap-1 text-xs text-muted hover:text-accent"
                onClick={() => toggleExpanded(c.case_id)}
              >
                {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                {isExpanded ? "Hide steps" : "Show steps"}
              </button>

              {isExpanded && (
                <ul className="space-y-2">
                  {c.steps.map((s) => (
                    <li key={s.step_number} className="rounded-md border border-border/60 p-2 text-xs">
                      <div className="flex items-center gap-2">
                        <span className="font-medium text-muted">
                          {s.step_number === 0 ? "Sign in" : `Step ${s.step_number}`}
                        </span>
                        {s.screenshot && (
                          <button
                            type="button"
                            className="text-muted underline hover:text-accent"
                            onClick={() => openShot(s.screenshot!)}
                          >
                            Picture
                          </button>
                        )}
                      </div>
                      {s.outcomes.map((o, i) => (
                        <p
                          key={i}
                          className={cn("mt-1 flex items-center gap-2", o.ok ? "text-muted" : "text-danger")}
                        >
                          {/* Its own element, separate from the Picture button below - a
                              sibling button inside the same node would fold into this
                              text's own content and break an exact-text lookup on it. */}
                          <span>{o.detail}</span>
                          {o.screenshot && (
                            <button
                              type="button"
                              className="text-muted underline hover:text-accent"
                              onClick={() => openShot(o.screenshot!)}
                            >
                              Picture
                            </button>
                          )}
                        </p>
                      ))}
                    </li>
                  ))}
                </ul>
              )}
            </li>
          );
        })}
      </ul>

      {readOnly && run.published && (
        <p className="text-xs text-muted">Sent to Azure DevOps {when(run.published.at)}</p>
      )}

      <div className="flex items-center justify-between gap-2 border-t border-border pt-3">
        <p className="text-xs text-muted">
          {confirmed} of {run.cases.length} confirmed
        </p>
        <div className="flex gap-2">
          <Button variant="ghost" size="sm" disabled={saving} onClick={onClose}>
            <IconCancel aria-hidden />
            Close
          </Button>
          {!readOnly && (
            <Button size="sm" disabled={saving} onClick={save}>
              <IconConfirm aria-hidden />
              Save review
            </Button>
          )}
        </div>
      </div>

      {shot && (
        <Modal onClose={() => setShot(null)} className="max-h-[90vh] max-w-5xl overflow-auto p-3">
          <img src={shot} alt="Screenshot of the step" className="max-w-full" />
        </Modal>
      )}
    </Modal>
  );
}
