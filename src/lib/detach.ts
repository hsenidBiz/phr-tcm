/**
 * Stop listening, without letting the teardown throw.
 *
 * The event plugin's `unlisten` reaches into
 * `window.__TAURI_EVENT_PLUGIN_INTERNALS__`, which only the Tauri runtime
 * fills in. Anywhere else - jsdom, a browser preview - calling it rejects,
 * and an unhandled rejection out of a React cleanup is attributed to
 * whatever test happened to be unmounting, so one listener took several
 * unrelated tests down with it.
 *
 * There is nothing to recover from here either way: the listener is going
 * away with the component.
 */
export function detach(unlisten: (() => void) | undefined): void {
  if (!unlisten) return;
  try {
    // May return a promise (it does in @tauri-apps/api) or nothing.
    void Promise.resolve(unlisten() as unknown).catch(() => {});
  } catch {
    // Threw synchronously instead - same conclusion.
  }
}
