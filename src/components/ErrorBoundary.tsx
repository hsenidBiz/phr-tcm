// The last line of defence against a white window.
//
// React unmounts the whole tree when a render throws and nothing catches
// it - and this app had nothing catching it. A user saw exactly that after
// an upload: the window went white, the app log recorded nothing (only
// Rust writes the log), and there was no way to tell what had thrown.
//
// Two jobs, then. Catch it: show what went wrong and a way back, with the
// app still standing. Record it: the message and stack go into the app
// log through the same command the UI breadcrumbs use, so a bug report
// carries the failure instead of "the screen went blank". The global
// hooks below do the recording for errors that happen OUTSIDE render -
// event handlers, promises - which React never routes through a boundary.

import { Component, type ErrorInfo, type ReactNode } from "react";
import { commands } from "../bindings";

/** Best-effort: the log is a diagnostic, never a reason to fail twice. */
function record(kind: string, message: string, detail?: string): void {
  const line = detail ? `${kind}: ${message}\n${detail}` : `${kind}: ${message}`;
  try {
    void commands.logUi(line.slice(0, 4000));
  } catch {
    // Outside the webview (tests, a plain browser) there is no bridge.
  }
}

/** Records uncaught errors and unhandled promise rejections into the app
 * log. Idempotent: installing twice adds nothing. */
let installed = false;
export function installGlobalErrorLog(): void {
  if (installed || typeof window === "undefined") return;
  installed = true;
  window.addEventListener("error", (e) => {
    const err = e.error as { stack?: string } | undefined;
    record("uncaught error", e.message || String(e.error), err?.stack);
  });
  window.addEventListener("unhandledrejection", (e) => {
    const r = e.reason as { message?: string; stack?: string } | undefined;
    record("unhandled rejection", r?.message ?? String(e.reason), r?.stack);
  });
}

type State = { error: Error | null };

export default class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    record("render crash", error.message, `${error.stack ?? ""}\ncomponent stack:${info.componentStack ?? ""}`);
  }

  render(): ReactNode {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div role="alert" className="flex h-screen flex-col items-center justify-center gap-4 bg-bg p-8 text-text">
        <h1 className="text-lg font-semibold">Something went wrong in the app</h1>
        <p className="max-w-lg text-center text-sm text-muted">
          The screen could not be drawn. The details are in the app log (Settings → Logs), and a
          bug report from there will carry them. Reloading brings the app back; your queue and
          settings are kept.
        </p>
        <pre className="max-h-40 max-w-2xl overflow-auto rounded-md border border-border bg-surface px-3 py-2 text-xs text-danger">
          {error.message}
        </pre>
        <button
          className="rounded-md bg-accent px-4 py-2 text-sm font-medium text-on-accent hover:opacity-90"
          onClick={() => window.location.reload()}
        >
          Reload the app
        </button>
      </div>
    );
  }
}
