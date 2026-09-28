import { commands } from "../bindings";

/** Who is signed in - the one query for it, shared so every reader asks
 *  the same function under the same key. `resumeSession` answers from
 *  memory once someone is signed in; before that (at launch) it first tries
 *  the sign-in Stay signed in kept, so a session Microsoft still accepts
 *  goes straight into the app without the browser. */
export const authQuery = {
  queryKey: ["auth"],
  queryFn: () => commands.resumeSession(),
};

/** The query keys that outlive a sign-out: what is about the app and this
 *  machine, not about the person who was signed in. */
export const KEPT_ON_SIGN_OUT = new Set(["auth", "update", "app-version", "app-settings", "autostart"]);
