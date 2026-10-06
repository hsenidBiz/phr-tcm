// The script editor's Setup section: the fixture a script builds its data
// from, shown as it would run, with the approval that lets it. Only a person
// approves here - no tool can write an approval - and Approve setup signs
// exactly what is on screen: the view's fingerprint goes with it, so a setup
// that changed since is refused rather than approved unseen.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconConfirm, IconUndo } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { dayMonth, stampDate } from "../ApiTemplates/TemplateRow";
import { CHANGED_WHILE_LOOKING, loadSetupView, setupViewKey } from "./setupApproval";

export default function SetupSection({
  org,
  project,
  caseId,
}: {
  org: string;
  project: string;
  caseId: number;
}) {
  const qc = useQueryClient();
  const key = setupViewKey(org, project, caseId);
  const view = useQuery({
    queryKey: key,
    queryFn: () => loadSetupView(org, project, caseId),
    enabled: Boolean(org && project),
    retry: false,
  });
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState("");

  const v = view.data;
  // A script with no setup has no section at all.
  if (!v) {
    return view.isError ? <p className="text-xs text-danger">{view.error.message}</p> : null;
  }

  /** Runs one approval command and takes the view it answers; a refusal is
   * shown, and a setup that changed under the person is read again. */
  const run = async (work: () => Promise<typeof v>) => {
    setProblem("");
    setBusy(true);
    try {
      qc.setQueryData(key, await work());
    } catch (e) {
      const sentence = e instanceof Error ? e.message : String(e);
      setProblem(sentence);
      if (sentence.includes(CHANGED_WHILE_LOOKING)) await qc.invalidateQueries({ queryKey: key });
    } finally {
      setBusy(false);
    }
  };

  const approve = () =>
    run(async () => unwrapStr(commands.autoRunApproveSetup(org, project, caseId, v.fingerprint)));
  const withdraw = () => run(async () => unwrapStr(commands.autoRunWithdrawSetup(org, project, caseId)));

  const when = v.approved_at ? stampDate(v.approved_at) : null;
  const approvedOn = when ? dayMonth(when) : (v.approved_at ?? "");

  return (
    <section aria-label="Setup" className="space-y-1">
      <span className="text-xs font-medium text-muted">Setup</span>
      <dl className="grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-xs">
        <dt className="text-muted">Fixture</dt>
        <dd className="break-words text-text">{v.fixture_name}</dd>
        <dt className="text-muted">Runs as</dt>
        <dd className="break-words text-text">{v.account}</dd>
      </dl>
      {v.steps.length > 0 && (
        <ol aria-label="Setup steps" className="space-y-0.5 text-xs">
          {v.steps.map((line, i) => (
            <li key={`${i}-${line}`} className="id-mono break-words text-text">
              {line}
            </li>
          ))}
        </ol>
      )}
      {v.creates.length > 0 && (
        <p className="text-xs text-text">
          <span className="text-muted">Creates </span>
          {v.creates.join(", ")}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-2 pt-1">
        {v.approval === "approved" ? (
          <>
            <span className="text-xs text-success">{`Approved ${approvedOn}`}</span>
            <Button size="sm" variant="ghost" disabled={busy} onClick={withdraw}>
              <IconUndo aria-hidden />
              Withdraw approval
            </Button>
          </>
        ) : (
          <>
            {v.approval === "changed" && (
              <span className="text-xs text-warning">Changed since you approved it</span>
            )}
            <Button size="sm" variant="outline" disabled={busy} onClick={approve}>
              <IconConfirm aria-hidden />
              Approve setup
            </Button>
          </>
        )}
      </div>
      {problem && (
        <p role="alert" className="text-xs text-danger">
          {problem}
        </p>
      )}
    </section>
  );
}
