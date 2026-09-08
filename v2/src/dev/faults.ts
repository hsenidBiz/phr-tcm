/**
 * DEV-ONLY FAULT INJECTION - make a command fail on purpose.
 *
 * Error copy is the part of a UI nobody ever sees before a user does: the
 * happy path is exercised a hundred times a day and the failure path only
 * when something is already going wrong, in the field, where it cannot be
 * looked at. This arms the next command (or every command) to come back as
 * a chosen `AdoError`.
 *
 * It patches the bindings rather than raising a toast directly, and that
 * distinction is the whole point. A fake `toast.error(...)` would show the
 * string and prove nothing: the real question is what the SCREEN does with
 * a rejection - which query retries, what the row looks like mid-failure,
 * whether the message is even reachable behind a dialog. Going through the
 * bindings means the failure travels the same path a real one does,
 * `describeAdoError` included - and so does its side effect, the
 * session-expired flag that `Unauthorized` raises in `lib/ipc.ts`.
 *
 * Dev build only: loaded from main.tsx through the same `import.meta.env.DEV`
 * branch as demo and latency, so a release binary contains none of it.
 */
import { commands, type AdoError } from "../bindings";
import { describeAdoError } from "../lib/ipc";

/**
 * The three network sentences, mirrored from `network_error` in
 * `v2/src-tauri/src/ado/transport.rs`.
 *
 * A copy, because the Rust constants cannot be imported here - but not an
 * unguarded one: `v2/src-tauri/tests/ado_network.rs` reads THIS FILE and
 * fails if any of the three drifts from the Rust it claims to reproduce.
 * Simulating a message that the app no longer sends would be worse than
 * having no simulator at all.
 */
export const NET_TIMEOUT =
  "Azure DevOps didn't respond in time. Check your connection and try again. Settings → Logs has the details.";
export const NET_UNREACHABLE =
  "Can't reach Azure DevOps. Check your internet connection or VPN, then try again. Settings → Logs has the details.";
export const NET_GENERIC =
  "The connection to Azure DevOps failed. Try again - restart the app if it keeps happening. Settings → Logs has the details.";

/**
 * The update check's own unreachable message, mirrored from
 * `FEED_UNREACHABLE` in `v2/src-tauri/src/updater/mod.rs` and pinned by the
 * same Rust test as the three above.
 *
 * It needs its own switch because `checkUpdate` does not return a
 * `Result` - an unreachable feed is a `blocked` FIELD on a successful
 * response, not an error - so the injector below cannot reach it.
 * Lowercase: `updateToast.ts` renders it after "Could not check for
 * updates: ".
 */
export const FEED_UNREACHABLE =
  "the update server could not be reached. Check your internet connection and try again. Settings → Logs has the details.";

/** One offered failure, in the order the panel lists them. */
export const FAULTS = [
  { id: "timeout", label: "Timeout", error: { kind: "Network", detail: NET_TIMEOUT } },
  { id: "unreachable", label: "Unreachable", error: { kind: "Network", detail: NET_UNREACHABLE } },
  { id: "network", label: "Network (other)", error: { kind: "Network", detail: NET_GENERIC } },
  { id: "unauthorized", label: "401 expired", error: { kind: "Unauthorized" } },
  { id: "forbidden", label: "403", error: { kind: "Forbidden" } },
  { id: "notfound", label: "404", error: { kind: "NotFound" } },
  {
    id: "ratelimited",
    label: "429",
    error: { kind: "RateLimited", detail: { retry_after_secs: 30 } },
  },
  {
    // A real ADO rule rejection - the shape `describeAdoError` digs the
    // `message` out of, which is the one Http case a user can act on.
    id: "rule",
    label: "400 rule error",
    error: {
      kind: "Http",
      detail: {
        status: 400,
        body: JSON.stringify({
          message:
            "TF401320: Rule Error for field Remaining Work. Error code: Required, HasValues.",
        }),
      },
    },
  },
] as const satisfies readonly { id: string; label: string; error: AdoError }[];

export type FaultId = (typeof FAULTS)[number]["id"];
export type FaultMode = "once" | "always";

