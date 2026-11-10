// The sign-in the assistant proposed, under the Sign-in row of Setup: what
// it would save, which parts differ from what is saved now, and Approve or
// Reject. Nothing is shown when there is no proposal.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { envKeys } from "../../lib/environments";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";
import { IconClear, IconConfirm } from "../../lib/actionIcons";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

// The one failure after which the sign-in IS saved (and the proposal gone).
const SAVED_BUT_SWITCH_STAYED_OFF = "the sign-in was saved, but Reset expired passwords could not be turned on";

export default function SignInProposalNotice({ org, project }: { org: string; project: string }) {
  const qc = useQueryClient();
  const proposalKey = ["autorun-sign-in-proposal", org, project];
  const proposal = useQuery({
    queryKey: proposalKey,
    queryFn: () => unwrapStr(commands.autoRunSignInProposal(org, project)),
  });
  const [turnOnReset, setTurnOnReset] = useState(false);
  const [busy, setBusy] = useState(false);

  if (proposal.isError) return <p className="text-xs text-danger">The assistant's proposed sign-in could not be read</p>;
  const p = proposal.data;
  if (!p) return null;

  const mark = (part: string) =>
    p.changes.includes(part) ? (
      <Badge data-testid={`changes-${part.replace(/\s+/g, "-")}`} className="ml-2 shrink-0 bg-accent/15 text-accent">
        changes
      </Badge>
    ) : null;

  const settle = async (switchChanged: boolean) => {
    await qc.invalidateQueries({ queryKey: proposalKey });
    await qc.invalidateQueries({ queryKey: ["autorun-recipe", org, project] });
    if (switchChanged) await qc.invalidateQueries({ queryKey: envKeys.list });
  };

  const approve = async () => {
    setBusy(true);
    try {
      await unwrapStr(commands.autoRunApproveSignInProposal(org, project, turnOnReset));
      toast.success("Sign-in recipe saved.");
      await settle(turnOnReset);
      setTurnOnReset(false);
    } catch (e) {
      const said = message(e);
      toast.error(said);
      // Saved all the same: show what is saved now, not the proposal.
      if (said.startsWith(SAVED_BUT_SWITCH_STAYED_OFF)) {
        await settle(true);
        setTurnOnReset(false);
      }
    } finally {
      setBusy(false);
    }
  };

  const reject = async () => {
    setBusy(true);
    try {
      await unwrapStr(commands.autoRunRejectSignInProposal(org, project));
      await settle(false);
      setTurnOnReset(false);
    } catch (e) {
      toast.error(message(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-label="Proposed sign-in" className="space-y-2 rounded-md border border-border bg-surface px-3 py-2">
      <h3 className="text-xs font-semibold text-text">Proposed by the assistant</h3>
      <dl className="space-y-1 text-xs">
        <div className="flex items-baseline">
          <dt className="w-28 shrink-0 text-muted">Address</dt>
          <dd className="text-text">
            {p.address}
            {mark("address")}
          </dd>
        </div>
        <div className="flex items-baseline">
          <dt className="w-28 shrink-0 text-muted">
            Steps
            {mark("steps")}
          </dt>
          <dd className="min-w-0 text-text">
            <ol className="list-decimal space-y-0.5 pl-4">
              {p.steps.map((s, i) => (
                <li key={i}>{s}</li>
              ))}
            </ol>
          </dd>
        </div>
        <div className="flex items-baseline">
          <dt className="w-28 shrink-0 text-muted">Signed-in check</dt>
          <dd className="text-text">
            {p.signed_in}
            {p.signed_in_in_menu ? " (inside a menu)" : ""}
            {mark("signed-in check")}
          </dd>
        </div>
        {p.password_reset && (
          <div className="flex items-baseline">
            <dt className="w-28 shrink-0 text-muted">Change password</dt>
            <dd className="text-text">
              {p.password_reset}
              {mark("change-password part")}
            </dd>
          </div>
        )}
      </dl>
      <p className="text-xs text-muted">Tried with {p.account}</p>
      {p.offer_reset_switch && (
        <label className="flex items-center gap-2 text-xs text-text">
          <Checkbox checked={turnOnReset} disabled={busy} ariaLabel="Also turn on Reset expired passwords" onCheckedChange={setTurnOnReset} />
          Also turn on Reset expired passwords
        </label>
      )}
      <div className="flex gap-2">
        <Button size="sm" variant="outline" disabled={busy} onClick={() => void approve()}>
          <IconConfirm aria-hidden />
          Approve
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void reject()}>
          <IconClear aria-hidden />
          Reject
        </Button>
      </div>
    </section>
  );
}
