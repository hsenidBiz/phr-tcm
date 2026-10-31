// Rust says `autorun-environment-changed` after a switch, a copy into the
// active environment and an import. Everything the webview read for the
// environment that was active is then forgotten and read again, and the
// list of environments is read afresh. Used once, by App, beside
// `useAutoRunDataSync`.

import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { events } from "../bindings";
import { envKeys, forgetEnvironmentData } from "../lib/environments";

export function useEnvironmentChangeSync(): void {
  const qc = useQueryClient();
  useEffect(() => {
    const un = events.autorunEnvironmentChanged.listen(() => {
      void qc.invalidateQueries({ queryKey: envKeys.list });
      void forgetEnvironmentData(qc);
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [qc]);
}
