// What the server said about each saved database's login: may it create,
// update and delete? Rust asks (`db_check_write`) and keeps the answer; this
// file reads it, asks again when told to, and remembers which asks are in
// flight and which failed so the AI Bridge card and the Manage databases
// dialog show the same thing.
//
// None of it is secret - a verdict, a time and a sentence. The login's
// password never leaves Rust.

import { useQuery, type QueryClient } from "@tanstack/react-query";
import { useSyncExternalStore } from "react";
import { commands, type DbWriteVerdict, type WriteCheck } from "../bindings";
import { logUi } from "./uiLog";

export const DB_VERDICTS_KEY = ["db-write-verdicts"] as const;

/** Said when a check could not be made: the server's own words stay in the log. */
export const CHECK_FAILED = "Could not check this login's permissions. Try Recheck, or see Settings → Logs.";

/** The start of each sentence Rust itself words for a check that could not be
 * made at all (no sqlcmd, no login saved, no database chosen). Anything else
 * - transport text included - is never shown; `CHECK_FAILED` stands in. */
const RUST_SENTENCES = [
  "sqlcmd is not installed on this machine",
  "no login is saved for this database",
  "no database connection is chosen",
];

/** `error` when it is one of Rust's own fixed sentences, else null. */
export function ownSentence(error: unknown): string | null {
  return typeof error === "string" && RUST_SENTENCES.some((s) => error.startsWith(s)) ? error : null;
}

type Progress = {
  checking: ReadonlySet<string>;
  failed: ReadonlySet<string>;
  /** Why a failed check failed, where Rust said so in its own sentence. */
  reasons: ReadonlyMap<string, string>;
};
let progress: Progress = { checking: new Set(), failed: new Set(), reasons: new Map() };
const listeners = new Set<() => void>();
const set = (next: Progress) => {
  progress = next;
  for (const l of listeners) l();
};
const subscribe = (cb: () => void) => {
  listeners.add(cb);
  return () => listeners.delete(cb);
};
const snapshot = () => progress;

/** The verdicts Rust holds now, one per database whose current login has one. */
export function useDbWriteVerdicts(enabled: boolean) {
  return useQuery({
    queryKey: DB_VERDICTS_KEY,
    queryFn: async () => (await commands.dbWriteVerdicts()) ?? [],
    enabled,
  });
}

/** The verdict for database `id`, or null while it has none. */
export function verdictOf(list: DbWriteVerdict[] | undefined, id: string): WriteCheck | null {
  return list?.find((v) => v.id === id)?.check ?? null;
}

/** Which databases are being asked about, and which ask failed. */
export function useDbCheckProgress(): Progress {
  return useSyncExternalStore(subscribe, snapshot);
}

/** Asks the server about database `id` and refreshes the verdicts. Never
 * throws: a failure is remembered (and logged without the server's words
 * beyond what Rust already filtered) until the next try. A check already in
 * flight for the same id is not repeated. */
export async function recheckDb(qc: QueryClient, id: string): Promise<void> {
  if (!id || progress.checking.has(id)) return;
  set({
    checking: new Set(progress.checking).add(id),
    failed: without(progress.failed, id),
    reasons: withoutKey(progress.reasons, id),
  });
  let failed = false;
  let reason: string | null = null;
  try {
    const res = await commands.dbCheckWrite(id);
    if (res.status === "error") {
      failed = true;
      reason = ownSentence(res.error);
      logUi(`db write check failed: ${res.error}`);
    }
  } catch (e) {
    failed = true;
    logUi(`db write check failed: ${String(e)}`);
  }
  set({
    checking: without(progress.checking, id),
    failed: failed ? new Set(progress.failed).add(id) : progress.failed,
    reasons: reason ? new Map(progress.reasons).set(id, reason) : progress.reasons,
  });
  await qc.invalidateQueries({ queryKey: DB_VERDICTS_KEY });
}

function without(s: ReadonlySet<string>, id: string): ReadonlySet<string> {
  const next = new Set(s);
  next.delete(id);
  return next;
}

function withoutKey(m: ReadonlyMap<string, string>, id: string): ReadonlyMap<string, string> {
  const next = new Map(m);
  next.delete(id);
  return next;
}

/** The sentence to show for a database: the check failing (Rust's own
 * sentence when it gave one, else the generic one), the verdict, or nothing
 * before it has been checked. */
export function reasonFor(check: WriteCheck | null, failed: boolean, why?: string | null): string | null {
  if (failed) return why ?? CHECK_FAILED;
  return check ? check.reason : null;
}

/** "Can write", "Read only" or "Not checked". */
export function statusOf(check: WriteCheck | null): string {
  return check ? (check.can_write ? "Can write" : "Read only") : "Not checked";
}

/** The check's time for a `title`, or an empty string. */
export function checkedAtTitle(check: WriteCheck | null): string {
  return check?.checked_at != null ? `Checked ${new Date(check.checked_at).toLocaleString()}` : "";
}

