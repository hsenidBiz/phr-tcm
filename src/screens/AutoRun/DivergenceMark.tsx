// A reported divergence on a case card: where the test case and the live
// application disagree. An assistant reports it instead of fitting a check
// to the page; the person rules here - the case is right, the data is
// fixed, or the case is rewritten in Azure DevOps. A ruling touches only
// the divergence record; nothing is written to Azure DevOps.

import { useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useId, useState } from "react";
import { commands, type Divergence, type Ruling } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconCancel, IconConfirm, IconDataFixed, IconOpenInBrowser, IconUndo } from "../../lib/actionIcons";
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

export function DivergenceBadge({ divergence }: { divergence: Divergence }) {
  const describedBy = useId();
  const detail = `Step ${divergence.step}: the case says "${divergence.case_says}"; the app shows "${divergence.app_shows}" (${divergence.kind})`;
  const ruled = divergence.status === "ruled";
  return (
    <>
      <span
        title={detail}
        aria-describedby={describedBy}
        className={
          ruled
            ? "shrink-0 rounded-md border border-border px-2 py-0.5 text-xs font-medium text-muted"
            : "shrink-0 rounded-md border border-warning/50 px-2 py-0.5 text-xs font-medium text-warning"
        }
      >
        <span className="label-trim">{badgeText(divergence)}</span>
      </span>
      <span id={describedBy} className="sr-only">
        {detail}
      </span>
    </>
  );
}

const SENTENCE: Record<Ruling, (step: number) => string> = {
  case_stands: (step) => `The script keeps the case's value at step ${step}, and the case runs again.`,
  data_fixed: () => "You changed the environment so it matches the case; the case runs again.",
  rewrite: () => "Azure DevOps opens on the case. The case waits until its script is repaired to what you write.",
};

export function DivergenceRow({
  org,
  project,
  caseId,
  divergence,
}: {
  org: string;
  project: string;
  caseId: number;
  divergence: Divergence;
}) {
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

  // Takes a "Rewrite the case" ruling back: the divergence needs a ruling again.
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
  return (
    <div className="flex flex-wrap items-center gap-2 pt-1">
      <DivergenceBadge divergence={divergence} />
      <span className="min-w-0 flex-1 break-words text-xs text-text">
        Step {step}: the case says &quot;{divergence.case_says}&quot;; the app shows &quot;{divergence.app_shows}&quot;.
        {waiting && " Ask the assistant to repair the script (/tcm:heal)."}
      </span>
      {divergence.status === "open" && (
        <>
          <Button
            size="sm"
            variant="outline"
            className="shrink-0"
            aria-label={`Rule the case is right for #${caseId} step ${step}`}
            onClick={() => setAsking("case_stands")}
          >
            <IconConfirm aria-hidden />
            The case is right
          </Button>
          {divergence.kind === "data" && (
            <Button
              size="sm"
              variant="outline"
              className="shrink-0"
              aria-label={`Rule the data fixed for #${caseId} step ${step}`}
              onClick={() => setAsking("data_fixed")}
            >
              <IconDataFixed aria-hidden />
              Data fixed
            </Button>
          )}
          <Button
            size="sm"
            variant="outline"
            className="shrink-0"
            aria-label={`Rewrite case #${caseId} in Azure DevOps`}
            onClick={() => setAsking("rewrite")}
          >
            <IconOpenInBrowser aria-hidden />
            Rewrite the case
          </Button>
        </>
      )}
      {waiting && (
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
      )}
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
