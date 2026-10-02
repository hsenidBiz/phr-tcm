// A case's "suspected application defect" mark, as it shows on the case row.
//
// An assistant sets the mark when a script is right and the application did
// not do what the case expects (see `store::set_suspected_defect`). The row
// only reports it: a person who has looked into it can clear it, after an
// inline confirm. Clearing touches nothing but the mark - the script's steps
// and repairs stay as they are, and nothing reaches Azure DevOps.

import { useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands, type SuspectedDefect } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconCancel, IconClearDefect, IconClear } from "../../lib/actionIcons";

/** The badge, and its Clear button. The caller renders `ClearConfirm` on a
 * row of its own when `confirming` is set, so the confirm gets the row's
 * full width instead of squeezing between the title and the buttons. */
export function SuspectedDefectBadge({
  caseId,
  defect,
  onClear,
}: {
  caseId: number;
  defect: SuspectedDefect;
  onClear: () => void;
}) {
  const describedBy = useId();
  const detail = `Suspected application defect at step ${defect.step_number}: ${defect.note}`;
  return (
    <>
      <span
        title={detail}
        aria-describedby={describedBy}
        className="shrink-0 rounded-md border border-warning/50 px-2 py-0.5 text-xs font-medium text-warning"
      >
        Suspected defect
      </span>
      <span id={describedBy} className="sr-only">
        {detail}
      </span>
      <Button
        size="sm"
        variant="outline"
        className="shrink-0"
        aria-label={`Clear suspected defect for #${caseId}`}
        onClick={onClear}
      >
        <IconClearDefect aria-hidden />
        Clear
      </Button>
    </>
  );
}

/** Keep / Clear, with the reason a Clear failed. Confirming calls the
 * command, then refreshes the script the row reads so the badge goes. */
export function ClearConfirm({ caseId, onDone }: { caseId: number; onDone: () => void }) {
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);

  const clear = async () => {
    setBusy(true);
    setProblem(null);
    try {
      const res = await commands.autoRunClearSuspectedDefect(caseId);
      if (res.status === "error") {
        setProblem(res.error);
        return;
      }
      await queryClient.invalidateQueries({ queryKey: ["autorun-script", caseId] });
      onDone();
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex w-full flex-wrap items-center gap-2 border-t border-border/60 pt-2">
      <span className="min-w-0 flex-1 text-xs text-text">
        Clear the suspected defect on #{caseId}? The script itself is not changed.
      </span>
      <Button size="sm" variant="ghost" onClick={onDone}>
        <IconCancel aria-hidden />
        Keep
      </Button>
      <Button size="sm" variant="outline" disabled={busy} onClick={() => void clear()}>
        <IconClear aria-hidden />
        Clear
      </Button>
      {problem && (
        <p role="status" className="w-full break-words text-xs text-danger">
          {problem}
        </p>
      )}
    </div>
  );
}
