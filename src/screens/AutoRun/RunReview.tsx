// Reviewing one run: confirm or change each case's proposed verdict.
//
// `proposed` is the machine's best guess, shown as evidence next to a
// button the person still has to press themselves - nothing here is ever
// preselected. Saving writes the WHOLE run this screen loaded with only
// `verdict` and `note` replaced, so a field this screen doesn't know about
// (added by a later task) survives untouched.

import { ChevronDown, ChevronRight } from "lucide-react";
import { Fragment, useEffect, useState } from "react";
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "../../lib/toast";
import { commands, type LocalRun_Serialize, type PublishResult } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Textarea } from "../../components/ui/input";
import { cn } from "../../lib/cn";
import { unwrap, unwrapStr } from "../../lib/ipc";
import { IconCancel, IconConfirm, IconOpenInBrowser, IconSendResults } from "../../lib/actionIcons";
import ResultFilterRow from "./ResultFilterRow";
import RetriedBadge from "./RetriedBadge";
import RunDownloads from "./RunDownloads";
import NoticeBadge from "./NoticeBadge";
import VerdictPicker from "./VerdictPicker";
import { resetLinesBefore } from "./plan";
import { countBuckets, matchesFilter, replayStep, type ResultFilter } from "./verdicts";

/** The result of a successful send - never the "refused" branch, which
 * never has anything to show beyond its own sentence. */
type SentResult = Extract<PublishResult, { status: "sent" }>;

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

/** What both the disabled Send button's title and a refused send say when
 * this run belongs to a different PBI than the one now selected. */
function mismatchSentence(runPbiId: number): string {
  return `this run is for PBI #${runPbiId} - select that PBI to send it`;
}

/** What a step's line is called in the review: the sign-in, the runner's
 * own trip to the case's module, or the case's own step. */
export function stepLabel(stepNumber: number): string {
  if (stepNumber === 0) return "Sign in";
  if (stepNumber === -1) return "Module";
  return `Step ${stepNumber}`;
}

