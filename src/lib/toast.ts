/**
 * The app's toasts. Every screen imports `toast` from HERE - never from a
 * library - so the library behind it is one file's business.
 *
 * It forwards to XiodUI's toast manager (Base UI underneath). The options
 * are the ones the call sites were written against when this was sonner:
 * a message, an optional description, a duration in ms, and an optional
 * action button. The host that draws them is components/ui/toaster.tsx.
 *
 * The timing is ours, not the manager's. Base UI pauses every toast while
 * the window is not the focused one, so a toast that arrived while the
 * person was in another window - the app beside a browser, or out in the
 * tray - never left at all. Here a toast leaves when its time is up
 * whatever window has focus, and waits only while the pointer is actually
 * on the toasts, where it is being read. No toast stays past
 * `MAX_TOAST_MS`; it fades away where it stands (xiod-theme.css).
 *
 * Exports `toast` only: test files mock this module as `{ toast: {...} }`,
 * and a named export they do not provide would throw when read.
 */
import type { ReactNode } from "react";
import { toastManager } from "xiod-ui/toast";

export type ToastOptions = {
  /** A second line under the message. */
  description?: ReactNode;
  /** How long it stays, in ms: 4 s when left out, and never more than
   * 10 s. `Infinity` keeps it until dismissed, as under sonner. */
  duration?: number;
  /** One button on the toast. Clicking it runs `onClick` and dismisses the
   * toast, as sonner's action did. */
  action?: { label: ReactNode; onClick: () => void };
};

type Kind = "success" | "error" | "info" | "warning";
type Show = (message: ReactNode, opts?: ToastOptions) => string;

let issued = 0;

/** Sonner's default - the length every message in the app was written to
 * be read in. */
const DEFAULT_MS = 4_000;
/** The longest any toast stays, whatever its caller asked for - except
 * `Infinity`, which asks to stay until dismissed. */
const MAX_TOAST_MS = 10_000;

type Timer = { remaining: number; started: number; handle: ReturnType<typeof setTimeout> | null };
const timers = new Map<string, Timer>();
/** The pointer is on the toasts: every timer waits until it leaves. */
let hovering = false;

function run(id: string, t: Timer): void {
  if (t.handle || hovering) return;
  t.started = Date.now();
  t.handle = setTimeout(() => {
    timers.delete(id);
    toastManager.close(id);
  }, t.remaining);
}

function hold(t: Timer): void {
  if (!t.handle) return;
  clearTimeout(t.handle);
  t.handle = null;
  t.remaining = Math.max(0, t.remaining - (Date.now() - t.started));
}

const inToasts = (n: EventTarget | null) =>
  n instanceof Element && n.closest('[data-slot="toast-viewport"]') !== null;

let watching = false;
/** Where the pointer is, once, on the document: the viewport is portalled
 * and remounts, so listening on it would lose track. */
function watchPointer(): void {
  if (watching || typeof document === "undefined") return;
  watching = true;
  document.addEventListener("pointerover", (e) => {
    if (hovering || !inToasts(e.target)) return;
    hovering = true;
    for (const t of timers.values()) hold(t);
  });
  document.addEventListener("pointerout", (e) => {
    if (!hovering || inToasts(e.relatedTarget)) return;
    hovering = false;
    for (const [id, t] of timers) run(id, t);
  });
}

function show(kind: Kind | undefined, message: ReactNode, opts: ToastOptions = {}): string {
  // The id is ours, not the manager's, because the action has to close
  // exactly this toast and is built before the manager would hand one back.
  issued += 1;
  const id = `app-toast-${issued}`;
  const { action } = opts;
  toastManager.add({
    id,
    type: kind,
    title: message,
    description: opts.description,
    // Never the manager's own timer (0 is its "until dismissed") - see
    // the note at the top for why the timing is ours.
    timeout: 0,
    onRemove: () => {
      const t = timers.get(id);
      if (t?.handle) clearTimeout(t.handle);
      timers.delete(id);
    },
    actionProps: action
      ? {
          children: action.label,
          onClick: () => {
            action.onClick();
            toastManager.close(id);
          },
        }
      : undefined,
  });
  if (opts.duration !== Infinity) {
    watchPointer();
    const ms = Math.min(Math.max(0, opts.duration ?? DEFAULT_MS), MAX_TOAST_MS);
    const t: Timer = { remaining: ms, started: 0, handle: null };
    timers.set(id, t);
    run(id, t);
  }
  return id;
}

export const toast: Show & {
  success: Show;
  error: Show;
  info: Show;
  warning: Show;
  dismiss: (id?: string) => void;
} = Object.assign((message: ReactNode, opts?: ToastOptions) => show(undefined, message, opts), {
  success: (message: ReactNode, opts?: ToastOptions) => show("success", message, opts),
  error: (message: ReactNode, opts?: ToastOptions) => show("error", message, opts),
  info: (message: ReactNode, opts?: ToastOptions) => show("info", message, opts),
  warning: (message: ReactNode, opts?: ToastOptions) => show("warning", message, opts),
  /** One toast by id, or every toast. */
  dismiss: (id?: string) => toastManager.close(id),
});
