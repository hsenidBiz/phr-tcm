// Keeps the Playwright header's specs pill true: looks at the clone's spec
// files when the screen opens, when the window regains focus and every minute
// while the screen is shown, and reads the clone's data again when they
// changed. The look (`pw_repo_specs_status`) is a walk of file metadata - it
// starts no process - so the timer costs nothing when nothing changed.
//
// The screen is only mounted while the Playwright tab is shown, so its
// effects ARE "while shown". What was last read again is kept on the query
// client (not here), so coming back to the tab after the repo changed reads
// the clone again instead of trusting what the last visit loaded.

import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState } from "react";
import { commands } from "../../bindings";
import { unwrapStr } from "../../lib/ipc";
import { SPECS_CHECK_MS, setSpecsPill, specsPill, type SpecsView } from "../../lib/pwSpecs";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The query-client key of the fingerprint a clone was last read again at. */
const processedKey = (clone: string) => ["pw-specs-processed", clone];

type State = SpecsView & { clone: string };
const empty = (clone: string): State => ({ clone, walk: null, processed: null, processing: false, failed: null });

export function useSpecsWatch({
  clone,
  ready,
  busy,
  process,
}: {
  /** The clone folder; empty for none. */
  clone: string;
  /** The settings that name the clone have been read. */
  ready: boolean;
  /** Something is writing or reading the clone: a change waits for it. */
  busy: boolean;
  /** Reads the clone's data again; rejects when that failed. */
  process: () => Promise<void>;
}): { reprocess: () => Promise<void> } {
  const qc = useQueryClient();
  const [state, setState] = useState<State>(() => empty(clone));
  const view = state.clone === clone ? state : empty(clone);

  const latest = useRef({ clone, busy, process });
  latest.current = { clone, busy, process };
  const inFlight = useRef(false);
  /** The pipeline rule files' fingerprint at the last look. */
  const rulesSeen = useRef<{ clone: string; fingerprint: string } | null>(null);
  /** The fingerprint a re-read failed at: the timer does not try it again. */
  const failedAt = useRef<{ clone: string; fingerprint: string } | null>(null);

  const patch = useCallback((forClone: string, p: Partial<SpecsView>) => {
    setState((s) => ({ ...(s.clone === forClone ? s : empty(forClone)), ...p }));
  }, []);

  /** One look at the clone, and the re-read when it changed. `force` is the
   * Refresh button: it reads again whatever the look says. */
  const look = useCallback(
    async (force: boolean) => {
      const forClone = latest.current.clone;
      if (!forClone) return;
      let print: string | null = null;
      try {
        const got = await unwrapStr(commands.pwRepoSpecsStatus(forClone));
        if (!got || typeof got.fingerprint !== "string") throw new Error("The spec files could not be read.");
        if (latest.current.clone !== forClone) return;
        print = got.fingerprint;
        patch(forClone, { walk: { count: got.count, fingerprint: got.fingerprint, pipeline: got.pipeline } });
        // A pipeline rule file changed since the last look: the assistants'
        // agent files are written from those rules, so they catch up now
        // rather than the next time Auto Run opens. Never on the first look.
        const rules = got.pipeline.fingerprint;
        if (rulesSeen.current?.clone === forClone && rulesSeen.current.fingerprint !== rules) {
          void commands.pwRulesStatus().catch(() => {});
        }
        rulesSeen.current = { clone: forClone, fingerprint: rules };
      } catch (e) {
        if (latest.current.clone !== forClone) return;
        patch(forClone, { walk: { error: message(e) } });
      }
      if (!force) {
        if (print === null) return;
        const was = qc.getQueryData<string>(processedKey(forClone));
        // The first look of this session: what the tab loads next is current.
        if (was === undefined) {
          qc.setQueryData(processedKey(forClone), print);
          patch(forClone, { processed: print });
          return;
        }
        patch(forClone, { processed: was });
        if (was === print) return;
        const failed = failedAt.current;
        if (failed && failed.clone === forClone && failed.fingerprint === print) return;
        // Waiting on a running command, or on a re-read already going.
        if (inFlight.current || latest.current.busy) return;
      } else if (inFlight.current) {
        return;
      }
      inFlight.current = true;
      patch(forClone, { processing: true });
      let failure: string | null = null;
      try {
        await latest.current.process();
      } catch (e) {
        failure = message(e);
      }
      inFlight.current = false;
      if (latest.current.clone !== forClone) return;
      if (failure === null) {
        failedAt.current = null;
        if (print !== null) qc.setQueryData(processedKey(forClone), print);
        patch(forClone, { processing: false, failed: null, ...(print !== null ? { processed: print } : {}) });
      } else if (print !== null) {
        failedAt.current = { clone: forClone, fingerprint: print };
        patch(forClone, { processing: false, failed: { fingerprint: print, error: failure } });
      } else {
        patch(forClone, { processing: false });
      }
    },
    [patch, qc],
  );

  useEffect(() => {
    if (!clone || !ready) return;
    const check = () => void look(false);
    check();
    const id = setInterval(check, SPECS_CHECK_MS);
    window.addEventListener("focus", check);
    return () => {
      clearInterval(id);
      window.removeEventListener("focus", check);
    };
  }, [clone, ready, look]);

  const pill = clone && ready ? specsPill(view) : null;
  useEffect(() => {
    setSpecsPill(pill);
  }, [pill?.tone, pill?.label, pill?.why]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => setSpecsPill(null), []);

  const reprocess = useCallback(() => look(true), [look]);
  return { reprocess };
}
