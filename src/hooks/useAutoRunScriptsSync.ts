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

export function useAutoRunScriptsSync(): void {
  const qc = useQueryClient();
  useEffect(() => {
    const un = events.autorunScriptsChanged.listen((e) => {
      const ids = e.payload.case_ids;
      // No case named: every script is read again rather than none.
      if (ids.length === 0) {
        void qc.invalidateQueries({ queryKey: ["autorun-script"] });
        return;
      }
      for (const id of ids) void qc.invalidateQueries({ queryKey: scriptQueryKey(id) });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [qc]);
}
