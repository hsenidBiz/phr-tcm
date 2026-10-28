// What an assistant adds through the AI bridge - areas, components, the
// discovery map and its summary, quirks, test files, proposed accounts,
// fixtures - changes outside the screen. Rust says so after every write
// (`autorun-data-changed`, and `api-templates-changed` for fixtures), and the
// matching queries are read again, so the Auto Run screen shows it without a
// reload. The discovery map writes once per page read, so the events are
// gathered and answered once per burst. Used once, by App, where something
// is always mounted.

import { useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { events, type AutorunDataKind } from "../bindings";
import { envKeys } from "../lib/environments";
import { FIXTURES_KEY } from "../screens/ApiTemplates/FixturesTab";

/** How long a burst of events is gathered before the queries are read. */
const FLUSH_MS = 300;

/** The query keys each kind of data is read under, by prefix, so any
 * organization and project is reached. */
export const DATA_KIND_KEYS: Record<AutorunDataKind, readonly unknown[]> = {
  areas: ["autorun-nav"],
  components: ["autorun-components"],
  map: ["autorun-map"],
  mappingSummary: ["autorun-mapping-summary"],
  quirks: ["autorun-quirks"],
  testFiles: ["test-files"],
  proposals: envKeys.proposals,
};

/** One array, not one per event: `pending` compares keys by reference, so a
 * burst of fixture events collapses to a single read. */
const FIXTURES_QUERY_KEY: readonly unknown[] = [FIXTURES_KEY];

export function useAutoRunDataSync(): void {
  const qc = useQueryClient();
  useEffect(() => {
    const pending = new Set<readonly unknown[]>();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const want = (key: readonly unknown[]) => {
      pending.add(key);
      if (timer === undefined) {
        timer = setTimeout(() => {
          timer = undefined;
          const keys = [...pending];
          pending.clear();
          for (const queryKey of keys) void qc.invalidateQueries({ queryKey });
        }, FLUSH_MS);
      }
    };
    const unsubs = [
      events.autorunDataChanged.listen((e) => {
        for (const kind of e.payload.kinds) want(DATA_KIND_KEYS[kind]);
      }),
      events.apiTemplatesChanged.listen(() => want(FIXTURES_QUERY_KEY)),
    ];
    return () => {
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
      pending.clear();
      for (const un of unsubs) un.then((f) => f()).catch(() => {});
    };
  }, [qc]);
}