/** Armed fault, or null. Session-only, deliberately: an "every command
 * fails" flag that survived a reload would outlive the memory of setting
 * it, and the app would just look broken. */
export type ArmedFault = { id: FaultId; mode: FaultMode } | null;
let armed: ArmedFault = null;
/** Set when an armed `once` has been spent, so the panel can say so - the
 * next call is often a background poll rather than the thing you clicked. */
let fired = false;
/** Independent of `armed`: it targets one command, and leaving it on while
 * you work is the point - the check runs hourly on its own. */
let updateBlocked = false;

const listeners = new Set<() => void>();
let snapshot: { armed: ArmedFault; fired: boolean; updateBlocked: boolean } = {
  armed: null,
  fired: false,
  updateBlocked: false,
};

function publish() {
  snapshot = { armed, fired, updateBlocked };
  listeners.forEach((l) => l());
}

export function subscribeFaults(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** Stable object identity between changes - useSyncExternalStore compares
 * snapshots by reference and would loop forever on a fresh literal. */
export function faultSnapshot() {
  return snapshot;
}

export function armFault(id: FaultId, mode: FaultMode): void {
  armed = { id, mode };
  fired = false;
  publish();
}

export function disarmFault(): void {
  armed = null;
  fired = false;
  publish();
}

/** Make every update check report an unreachable feed, until turned off. */
export function toggleUpdateBlocked(): void {
  updateBlocked = !updateBlocked;
  publish();
}

/**
 * Commands the injector always lets through.
 *
 * The log viewer above all: the raw error - URL, timing, the lot - is
 * written to the app log, and checking that it landed there is half of
 * verifying this change. A fault mode that also broke the log viewer would
 * take away the evidence it exists to produce.
 */
const NEVER_FAIL = new Set(["appLogs", "appLogDir", "logUi"]);

/** The error object handed back, in a shape BOTH unwrappers read correctly.
 *
 * Half the generated commands return `Result<_, AdoError>` and half
 * `Result<_, string>`, and nothing at runtime says which is which. So the
 * injected error is an AdoError that also stringifies to its own rendered
 * message: `unwrap` reads `.kind` through `describeAdoError`, and
 * `unwrapStr`'s `new Error(r.error)` stringifies it to the same sentence
 * instead of "[object Object]". */
function injected(error: AdoError): AdoError {
  return Object.assign(Object.create({ toString: () => describeAdoError(error) }), error);
}

/**
 * Wrap every command with the injector.
 *
 * Called after the demo patches and before `applyDevLatency`, so a fake
 * failure arrives after the fake delay - the order a real one would.
 *
 * Only `typedError` commands are patched. The generated bindings also hold
 * infallible commands that resolve to raw data; handing one of those a
 * `{ status: "error" }` would not simulate a failure, it would feed its
 * caller a value of a type that command can never return. The source sniff
 * is what separates the two - crude, but this file only ever runs through
 * Vite's dev transform, which does not rename module-scope functions.
 */
export function applyDevFaults(): void {
  // `checkUpdate` resolves to an `UpdateStatus`, never a `Result` - so it
  // is patched by hand rather than by the loop, and a blocked check is
  // spelt the way the backend spells it: a successful response carrying a
  // reason. Anything else would simulate a failure the app cannot produce.
  const realCheckUpdate = commands.checkUpdate;
  commands.checkUpdate = async () =>
    updateBlocked
      ? { available: null, blocked: FEED_UNREACHABLE, failed_attempt: null }
      : realCheckUpdate();

  for (const [name, fn] of Object.entries(commands)) {
    if (typeof fn !== "function") continue;
    if (NEVER_FAIL.has(name) || name === "checkUpdate") continue;
    if (!fn.toString().includes("typedError")) continue;
    (commands as Record<string, unknown>)[name] = async (...args: unknown[]) => {
      if (armed) {
        const spec = FAULTS.find((f) => f.id === armed!.id)!;
        if (armed.mode === "once") {
          armed = null;
          fired = true;
          publish();
        }
        return { status: "error", error: injected(spec.error) };
      }
      return (fn as (...a: unknown[]) => unknown)(...args);
    };
  }
}
