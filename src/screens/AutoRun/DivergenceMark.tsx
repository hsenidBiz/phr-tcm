// A reported divergence on a case card: where the test case and the live
// application disagree. An assistant reports it instead of fitting a check
// to the page; the person rules here on which is right, the case or the
// app (or says the data was fixed). A ruling touches only the divergence
// record; nothing is written to Azure DevOps.

import { useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useId, useState } from "react";
import { commands, type Divergence, type Ruling } from "../../bindings";
import { Button } from "../../components/ui/button";
import {
  IconCancel,
  IconConfirm,
  IconDataFixed,
  IconHideDetails,
  IconOpenInBrowser,
  IconShowDetails,
  IconUndo,
} from "../../lib/actionIcons";
import { Term } from "../../components/Term";
import { toast } from "../../lib/toast";

/** True while a divergence stops its case's runs or waits on a repair. */
export function needsAttention(d: Divergence): boolean {
  return d.status === "awaiting_repair" || (d.status === "open" && d.kind !== "app");
}

export function badgeText(d: Divergence): string {
  if (d.status === "awaiting_repair") return "Waits for a repair";
  if (d.status === "ruled") return "Ruled";
  return d.kind === "app" ? "Divergence" : "Needs a ruling";
}

/** The word the badge's label is: a ruling for the ones that name it, the
 * divergence itself otherwise. */
function badgeTerm(d: Divergence): "ruling" | "divergence" {
  return badgeText(d) === "Needs a ruling" || badgeText(d) === "Ruled" ? "ruling" : "divergence";
}

export function DivergenceBadge({ divergence }: { divergence: Divergence }) {
  const describedBy = useId();
  const detail = `Step ${divergence.step}: the case says "${divergence.case_says}"; the app shows "${divergence.app_shows}" (${divergence.kind})${divergence.proposal ? `; proposed: "${divergence.proposal}"` : ""}`;
  const ruled = divergence.status === "ruled";
  return (
    <>
      <span
        aria-describedby={describedBy}
        className={
          ruled
            ? "shrink-0 rounded-md border border-border px-2 py-0.5 text-xs font-medium text-muted"
            : "shrink-0 rounded-md border border-warning/50 px-2 py-0.5 text-xs font-medium text-warning"
        }
      >
        <span className="label-trim">
          <Term name={badgeTerm(divergence)}>{badgeText(divergence)}</Term>
        </span>
      </span>
      <span id={describedBy} className="sr-only">
        {detail}
      </span>
    </>
  );
}

const SENTENCE: Record<Ruling, (step: number) => string> = {
  case_stands: (step) =>
    `The script keeps the case's value at step ${step}, so the run fails there until the app is fixed.`,
  data_fixed: () => "You changed the environment's data to match the case; the case runs again.",
  rewrite: () =>
    "Azure DevOps opens on the case so you can update it; the case waits until its script is repaired to match.",
};

