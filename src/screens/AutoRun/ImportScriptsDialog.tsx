// Bring the Auto Run scripts in a folder into the environment on screen.
// The plan comes first - how many are new, how many are already the same,
// how many cases already have a different script here - and nothing is
// written until Import. A case's existing script is only replaced when the
// person ticks Replace, and the one it replaces is kept as a deleted script.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Modal } from "../../components/ui/modal";
import { useCloseOnEnvironmentChange } from "../../hooks/useCloseOnEnvironmentChange";
import { IconCancel, IconImport } from "../../lib/actionIcons";
import { copyRefusalText } from "../../lib/copyRefusal";
import { isEnvironmentScopedKey } from "../../lib/environments";
import { plural } from "../../lib/plural";
import { toast } from "../../lib/toast";

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export default function ImportScriptsDialog({
  folder,
  envName,
  onClose,
}: {
  /** The folder the person picked. Only read. */
  folder: string;
  /** The environment the scripts land in, as it was named when the folder was picked. */
  envName: string;
  onClose: () => void;
}) {
  // The question names the environment as it was: it never outlives a switch.
  useCloseOnEnvironmentChange(onClose);
  const qc = useQueryClient();
  const titleId = useId();
  const [replace, setReplace] = useState(false);
  const [problem, setProblem] = useState("");
  const [busy, setBusy] = useState(false);

  const plan = useQuery({
    queryKey: ["autorun-import-folder-plan", folder],
    queryFn: async () => {
      const r = await commands.autoRunImportFolderPlan(folder);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    retry: false,
    gcTime: 0,
    staleTime: 0,
  });

  const run = async () => {
    setProblem("");
    setBusy(true);
    try {
      const r = await commands.autoRunImportFolder(folder, replace);
      if (r.status === "error") {
        setProblem(copyRefusalText(r.error));
        return;
      }
      const replaced = replace ? r.data.differ.length : 0;
      // The scripts on disk are the active environment's: read them again.
      await qc.invalidateQueries({ predicate: (q) => isEnvironmentScopedKey(q.queryKey) });
      toast.success(
        `Imported ${plural(r.data.new.length + replaced, "script")} into ${envName}` +
          (replaced > 0 ? `, replaced ${replaced}` : ""),
      );
      onClose();
    } catch (e) {
      setProblem(copyRefusalText(message(e)));
    } finally {
      setBusy(false);
    }
  };

  const p = plan.data;
  const k = p?.differ.length ?? 0;
  return (
    <Modal onClose={onClose} labelledBy={titleId} className="flex w-full max-w-md flex-col gap-3 p-5">
      <h2 id={titleId} className="text-sm font-semibold text-text">
        {`Import scripts into ${envName}`}
      </h2>
      <div aria-live="polite" className="space-y-2 text-xs text-text">
        {plan.isPending && <p className="text-muted">Reading the folder…</p>}
        {plan.isError && (
          <p role="alert" className="text-danger">
            {copyRefusalText(plan.error.message)}
          </p>
        )}
        {p && (
          <>
            <p>{`${plural(p.new.length, "new script")} will be added.`}</p>
            <p>{`${p.same.length} ${p.same.length === 1 ? "is" : "are"} already the same.`}</p>
            {k > 0 && (
              <p>
                {`${plural(k, "case")} already ${k === 1 ? "has" : "have"} a different script here and ${k === 1 ? "is" : "are"} left as ${k === 1 ? "it is" : "they are"}.`}
              </p>
            )}
            {p.unreadable > 0 && (
              <p className="text-muted">{`${plural(p.unreadable, "file")} in the folder could not be used as a script.`}</p>
            )}
            {k > 0 && (
              <label className="flex items-start gap-2 text-text">
                <Checkbox checked={replace} onCheckedChange={setReplace} ariaLabel={`Replace those ${k} scripts`} />
                {`Replace those ${k} scripts (the ones here are kept as deleted scripts)`}
              </label>
            )}
          </>
        )}
      </div>
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
        {p && (
          <Button size="sm" disabled={busy} onClick={run}>
            <IconImport aria-hidden />
            Import
          </Button>
        )}
      </div>
    </Modal>
  );
}
