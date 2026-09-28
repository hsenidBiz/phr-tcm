import { ToastProvider } from "xiod-ui/toast";

/** Only a fallback: every toast from lib/toast.ts brings its own timing
 * (and tells the provider "until dismissed"), because Base UI's clock
 * stops whenever the window is not focused. See lib/toast.ts. */
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
