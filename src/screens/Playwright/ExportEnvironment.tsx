// Which environment the Playwright tab exports from. Export, Sync and
// Finish in the repo all run in one environment - the Playwright export
// environment - whichever is active: the owner tries scripts in a developer
// environment, and the automation environment's scripts are the final ones
// that reach the repo. This line says which it is, says when it is not the
// active one (and so what to do), and is the only place it is changed.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { commands, type EnvView } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { IconCancel, IconConfirm } from "../../lib/actionIcons";
import { envKeys, useEnvironments } from "../../lib/environments";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";
import { Hint } from "./parts";
import { SYNC_KEY } from "./Sync";

/** The query key of the export environment. */
export const EXPORT_ENV_KEY = "pw-export-env";

/** What Rust says before any environment exists. */
const NO_ENVIRONMENTS = "no environment has been set up yet";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function ExportEnvironment() {
  const qc = useQueryClient();
  const titleId = useId();
  const envs = useEnvironments();
  // Asked once the list is known: reading the list creates the first
  // environment, which is what the export environment is then named from.
  const current = useQuery({
    queryKey: [EXPORT_ENV_KEY],
    queryFn: () => unwrapStr(commands.pwExportEnv()),
    enabled: envs.isSuccess,
    retry: false,
  });
  const [asking, setAsking] = useState<EnvView | null>(null);
  const [busy, setBusy] = useState(false);

  const others = (envs.data?.environments ?? []).filter((e) => e.id !== current.data?.id);

  const confirm = async () => {
    if (!asking) return;
    setBusy(true);
    try {
      const now = await unwrapStr(commands.pwSetExportEnv(asking.id));
      qc.setQueryData([EXPORT_ENV_KEY], now);
      // The list names the export environment too, and Sync's states were
      // compared for the old one.
      void qc.invalidateQueries({ queryKey: envKeys.list });
      void qc.invalidateQueries({ queryKey: [SYNC_KEY] });
      setAsking(null);
    } catch (e) {
      toast.error(message(e));
    } finally {
      setBusy(false);
    }
  };

  if (current.isError) {
    const why = message(current.error);
    return (
      <Hint>
        {why === NO_ENVIRONMENTS
          ? "No environment yet - exports come from one once it exists."
          : why}
      </Hint>
    );
  }
  const exportEnv = current.data;
  if (!exportEnv) return null;

  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1" role="group" aria-label="Export environment">
      <span className="rounded-full bg-accent/15 px-1.5 py-0.5 text-[10px] font-medium leading-none text-accent-fill">
        <span className="label-trim">{`Exports from ${exportEnv.name}`}</span>
      </span>
      {!exportEnv.is_active && (
        <Hint>{`It is not the active environment. Switch to ${exportEnv.name} on the AI Bridge tab to export or sync.`}</Hint>
      )}
      {others.length > 0 && (
        <Select
          aria-label="Change export environment"
          className="w-40"
          value=""
          onChange={(e) => setAsking(others.find((o) => o.id === e.target.value) ?? null)}
        >
          <option value="">Change…</option>
          {others.map((o) => (
            <option key={o.id} value={o.id}>
              {o.name}
            </option>
          ))}
        </Select>
      )}
      {asking && (
        <Modal onClose={() => !busy && setAsking(null)} labelledBy={titleId} className="flex w-full max-w-md flex-col gap-3 p-5">
          <h2 id={titleId} className="text-sm font-semibold text-text">
            Change the export environment
          </h2>
          <p className="text-sm text-text">
            {`Make ${asking.name} the Playwright export environment? The next Sync compares ${asking.name}'s scripts with the repo, so cases that differ show as out of date. Nothing is written to the repo until you sync.`}
          </p>
          <div className="flex justify-end gap-2">
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => setAsking(null)}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={busy} onClick={() => void confirm()}>
              <IconConfirm aria-hidden />
              Make export environment
            </Button>
          </div>
        </Modal>
      )}
    </div>
  );
}