export function DivergenceRow({
  org,
  project,
  caseId,
  divergence,
  compact = false,
  tag,
}: {
  org: string;
  project: string;
  caseId: number;
  divergence: Divergence;
  /** A ruling as one line (its step, what was decided, Details and Undo) for the
   * open case card. Only a ruled divergence draws so; an open one keeps its buttons. */
  compact?: boolean;
  /** The small word before a compact ruling's line. Default "Step N"; the open
   * case card shows a ruling under its own step and says "Ruling" there. */
  tag?: string;
}) {
  const detailsId = useId();
  const [showDetails, setShowDetails] = useState(false);
  const queryClient = useQueryClient();
  const [asking, setAsking] = useState<Ruling | null>(null);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const step = divergence.step;

  const rule = async (ruling: Ruling) => {
    setBusy(true);
    setProblem(null);
    try {
      if (ruling === "rewrite") {
        try {
          await openUrl(
            `https://dev.azure.com/${encodeURIComponent(org)}/${encodeURIComponent(project)}/_workitems/edit/${caseId}`,
          );
        } catch {
          toast.error("Could not open the browser.");
        }
      }
      const res = await commands.autoRunRuleDivergence(org, project, divergence.id, ruling);
      if (res.status === "error") {
        setProblem(res.error);
        return;
      }
      await queryClient.invalidateQueries({ queryKey: ["autorun-divergences", org, project] });
      setAsking(null);
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  // Takes an "update the case" ruling back: the divergence needs a ruling again.
  const undo = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const res = await commands.autoRunUndoDivergenceRuling(org, project, divergence.id);
      if (res.status === "error") {
        setProblem(res.error);
        return;
      }
      await queryClient.invalidateQueries({ queryKey: ["autorun-divergences", org, project] });
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const waiting = divergence.status === "awaiting_repair";
  // An assistant's ruling carries the person's own words from the chat, and
  // can be taken back; a ruling the person made with the buttons here cannot.
  const byAssistant = divergence.ruled_by === "assistant";
  // A rewrite the assistant ruled that a repair has since closed is done:
  // taking it back would block the case on a step the rewrite may have
  // renumbered. Any other assistant ruling keeps its Undo as long as it shows.
  const repaired = divergence.status === "ruled" && divergence.ruling === "rewrite";
  const canUndo = waiting || (byAssistant && !repaired);
  const undoButton = canUndo && (
    <Button
      size="sm"
      variant="outline"
      className="shrink-0"
      disabled={busy}
      aria-label={`Undo the ruling on #${caseId} step ${step}`}
      onClick={() => void undo()}
    >
      <IconUndo aria-hidden />
      Undo ruling
    </Button>
  );
  if (compact && divergence.status !== "open") {
    const decided =
      divergence.note?.trim() ||
      divergence.proposal?.trim() ||
      (divergence.ruling
        ? SENTENCE[divergence.ruling](step)
        : `The case says "${divergence.case_says}"; the app shows "${divergence.app_shows}".`);
    return (
      <div className="space-y-1 pt-1">
        <div className="flex items-center gap-2">
          <span className="shrink-0 rounded bg-accent/15 px-1.5 py-0.5 text-[10px] font-medium text-accent-fill">
            <span className="label-trim">{tag ?? `Step ${step}`}</span>
          </span>
          <span className="min-w-0 flex-1 truncate text-xs text-text" title={decided}>
            {decided}
          </span>
          {waiting && <DivergenceBadge divergence={divergence} />}
          <Button
            size="sm"
            variant="ghost"
            className="shrink-0"
            aria-label={`Details of the ruling on #${caseId} step ${step}`}
            aria-expanded={showDetails}
            aria-controls={showDetails ? detailsId : undefined}
            onClick={() => setShowDetails((v) => !v)}
          >
            {showDetails ? <IconHideDetails aria-hidden /> : <IconShowDetails aria-hidden />}
            Details
          </Button>
          {undoButton}
        </div>
        {waiting && <p className="break-words pl-1 text-xs text-text">Ask the assistant to repair the script (/tcm:heal).</p>}
        {showDetails && (
          <div id={detailsId} className="space-y-1 pl-1">
            <div className="flex flex-wrap items-center gap-2">
              {!waiting && <DivergenceBadge divergence={divergence} />}
              <span className="min-w-0 flex-1 break-words text-xs text-text">
                Step {step}: the case says &quot;{divergence.case_says}&quot;; the app shows &quot;{divergence.app_shows}
                &quot;.
                {divergence.proposal && <> Proposed: &quot;{divergence.proposal}&quot;.</>}
              </span>
            </div>
            {byAssistant && (
              <p className="break-words text-xs text-muted">
                Ruled by the assistant, on your words: &quot;{divergence.said}&quot;
                {divergence.note ? ` Its note: ${divergence.note}` : ""}
              </p>
            )}
          </div>
        )}
        {problem && (
          <p role="status" className="break-words text-xs text-danger">
            {problem}
          </p>
        )}
      </div>
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-2 pt-1">
      <DivergenceBadge divergence={divergence} />
      <span className="min-w-0 flex-1 break-words text-xs text-text">
        Step {step}: the case says &quot;{divergence.case_says}&quot;; the app shows &quot;{divergence.app_shows}&quot;.
        {divergence.proposal && <> Proposed: &quot;{divergence.proposal}&quot;.</>}
        {waiting && " Ask the assistant to repair the script (/tcm:heal)."}
      </span>
      {divergence.status === "open" && (
        <>
          {divergence.kind !== "app" && (
            <span className="w-full text-xs font-medium text-text">
              The case and the app disagree at step {step}. Which is right?
            </span>
          )}
          <Button
            size="sm"
            variant="outline"
            className="shrink-0"
            aria-label={`The app is wrong for #${caseId} step ${step}`}
            onClick={() => setAsking("case_stands")}
          >
            <IconConfirm aria-hidden />
            The app is wrong
          </Button>
          {divergence.kind === "data" && (
            <Button
              size="sm"
              variant="outline"
              className="shrink-0"
              aria-label={`I fixed the data for #${caseId} step ${step}`}
              onClick={() => setAsking("data_fixed")}
            >
              <IconDataFixed aria-hidden />
              I fixed the data
            </Button>
          )}
          <Button
            size="sm"
            variant="outline"
            className="shrink-0"
            aria-label={`The app is right, update case #${caseId} for step ${step}`}
            onClick={() => setAsking("rewrite")}
          >
            <IconOpenInBrowser aria-hidden />
            The app is right, update the case
          </Button>
        </>
      )}
      {byAssistant && (
        <span className="w-full break-words text-xs text-muted">
          Ruled by the assistant, on your words: &quot;{divergence.said}&quot;
          {divergence.note ? ` Its note: ${divergence.note}` : ""}
        </span>
      )}
      {undoButton}
      {asking && (
        <div className="flex w-full flex-wrap items-center gap-2 border-t border-border/60 pt-2">
          <span className="min-w-0 flex-1 text-xs text-text">{SENTENCE[asking](step)}</span>
          <Button size="sm" variant="ghost" onClick={() => setAsking(null)}>
            <IconCancel aria-hidden />
            Keep
          </Button>
          <Button size="sm" variant="outline" disabled={busy} onClick={() => void rule(asking)}>
            <IconConfirm aria-hidden />
            Confirm
          </Button>
        </div>
      )}
      {problem && (
        <p role="status" className="w-full break-words text-xs text-danger">
          {problem}
        </p>
      )}
    </div>
  );
}
