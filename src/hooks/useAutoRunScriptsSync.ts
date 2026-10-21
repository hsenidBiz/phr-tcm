// Auto Run's scripts change outside the screen: the assistant saves or
// repairs one through the AI bridge, a run clears a mark, a delete moves one
// aside. Rust says so after every write (`autorun_scripts_changed`), and the
// changed cases' script queries are read again, so the Auto Run screen and
// any editor open on one show it without a reload. Used once, by App, where
// something is always mounted.

import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { events } from "../bindings";

/** The script query of a case, as every Auto Run view keys it. */
export const scriptQueryKey = (caseId: number) => ["autorun-script", caseId] as const;

/** The first part of a case's setup view key (`setupViewKey`): the setup
 * is read from the script, so it is read again with it. */
const SETUP_VIEW = "autorun-setup";

export function useAutoRunScriptsSync(): void {
  const qc = useQueryClient();
  useEffect(() => {
    const un = events.autorunScriptsChanged.listen((e) => {
      // A save closes a divergence awaiting its repair; the card shows it.
      void qc.invalidateQueries({ queryKey: ["autorun-divergences"] });
      const ids = e.payload.case_ids;
      // No case named: every script is read again rather than none.
      if (ids.length === 0) {
        void qc.invalidateQueries({ queryKey: ["autorun-script"] });
        void qc.invalidateQueries({ queryKey: [SETUP_VIEW] });
        return;
      }
      for (const id of ids) void qc.invalidateQueries({ queryKey: scriptQueryKey(id) });
      // Keyed [SETUP_VIEW, org, project, caseId], for any org and project.
      void qc.invalidateQueries({
        predicate: (q) => q.queryKey[0] === SETUP_VIEW && ids.includes(q.queryKey[3] as number),
      });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [qc]);
}
