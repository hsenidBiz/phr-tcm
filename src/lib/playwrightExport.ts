// Export to Playwright lives in the Playwright tab. Auto Run's "Export to
// Playwright" menu item is a shortcut to it: it leaves the PBI's cases here,
// the App shell opens the Playwright tab, and the tab opens the Export
// dialog with them. Closing the dialog clears the request.
import { useEffect, useRef, useSyncExternalStore } from "react";

export type PlaywrightExportRequest = {
  pbiId: number;
  caseIds: number[];
  /** Each case's Module, as Auto Run has it. */
  modules: [number, string][];
};

let pending: PlaywrightExportRequest | null = null;
const listeners = new Set<() => void>();

function notify(): void {
  for (const l of [...listeners]) l();
}

export function subscribePlaywrightExport(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}

export function playwrightExportSnapshot(): PlaywrightExportRequest | null {
  return pending;
}

/** Ask for the Export dialog in the Playwright tab, with these cases. */
export function requestPlaywrightExport(r: PlaywrightExportRequest): void {
  pending = r;
  notify();
}

export function clearPlaywrightExportRequest(): void {
  if (pending === null) return;
  pending = null;
  notify();
}

export function usePlaywrightExportRequest(): PlaywrightExportRequest | null {
  return useSyncExternalStore(subscribePlaywrightExport, playwrightExportSnapshot);
}

/** Calls `open` each time a new request arrives: the App shell's way of
 * moving to the Playwright tab, whichever tab the person is on. */
export function useOpenOnExportRequest(open: () => void): void {
  const request = usePlaywrightExportRequest();
  const latest = useRef(open);
  useEffect(() => {
    latest.current = open;
  });
  useEffect(() => {
    if (request) latest.current();
  }, [request]);
}
