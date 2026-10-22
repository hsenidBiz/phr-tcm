// Auto Run's assistant takes its scripting rules from the team's Playwright
// repo (the clone set on the Playwright tab). When no clone is set, or the
// setting no longer points at a folder, this says so once per app start -
// the assistant still works, on the rules shipped with this version.
//
// Rust answers whether a clone is set (`pw_rules_status`); asking it also
// refreshes the assistant's agent files, which is why the question is put
// every time Auto Run mounts and never served from the cache.

import { useQuery } from "@tanstack/react-query";
import { useEffect, useId, useState } from "react";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { IconCancel, IconModulePlaywright, IconModules } from "../../lib/actionIcons";
import { MODAL_SHORT } from "./modalWidths";

/** Set when the dialog first opens, so it appears at most once per app start. */
let shownThisStart = false;

/** Test seam: a new app start. */
export function resetPlaywrightNoticeForTests(): void {
  shownThisStart = false;
}

/** Asks pwRulesStatus each time Auto Run mounts (it also refreshes the agent files); open once per app start when clone_set is false. */
export function usePlaywrightNotice(): { open: boolean; gone: boolean; close: () => void } {
  const status = useQuery({
    queryKey: ["pw-rules-status"],
    queryFn: () => commands.pwRulesStatus(),
    staleTime: 0,
    refetchOnMount: "always",
    retry: false,
  });
  const [open, setOpen] = useState(false);
  // Only an answer fetched by this mount counts: the cached one may be from
  // before the person set (or cleared) the repo.
  const missing = status.isFetchedAfterMount && status.data?.clone_set === false;
  useEffect(() => {
    if (missing && !shownThisStart) {
      shownThisStart = true;
      setOpen(true);
    }
  }, [missing]);
  // A repo that was set and whose folder is gone is not the same as none ever set.
  return { open, gone: status.data?.clone_gone === true, close: () => setOpen(false) };
}

export default function PlaywrightNeededDialog({
  playwrightOn,
  gone = false,
  onOpenPlaywright,
  onOpenModules,
  onClose,
}: {
  playwrightOn: boolean;
  /** A Playwright repo is set but its folder is no longer there. */
  gone?: boolean;
  onOpenPlaywright: () => void;
  onOpenModules: () => void;
  onClose: () => void;
}) {
  const titleId = useId();
  return (
    <Modal onClose={onClose} labelledBy={titleId} className={`flex ${MODAL_SHORT} flex-col gap-3 p-5`}>
      <h2 id={titleId} className="text-sm font-semibold text-text">
        Playwright is needed
      </h2>
      <p className="text-xs text-muted">
        Auto Run&apos;s assistant takes its scripting rules from the team&apos;s Playwright repo.{" "}
        {gone
          ? "The Playwright repo set on the Playwright tab is no longer there"
          : "No Playwright repo is set on the Playwright tab"}
        , so the assistant is using the rules shipped with this version of Test Case Manager until one is set.
      </p>
      {!playwrightOn && (
        <p className="text-xs text-muted">
          The Playwright module is off: switch it on in Modules, then set the repo on the Playwright tab.
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
        {playwrightOn ? (
          <Button
            size="sm"
            onClick={() => {
              onClose();
              onOpenPlaywright();
            }}
          >
            <IconModulePlaywright aria-hidden />
            Open the Playwright tab
          </Button>
        ) : (
          <Button
            size="sm"
            onClick={() => {
              onClose();
              onOpenModules();
            }}
          >
            <IconModules aria-hidden />
            Open Modules
          </Button>
        )}
      </div>
    </Modal>
  );
}
