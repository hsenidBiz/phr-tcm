import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { useEffect, useState, useSyncExternalStore } from "react";
import { commands } from "../bindings";
import { toast } from "../lib/toast";
import {
  clearSessionExpired,
  sessionExpiredSnapshot,
  setSessionActive,
  subscribeSessionExpired,
} from "../lib/sessionExpired";
import { authQuery } from "../lib/authQuery";
import { claimCacheFor } from "../lib/cache";
import { forgetAllNotifications } from "../lib/notifications";
import { forgetMentionBaselines } from "../lib/mentions";
import { hideSplash } from "../lib/splash";
import { logUi } from "../lib/uiLog";
import { DEV_TOOLS, loadSignIn } from "./lazyScreens";

/** Sign-in state: the kept session, the sign-in action, the expired-session prompt. */
export function useAppSession() {
  const qc = useQueryClient();
  // On launch this also tries the sign-in Stay signed in kept (lib/
  // authQuery.ts), so the loading screen below stays up for that one
  // round trip and a kept session opens straight into the app.
  const status = useQuery(authQuery);

  // The loading screen (index.html) stays until this answers: before it,
  // `signedIn` reads false, so a signed-in person would see the sign-in
  // screen flash past on every launch. An error answers too - the sign-in
  // screen is the right thing to show then.
  const authKnown = !status.isPending;
  const signedInAtLaunch = Boolean(status.data?.signed_in);
  useEffect(() => {
    if (!authKnown) return;
    const ready = () => {
      hideSplash();
      logUi(`startup: ready ${Math.round(performance.now())} ms after the page began loading`);
    };
    // Signed out, the sign-in screen is what shows next, and it loads on
    // demand: the loading screen waits for it rather than uncovering an
    // empty window. A failed load still lets the loading screen go.
    if (signedInAtLaunch) ready();
    else loadSignIn().then(ready, ready);
    // Once, when the answer arrives: a later sign-in or sign-out has
    // nothing to uncover, so `signedInAtLaunch` is read but not watched.
  }, [authKnown]);

  const signIn = useMutation({
    mutationFn: async () => {
      // A plain browser tab on the Vite dev server has no Tauri backend -
      // the first IPC call used to surface as a raw "Cannot read
      // properties of undefined (reading 'invoke')" toast. Sign-in can
      // NEVER work there (MSAL and the ADO client live in the Rust
      // process), so say what is going on and what to do instead.
      if (!isTauri()) {
        throw new Error(
          "this page is running in a plain browser, where the app's backend isn't available. " +
            "Use the desktop window (npm run tauri dev), or turn on Demo data in the Dev Panel " +
            "to explore the UI with fake data.",
        );
      }
      const r = await commands.signIn();
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: () => {
      // A fresh token retires any pending "session expired" - including a
      // latch parked by pre-sign-in Unauthorized errors racing the very
      // first sign-in. Cleared HERE, on every successful sign-in, not only
      // on the modal's own re-sign-in path.
      clearSessionExpired();
      qc.invalidateQueries({ queryKey: ["auth"] });
    },
    onError: (e) => toast.error(`Sign-in failed: ${e.message}`),
  });

  // Raised by describeAdoError whenever a request came back 401 - silent
  // token renewal has given up (days in hibernate does it), so every new
  // fetch fails until the user signs in again. One prompt instead of a
  // "Not authorized" on each screen.
  const sessionExpired = useSyncExternalStore(subscribeSessionExpired, sessionExpiredSnapshot);

  const reSignIn = () =>
    signIn.mutate(undefined, {
      onSuccess: () => {
        clearSessionExpired();
        // Every query that 401'd is sitting in an error state on cached
        // data - refetch the lot now that the token works again.
        qc.invalidateQueries();
        toast.success("Signed back in.");
      },
    });

  // Dev-only auth override: "out" shows the sign-in screen from a signed-in
  // app (to iterate on it), "in" proceeds without any real session (demo
  // data needs none). DEV_TOOLS is compile-time false in releases, so this
  // state and every branch on it is dead-code-eliminated from client builds.
  const [devAuth, setDevAuth] = useState<"real" | "out" | "in">("real");
  const signedIn =
    DEV_TOOLS && devAuth !== "real" ? devAuth === "in" : Boolean(status.data?.signed_in);

  // The latch is armed only while a session exists. Signed out, the Rust
  // side answers every command with Unauthorized (no token to send) and
  // those flow through the same formatter as real expiries - unarmed, they
  // no longer park a "Session expired" for the freshly signed-in user.
  useEffect(() => {
    setSessionActive(signedIn);
  }, [signedIn]);

  // The persistent cache is scoped by org and project, which is not the same
  // as being scoped by person. Claim it for whoever is signed in NOW.
  //
  // During RENDER, not in an effect. An effect runs after the commit, so on
  // the render where a new account first appears the screens below have
  // already mounted and seeded their queries from the previous account's
  // cache - the wipe then landed a beat too late to stop it being read.
  // `claimCacheFor` is idempotent and guarded by its own owner key, so
  // calling it every render costs a string compare. When it actually wipes
  // (a different account just claimed the cache), the bell and the
  // mentions first-run baseline are the previous person's too - and go
  // with it, or the new account's bell would open showing someone else's
  // notifications, mention bodies included.
  if (claimCacheFor(status.data?.account ?? null)) {
    forgetAllNotifications();
    forgetMentionBaselines();
  }

  return { status, signIn, sessionExpired, reSignIn, signedIn, setDevAuth };
}
