// Copy one case's script between environments, in either direction: "to"
// pushes it from the environment on screen into another, "from" pulls it
// from another into the one on screen. Choosing the other environment shows
// the dry run first - what comes across, what the destination keeps its own
// version of, what it lacks - and nothing is written until the person
// confirms. A destination that already has a script for the case is only
// replaced by the button that says so.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type CaseCopyPlan } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { useCloseOnEnvironmentChange } from "../../hooks/useCloseOnEnvironmentChange";
import { IconCancel, IconCopy } from "../../lib/actionIcons";
import { isEnvironmentScopedKey, useEnvironments } from "../../lib/environments";
import { toast } from "../../lib/toast";

function words(kind: string, names: string[]): string {
  return names.map((n) => `${kind} ${n}`).join(", ");
}

function list(parts: string[]): string {
  return parts.filter(Boolean).join(", ");
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** What a copy brings, in the order the confirmation lists it. */
function Lines({ plan, dest }: { plan: CaseCopyPlan; dest: string }) {
  const brings = list([
    words("component", plan.components_added),
    words("area", plan.areas_added),
    plan.recipe_added ? "the sign-in recording" : "",
  ]);
  const keeps = list([words("component", plan.components_kept), words("area", plan.areas_kept)]);
  return (
    <div className="space-y-1 text-xs text-text">
      {brings && <p>{`Brings: ${brings}.`}</p>}
      {plan.recipe_added && plan.recipe_address && (
        <p>
          {`The sign-in recording brings its site address, ${plan.recipe_address}. A run in ${dest} with no address of its own signs in there.`}
        </p>
      )}
      {keeps && <p>{`Uses ${dest}'s own: ${keeps}.`}</p>}
      {plan.missing_accounts.map((key) => (
        <p key={key}>{`Add the account ${key} in ${dest}'s Accounts before running.`}</p>
      ))}
      {plan.missing_components.map((name) => (
        <p key={name}>{`The component ${name} is in neither environment: the case will fail until it is added.`}</p>
      ))}
      {plan.missing_api.length > 0 && (
        <p>{`${dest} lacks the API data this case needs: ${plan.missing_api.join(", ")}.`}</p>
      )}
      {plan.uses_default_area && (
        <p>{`This case uses its module's default area: check ${dest} has it.`}</p>
      )}
    </div>
  );
}

export default function CopyCaseDialog({
  caseId,
  org,
  project,
  direction = "to",
  onClose,
}: {
  caseId: number;
  org: string;
  project: string;
  /** "to": from the environment on screen into another. "from": the other way. */
  direction?: "to" | "from";
  onClose: () => void;
}) {
  // The question names the environments as they were: it never outlives a switch.
  useCloseOnEnvironmentChange(onClose);
  const qc = useQueryClient();
  const envs = useEnvironments();
  const active = envs.data?.active ?? "";
  const activeEnv = envs.data?.environments.find((e) => e.id === active);
  const others = (envs.data?.environments ?? []).filter((e) => e.id !== active);
  const [pickedId, setPickedId] = useState("");
  const other = others.find((e) => e.id === pickedId);
  // The side the script lands in.
  const dest = direction === "to" ? other?.name : activeEnv?.name;
  const [problem, setProblem] = useState("");
  const [busy, setBusy] = useState(false);

  const plan = useQuery({
    queryKey: ["autorun-copy-case-plan", direction, org, project, caseId, active, pickedId],
    queryFn: async () => {
      const r =
        direction === "to"
          ? await commands.autoRunCopyCasePlan(org, project, caseId, active, pickedId)
          : await commands.autoRunCopyCaseFromPlan(org, project, caseId, active, pickedId);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    enabled: Boolean(other && active),
    retry: false,
    gcTime: 0,
    staleTime: 0,
  });

  const copy = async () => {
    if (!other || !plan.data) return;
    setProblem("");
    setBusy(true);
    try {
      const replace = plan.data.target_has_script;
      const r =
        direction === "to"
          ? await commands.autoRunCopyCase(org, project, caseId, active, pickedId, replace)
          : await commands.autoRunCopyCaseFrom(org, project, caseId, active, pickedId, replace);
      if (r.status === "error") {
        setProblem(r.error);
        return;
      }
      if (direction === "from") {
        // The script now on disk is the active environment's: read again.
        await qc.invalidateQueries({ predicate: (q) => isEnvironmentScopedKey(q.queryKey) });
      }
      toast.success(`Copied case #${caseId} into ${dest}`);
      onClose();
    } catch (e) {
      setProblem(message(e));
    } finally {
      setBusy(false);
    }
  };

  const label = direction === "to" ? "Copy to" : "Copy from";
  const title =
    direction === "to"
      ? `Copy case #${caseId} to another environment`
      : `Copy case #${caseId} from another environment`;

  return (
    <Modal onClose={onClose} labelledBy="copy-case-title" className="flex w-full max-w-md flex-col gap-3 p-5">
      <h2 id="copy-case-title" className="text-sm font-semibold text-text">
        {title}
      </h2>
      <label className="block space-y-1">
        <span className="text-xs font-medium text-muted">{label}</span>
        <Select
          aria-label={label}
          className="w-full"
          value={pickedId}
          onChange={(e) => {
            setProblem("");
            setPickedId(e.target.value);
          }}
        >
          <option value="">Choose an environment…</option>
          {others.map((e) => (
            <option key={e.id} value={e.id}>
              {e.name}
            </option>
          ))}
        </Select>
      </label>
      {other && plan.isPending && <p className="text-xs text-muted">Checking what the copy needs…</p>}
      {other && plan.isError && (
        <p role="alert" className="text-xs text-danger">
          {plan.error.message}
        </p>
      )}
      {other && plan.data && dest && (
        <>
          <Lines plan={plan.data} dest={dest} />
          {plan.data.target_has_script && (
            <p className="text-xs font-medium text-text">
              {`${dest} already has a script for this case. Copying replaces it.`}
            </p>
          )}
        </>
      )}
      {problem && (
        <p role="alert" className="text-xs text-danger">
          {problem}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        {other && plan.data && dest && (
          <Button size="sm" disabled={busy} onClick={copy}>
            <IconCopy aria-hidden />
            {plan.data.target_has_script ? `Replace ${dest}'s script` : "Copy"}
          </Button>
        )}
      </div>
    </Modal>
  );
}
