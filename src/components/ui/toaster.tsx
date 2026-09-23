import { ToastProvider } from "xiod-ui/toast";

/** Sonner's default - the length every message in the app was written to
 * be read in. A call site that needs longer passes `duration`. */
const TOAST_MS = 4_000;

/**
 * The window's one toast host: XiodUI's, bottom-right, where the app's
 * toasts have always come up. Mount it once per window (App, RunnerWindow
 * via HoverDismissToaster); `toast` from lib/toast.ts reaches it from
 * anywhere. Colours come from the app's tokens through src/xiod-theme.css.
 */
export function Toaster() {
  return <ToastProvider position="bottom-right" timeout={TOAST_MS} />;
}
