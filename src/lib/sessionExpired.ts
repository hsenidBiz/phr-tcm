/**
 * Whether the ADO session has expired, as one shared signal.
 *
 * The Rust side renews tokens silently while the refresh token is good,
 * so a 401 reaching the UI means silent renewal is over - typically after
 * a weekend in hibernate. Before this store, that state surfaced as one
 * "Not authorized" error per screen the user touched, with the fix (sign
 * in again) left for them to infer; the app looked broken, not signed out.
 *
 * The flag is raised wherever an Unauthorized error is being shown to the
 * user and read by App, which offers one re-sign-in prompt instead. It is
 * a latch, not a live probe: cleared when the user signs back in or
 * dismisses the prompt, re-raised by the next 401.
 *
 * The latch is ARMED only while the app is signed in. Before sign-in the
 * Rust side answers every command with Unauthorized without touching the
 * network (there is no token to send), and those errors flow through the
 * same formatter as real expiries - so a background query racing the very
 * first sign-in used to park the latch, and the "Session expired" modal
 * greeted the user seconds after they signed in. While signed out, the
 * SignIn screen IS the prompt; the latch has nothing to add.
 */

let expired = false;
let active = false;
const listeners = new Set<() => void>();

function set(value: boolean) {
  if (expired === value) return;
  expired = value;
  for (const l of listeners) l();
}

/** App tells the store whether a session exists. Arming late is the point:
 * an Unauthorized formatted while signed out is not an expiry. Going
 * inactive also retires any raised latch - there is no session left for
 * it to be about. */
export function setSessionActive(value: boolean): void {
  active = value;
  if (!value) set(false);
}

export function flagSessionExpired(): void {
  if (active) set(true);
}

export function clearSessionExpired(): void {
  set(false);
}

export function subscribeSessionExpired(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function sessionExpiredSnapshot(): boolean {
  return expired;
}
