/**
 * The app's toasts. Every screen imports `toast` from HERE - never from a
 * library - so the library behind it is one file's business.
 *
 * It forwards to XiodUI's toast manager (Base UI underneath). The options
 * are the ones the call sites were written against when this was sonner:
 * a message, an optional description, a duration in ms, and an optional
 * action button. The host that draws them is components/ui/toaster.tsx.
 *
 * Exports `toast` only: test files mock this module as `{ toast: {...} }`,
 * and a named export they do not provide would throw when read.
 */
import type { ReactNode } from "react";
import { toastManager } from "xiod-ui/toast";

export type ToastOptions = {
  /** A second line under the message. */
  description?: ReactNode;
  /** How long it stays, in ms. Left out, the host's default (4 s);
   * `Infinity` keeps it until dismissed, as under sonner. */
  duration?: number;
  /** One button on the toast. Clicking it runs `onClick` and dismisses the
   * toast, as sonner's action did. */
  action?: { label: ReactNode; onClick: () => void };
};

type Kind = "success" | "error" | "info" | "warning";
type Show = (message: ReactNode, opts?: ToastOptions) => string;

let issued = 0;

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
    // Base UI's "until dismissed" is 0; Infinity would reach setTimeout,
    // which fires at once.
    timeout: opts.duration === Infinity ? 0 : opts.duration,
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
