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
 */

let expired = false;
const listeners = new Set<() => void>();

function set(value: boolean) {
  if (expired === value) return;
  expired = value;
  for (const l of listeners) l();
}

export function flagSessionExpired(): void {
  set(true);
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
