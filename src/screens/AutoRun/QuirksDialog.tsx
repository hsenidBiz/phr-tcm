// The project's Known quirks: notes about how the application behaves,
// that the next script or API template should not have to rediscover.
// Opened from Auto Test's Setup card; the list itself is `QuirksList`,
// which saves every change the moment it is made. A note typed and not
// saved yet is not thrown away by a stray click or Escape: closing asks
// first.

import { useCloseOnEnvironmentChange } from "../../hooks/useCloseOnEnvironmentChange";
import { useQuery } from "@tanstack/react-query";
import { useCallback, useId, useState } from "react";
import { commands, type Quirk_Serialize } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { Term } from "../../components/Term";
import { IconCancel, IconRemove } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { plural } from "../../lib/plural";
import { MODAL_LARGE } from "./modalWidths";
import QuirksList from "./QuirksList";

/** The query key the Setup row, this dialog and the list share. */
export const quirksKey = (org: string, project: string) => ["autorun-quirks", org, project];

/** The project's quirks. A plain local file read through Rust. */
export function useQuirks(org: string, project: string) {
  return useQuery({
    queryKey: quirksKey(org, project),
    queryFn: () => unwrapStr(commands.autoRunLoadQuirks(org, project)),
    enabled: Boolean(org && project),
    retry: false,
  });
}

/** The Setup summary's line and how many notes are live. `null` until the
 * file has answered. Retired notes are not counted as notes. */
export function quirksSummary(list: Quirk_Serialize[] | undefined): { line: string; active: number } | null {
  if (!list) return null;
  const retired = list.filter((q) => q.status === "retired").length;
  const active = list.length - retired;
  const notes = active === 0 ? "None yet" : plural(active, "note");
  const line = retired === 0 ? notes : active === 0 ? `${retired} retired` : `${notes}, ${retired} retired`;
  return { line, active };
}

export default function QuirksDialog({
  org,
  project,
  onClose,
}: {
  org: string;
  project: string;
  onClose: () => void;
}) {
  // What it holds is this environment's: it never outlives a switch.
  useCloseOnEnvironmentChange(onClose);
  const headingId = useId();
  const quirks = useQuirks(org, project);
  /** A note typed into the list and not saved yet. */
  const [pending, setPending] = useState(false);
  const [asking, setAsking] = useState(false);
  const onPendingChange = useCallback((p: boolean) => setPending(p), []);

  // Every way out (Close, Escape, a click outside) comes here.
  const requestClose = () => (pending ? setAsking(true) : onClose());

  return (
    <Modal onClose={requestClose} labelledBy={headingId} className={`${MODAL_LARGE} flex flex-col gap-3 overflow-y-auto p-5`}>
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Known <Term name="quirk">quirks</Term>
      </h2>
      <p className="text-xs text-muted">
        Things learned about this application that the next script or API template - written by a person or an
        assistant - should not have to rediscover. A note an assistant filed with a repair shows what the runs since
        have said about it. Retire a note that no longer helps; it can be restored. Each change is saved as you make
        it.
      </p>
      {quirks.isError && <p className="text-xs text-danger">{quirks.error.message}</p>}
      {quirks.isSuccess && (
        <QuirksList org={org} project={project} quirks={quirks.data} onPendingChange={onPendingChange} />
      )}

      {asking && (
        <div
          role="group"
          aria-label="Close without saving?"
          className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-surface px-3 py-2"
        >
          <span className="min-w-0 flex-1 text-xs text-text">
            A note you typed has not been saved. Close and lose it?
          </span>
          <Button size="sm" variant="ghost" onClick={() => setAsking(false)}>
            <IconCancel aria-hidden />
            Keep editing
          </Button>
          <Button size="sm" variant="danger" onClick={onClose}>
            <IconRemove aria-hidden />
            Discard and close
          </Button>
        </div>
      )}

      <div className="flex justify-end border-t border-border pt-3">
        <Button size="sm" variant="ghost" onClick={requestClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}
