import { commands, type ScriptLookup_Serialize } from "../bindings";

type CaseScript = NonNullable<ScriptLookup_Serialize["script"]>;

/** The most cases one call carries. */
const CHUNK = 200;

type Waiter = { resolve: (s: CaseScript | null) => void; reject: (e: Error) => void };

let pending = new Map<number, Waiter[]>();
let scheduled = false;

/**
 * One case's saved script, or `null` when it has none - the same answer as
 * `autoRunLoadScript`, but every case asked for in the same moment is read
 * in one call. The Auto Run screen asks for each listed case's script at
 * once (and again after any script changes); one call per case was
 * hundreds of round trips, each opening a file. Nothing is kept: each
 * call is fresh, so the query cache stays the only cache.
 */
export function loadScript(caseId: number): Promise<CaseScript | null> {
  return new Promise((resolve, reject) => {
    const waiters = pending.get(caseId);
    if (waiters) waiters.push({ resolve, reject });
    else pending.set(caseId, [{ resolve, reject }]);
    if (!scheduled) {
      scheduled = true;
      // After the code asking has finished: a render starts its queries
      // one after another in one pass, so all of them land in one batch.
      queueMicrotask(flush);
    }
  });
}

function flush() {
  const batch = pending;
  pending = new Map();
  scheduled = false;
  const ids = [...batch.keys()];
  for (let i = 0; i < ids.length; i += CHUNK) {
    void send(ids.slice(i, i + CHUNK), batch);
  }
}

async function send(ids: number[], batch: Map<number, Waiter[]>) {
  const fail = (e: Error) => ids.forEach((id) => batch.get(id)?.forEach((w) => w.reject(e)));
  try {
    const r = await commands.autoRunLoadScripts(ids);
    if (r.status === "error") return fail(new Error(r.error));
    const answered = new Set<number>();
    for (const l of r.data) {
      answered.add(l.case_id);
      for (const w of batch.get(l.case_id) ?? []) {
        if (l.error) w.reject(new Error(l.error));
        else w.resolve(l.script ?? null);
      }
    }
    // Every id asked for must be answered, or its query would wait forever.
    const missing = ids.filter((id) => !answered.has(id));
    if (missing.length) {
      const e = new Error("the script was not in the answer");
      missing.forEach((id) => batch.get(id)?.forEach((w) => w.reject(e)));
    }
  } catch (e) {
    fail(e instanceof Error ? e : new Error(String(e)));
  }
}