export default function RunReview(props: {
  org: string;
  project: string;
  pbiTitle: string;
  /** The PBI currently selected on the Auto Run screen. `run.pbi_id` is
   * always what actually gets sent (see `doSend` below); this is only
   * used to catch a run reviewed under a DIFFERENT PBI than the one now
   * selected - sending it would use this PBI's title and step ids, which
   * belong to the wrong PBI entirely. */
  pbiId: number;
  runId: string;
  /** The case's real Azure DevOps step ids, aligned with its script steps.
   * Only the Send button reads this, to build what `auto_run_publish`
   * sends - accepted here so the host does not change twice. */
  stepIds: Record<number, string[]>;
  /** Each case's Shared Steps rows, 1-based. Sent with the run so a script
   * numbered without them never has its step marks recorded a row off.
   * Required, like `stepIds`: a caller that silently omits it would let
   * publish skip the check that refuses step marks on a Shared Steps row. */
  sharedSteps: Record<number, number[]>;
  onClose: () => void;
  /** Replay case `caseId` in the supervised browser up to the page before
   * `step`, its first failed step. Not offered on a run of another PBI: the
   * pane it opens saves under the PBI now selected. */
  onReplay?: (caseId: number, title: string, step: number) => void;
}) {
  const { runId, onClose, onReplay } = props;
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
  /** The verdict/note pair last known to be ON DISK, per case id - set
   * alongside `run` on load and refreshed after a successful save. Send
   * compares against this, not against `query.data` (which a save's own
   * refetch can move out from under the edit already in progress) - an
   * edit made after the last save is "dirty" and must not be sent, because
   * sending it would send whatever is on disk instead of what the person
   * just picked. */
  const [baseline, setBaseline] = useState<Record<number, { verdict: string; note: string }> | null>(
    null,
  );
  useEffect(() => {
    if (run === null && query.data) {
      setRun(query.data);
      setBaseline(
        Object.fromEntries(query.data.cases.map((c) => [c.case_id, { verdict: c.verdict, note: c.note }])),
      );
    }
  }, [run, query.data]);
  const dirty =
    run !== null &&
    baseline !== null &&
    run.cases.some((c) => {
      const b = baseline[c.case_id];
      return !b || b.verdict !== c.verdict || b.note !== c.note;
    });

  // One script lookup per case, so a step can say WHY it was never checked
  // - the run only recorded outcomes, not the script's own `unchecked`
  // reasons. Same query key `index.tsx`'s own script lookup uses, so a
  // script the person just saved from the editor is not fetched twice.
  const scripts = useQueries({
    queries: (run?.cases ?? []).map((c) => ({
      queryKey: ["autorun-script", c.case_id],
      queryFn: () => unwrapStr(commands.autoRunLoadScript(c.case_id)),
      retry: false,
    })),
  });
  const unchecked = (caseId: number, stepNumber: number): string | null => {
    const i = (run?.cases ?? []).findIndex((c) => c.case_id === caseId);
    const script = i >= 0 ? scripts[i]?.data : null;
    return script?.steps.find((s) => s.step_number === stepNumber)?.unchecked?.trim() || null;
  };

  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const toggleExpanded = (caseId: number) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(caseId)) next.delete(caseId);
      else next.add(caseId);
      return next;
    });

  /** Which cases the list shows. Only the LIST: Accept every proposal,
   * Save and Send always act on the whole run. */
  const [filter, setFilter] = useState<ResultFilter>("All");
  /** The cases the filter matched WHEN IT WAS PICKED, by id; `null` under
   * All. Frozen on purpose: a reviewer filtering to Failed who marks a
   * false failure Passed must keep that card - to check it, to write its
   * note - rather than have it vanish (taking keyboard focus with it) the
   * moment its bucket changes. The counts stay live; the list follows them
   * only when a filter is picked again. */
  const [visibleIds, setVisibleIds] = useState<Set<number> | null>(null);
  const pickFilter = (f: ResultFilter) => {
    setFilter(f);
    setVisibleIds(
      f === "All" || !run ? null : new Set(run.cases.filter((c) => matchesFilter(c, f)).map((c) => c.case_id)),
    );
  };

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
      // What is on disk now matches this screen's own edits - Send is no
      // longer blocked on an unsaved change.
      setBaseline(Object.fromEntries(run.cases.map((c) => [c.case_id, { verdict: c.verdict, note: c.note }])));
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

  const [confirming, setConfirming] = useState(false);
  const [sending, setSending] = useState(false);
  /** Set only by THIS screen's own send, this session - a run that was
   * already sent before this dialog opened has `run.published` instead,
   * and shows only the one-line "Sent to Azure DevOps ..." below. */
  const [sendResult, setSendResult] = useState<SentResult | null>(null);
  /** A refusal is an answer, not a failure - shown in place, not as a
   * toast, and it does not go away just because the person tries again. */
  const [sendRefusal, setSendRefusal] = useState<string | null>(null);

  const pbiMismatch = (r: LocalRun_Serialize) => r.pbi_id !== props.pbiId;

  const doSend = async () => {
    if (!run) return;
    setConfirming(false);
    // Defence in depth: the Send button is disabled for a mismatched PBI
    // (see `sendDisabled` below), but this is the one place that actually
    // reaches Azure DevOps, so it refuses on its own too rather than
    // trusting the button was never somehow pressed anyway.
    if (pbiMismatch(run)) {
      setSendRefusal(mismatchSentence(run.pbi_id));
      return;
    }
    setSending(true);
    setSendRefusal(null);
    try {
      const confirmedCases = run.cases.filter((c) => c.verdict);
      const r = await unwrap(
        commands.autoRunPublish(
          props.org,
          props.project,
          run.pbi_id,
          run.id,
          `${props.pbiTitle} - Auto Run`,
          confirmedCases.map((c) => ({
            case_id: c.case_id,
            step_ids: props.stepIds[c.case_id] ?? [],
            shared_steps: props.sharedSteps[c.case_id] ?? [],
          })),
        ),
      );
      if (r.status === "refused") {
        setSendRefusal(r.why);
        return;
      }
      setSendResult(r);
      // Marks this run read-only right away, the same as reloading it -
      // nothing here is ever reversible from this screen, so there is
      // nothing to gain by waiting on a refetch to say the same thing.
      setRun((prev) =>
        prev ? { ...prev, published: { run_id: r.run_id, web_url: r.web_url, at: String(Date.now()) } } : prev,
      );
      await queryClient.invalidateQueries({ queryKey: ["autorun-runs"] });
      await queryClient.invalidateQueries({ queryKey: ["autorun-run", runId] });
    } catch (e) {
      // Same shape as every other Azure DevOps failure in this app - see
      // RunnerWindow's recordResult handling.
      toast.error(`Could not send to Azure DevOps: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setSending(false);
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

  if (query.isError) {
    return (
      <Modal onClose={onClose} className="w-full max-w-md space-y-3 p-4">
        <p className="text-sm text-danger">
          {query.error instanceof Error ? query.error.message : String(query.error)}
        </p>
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
  const unconfirmed = run.cases.length - confirmed;
  const mismatch = pbiMismatch(run);
  const sendDisabled = dirty || confirmed === 0 || sending || mismatch;
  const sendTitle = mismatch ? mismatchSentence(run.pbi_id) : dirty ? "Save the review first" : undefined;
  // Counted off the cases as they stand on screen, so a verdict confirmed
  // here moves its case to its new bucket at once.
  const counts = countBuckets(run.cases);
  const shown = visibleIds ? run.cases.filter((c) => visibleIds.has(c.case_id)) : run.cases;

  return (
    <Modal onClose={onClose} className="w-full max-w-3xl space-y-3 p-4">
      <h2 className="text-sm font-semibold text-text">
        {when(run.started_at)} - {run.cases.length} case{run.cases.length === 1 ? "" : "s"}
      </h2>

      <ResultFilterRow value={filter} onChange={pickFilter} total={run.cases.length} counts={counts} />

      {shown.length === 0 && <p className="text-xs text-muted">No case in this run matches that filter.</p>}

      <ul className="max-h-[60vh] space-y-3 overflow-y-auto">
        {shown.map((c) => {
          const isExpanded = expanded.has(c.case_id);
          const replayTo = onReplay && !mismatch ? replayStep(c) : null;
          return (
            <Fragment key={c.case_id}>
              {/* Where the run paused for a reset, and how it went on. */}
              {resetLinesBefore(run.resets, c.case_id).map((line) => (
                <li key={line} className="border-l-2 border-l-warning pl-2 text-xs text-warning">
                  {line}
                </li>
              ))}
              <li
                aria-label={`Case #${c.case_id} ${c.title}`}
                className="space-y-2 rounded-md border border-border bg-surface p-3 text-sm"
              >
                <div className="flex items-center gap-2">
                  <span className="id-mono text-faint">#{c.case_id}</span>
                  <span className="min-w-0 flex-1 truncate text-text">{c.title}</span>
                  <RetriedBadge first={c.retried} />
                  <NoticeBadge notice={c.notice} />
                  {replayTo !== null && (
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Replay to step ${replayTo} for case ${c.case_id}`}
                      onClick={() => onReplay?.(c.case_id, c.title, replayTo)}
                    >
                      Replay to step {replayTo}
                    </Button>
                  )}
                </div>
                <p className="text-xs text-muted">{proposalLine(c)}</p>

                <VerdictPicker
                  value={c.verdict}
                  onPick={(v) => setVerdict(c.case_id, v)}
                  label={`Verdict for #${c.case_id}`}
                  disabled={readOnly}
                />

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
                            {stepLabel(s.step_number)}
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
                        {(() => {
                          const reason = unchecked(c.case_id, s.step_number);
                          return (
                            reason && <p className="mt-1 text-muted">not checked: {reason}</p>
                          );
                        })()}
                      </li>
                    ))}
                  </ul>
                )}

                {/* The files the case's steps saved, shown whether or not the
                    steps are unfolded; nothing for a case that saved none. */}
                <RunDownloads runId={run.id} steps={c.steps} />
              </li>
            </Fragment>
          );
        })}
      </ul>

      {readOnly && run.published && (
        <p className="text-xs text-muted">Sent to Azure DevOps {when(run.published.at)}</p>
      )}

      <div className="border-t border-border pt-3">
        {sendResult ? (
          // Replaces the usual footer entirely - this run is read-only now
          // (see `readOnly` above), so there is nothing left to do here but
          // read what happened and leave.
          <div className="space-y-2">
            <p className="text-sm font-medium text-text">
              Sent: {sendResult.sent.length} result{sendResult.sent.length === 1 ? "" : "s"} recorded.
            </p>
            <Button
              size="sm"
              variant="outline"
              onClick={() =>
                openUrl(sendResult.web_url).catch(() => toast.error("Could not open the browser."))
              }
            >
              <IconOpenInBrowser aria-hidden />
              Open the run
            </Button>
            {sendResult.skipped.length > 0 && (
              <ul className="space-y-1 text-xs text-warning">
                {sendResult.skipped.map((s) => (
                  <li key={s.case_id}>
                    #{s.case_id}: {s.why}
                  </li>
                ))}
              </ul>
            )}
            {sendResult.problems.length > 0 && (
              <ul className="space-y-1 text-xs text-warning">
                {sendResult.problems.map((p, i) => (
                  <li key={i}>{p}</li>
                ))}
              </ul>
            )}
            <div className="flex justify-end">
              <Button variant="ghost" size="sm" onClick={onClose}>
                <IconCancel aria-hidden />
                Close
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-wrap items-center justify-between gap-2">
            {/* The count and the one action that changes it in bulk sit
                together: "Accept every proposal" is about the tally on its
                left, not a separate step of its own. */}
            <div className="flex items-center gap-3">
              <p className="text-xs text-muted">
                {confirmed} of {run.cases.length} confirmed
              </p>
              {!readOnly && (
                <Button size="sm" variant="outline" onClick={acceptAll}>
                  Accept every proposal
                </Button>
              )}
            </div>
            {/* The filter narrows the list, never what the buttons act on -
                said only while it could surprise, i.e. while some cases are
                hidden. */}
            {!readOnly && filter !== "All" && (
              <p className="order-last w-full text-xs text-faint">
                The filter only changes what is listed. Accept every proposal, Save review and Send act
                on every case in this run.
              </p>
            )}
            <div className="flex flex-wrap items-center justify-end gap-2">
              {/* A refusal is an answer, not a toast - it stays on screen
                  until the next attempt changes it. */}
              {sendRefusal && <p className="text-xs text-warning">{sendRefusal}</p>}
              <Button variant="ghost" size="sm" disabled={saving} onClick={onClose}>
                <IconCancel aria-hidden />
                Close
              </Button>
              {!readOnly && (
                <>
                  {/* Send is the primary action: it is what the whole
                      review is for, and the one that leaves this machine.
                      It still goes through the confirm below. Save is the
                      way to stop part-way and come back. */}
                  <Button size="sm" variant="outline" disabled={saving} onClick={save}>
                    <IconConfirm aria-hidden />
                    Save review
                  </Button>
                  <Button
                    size="sm"
                    disabled={sendDisabled}
                    title={sendTitle}
                    onClick={() => setConfirming(true)}
                  >
                    <IconSendResults aria-hidden />
                    Send to Azure DevOps
                  </Button>
                </>
              )}
            </div>
          </div>
        )}
      </div>

      {confirming && (
        <Modal onClose={() => setConfirming(false)} className="w-full max-w-md space-y-3 p-4">
          <p className="text-sm text-text">
            This creates one test run in Azure DevOps for "{props.pbiTitle}" with {confirmed} confirmed
            result{confirmed === 1 ? "" : "s"}. {unconfirmed} unconfirmed case
            {unconfirmed === 1 ? " is" : "s are"} left out. Nothing in Azure DevOps is deleted or
            overwritten; the run cannot be taken back from here.
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" onClick={() => setConfirming(false)}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={sending} onClick={doSend}>
              <IconSendResults aria-hidden />
              {sending ? "Sending" : "Confirm"}
            </Button>
          </div>
        </Modal>
      )}

      {shot && (
        // max-h-full, not a vh: the backdrop starts below the title bar, so
        // 90vh of the window no longer fits inside it.
        <Modal onClose={() => setShot(null)} className="max-h-full max-w-5xl overflow-auto p-3">
          <img src={shot} alt="Screenshot of the step" className="max-w-full" />
        </Modal>
      )}
    </Modal>
  );
}
