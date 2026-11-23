import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useSyncExternalStore } from "react";
import { commands } from "../bindings";
import { selectedDbSnapshot, subscribeDbSettings } from "../lib/dbServer";
import { recheckDb, useDbWriteVerdicts, verdictOf } from "../lib/dbWriteCheck";
import { useModule } from "../lib/modules";

/**
 * Asks the server about the chosen database's login once, when there is no
 * answer for it yet. Mounted with the bridge sync at App level, so an update
 * or a switch to an unchecked database is checked without a visit to the AI
 * Bridge tab - writes stay off until a verdict exists.
 *
 * Once per login (id, server, database, user) while the app runs: a failed
 * check waits for Recheck rather than looping. Never while signed out or with
 * Database Access off.
 */
export function useAutoDbCheck(signedIn: boolean) {
  const qc = useQueryClient();
  const dbOn = useModule("db_access");
  const on = signedIn && dbOn;
  const dbId = useSyncExternalStore(subscribeDbSettings, selectedDbSnapshot);
  // The same query the AI Bridge tab reads: who each database signs in as
  // and whether a password is saved - never the password.
  const databases = useQuery({
    queryKey: ["db-databases"],
    queryFn: async () => (await commands.dbDatabases()) ?? [],
    enabled: on,
  });
  const verdicts = useDbWriteVerdicts(on);
  const selectedDb = databases.data?.find((d) => d.id === dbId) ?? null;
  const hasVerdict = selectedDb ? verdictOf(verdicts.data, selectedDb.id) !== null : false;
  const asked = useRef(new Set<string>());
  useEffect(() => {
    if (!on || !selectedDb || !verdicts.isSuccess || hasVerdict) return;
    if (!selectedDb.server || !selectedDb.user || !selectedDb.has_password) return;
    const login = `${selectedDb.id}|${selectedDb.server}|${selectedDb.database}|${selectedDb.user}`;
    if (asked.current.has(login)) return;
    asked.current.add(login);
    void recheckDb(qc, selectedDb.id);
  }, [on, selectedDb, verdicts.isSuccess, hasVerdict, qc]);
}
