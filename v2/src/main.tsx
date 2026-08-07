import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import RunnerWindow from "./screens/RunnerWindow";
import { TooltipLayer } from "./components/ui/tooltip";
import { initTheme } from "./lib/theme";
import { initUiClickLog } from "./lib/uiLog";
import { initExternalLinks } from "./lib/externalLinks";
import "./index.css";

// refetchOnWindowFocus off: a desktop app loses/regains focus constantly
// (alt-tab to the browser and back), and the default would refire every
// mounted ADO query each time - the app's single largest source of
// silent API traffic. Screens that need freshness refetch explicitly.
const queryClient = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false } },
});
initTheme();
initUiClickLog();
initExternalLinks();

// Suppress the browser context menu (back / refresh / inspect) everywhere
// except editable fields, where the native cut/copy/paste menu stays useful.
// Applies to both the main and runner windows (same bundle).
window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement | null;
  if (!t?.closest?.('input, textarea, [contenteditable="true"]')) e.preventDefault();
});

// Dev builds only: demo-data mode patches every ADO command with an
// in-memory fake org BEFORE anything fetches. The dynamic import inside a
// statically-false branch keeps the module out of release bundles.
if (import.meta.env.DEV && import.meta.env.MODE !== "test") {
  const { maybeEnableDemoMode } = await import("./dev/demo");
  maybeEnableDemoMode();
  // Latency wraps AFTER the demo patches, so real IPC and the demo fakes
  // both answer through the same configurable delay (DevPanel knob).
  const { applyDevLatency } = await import("./dev/latency");
  applyDevLatency();
}

// The compact always-on-top runner opens as a second webview window on the
// same bundle, routed by hash (see RunTests -> openRunnerWindow).
const Root = window.location.hash === "#runner" ? RunnerWindow : App;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <Root />
      {/* One per window: turns every `title` in the tree into the app's
          own tooltip. See components/ui/tooltip.tsx. */}
      <TooltipLayer />
    </QueryClientProvider>
  </React.StrictMode>,
);

// React has mounted: fade the splash out and reveal the (hidden-at-start)
// window - together these kill the white startup flash.
requestAnimationFrame(() => {
  const splash = document.getElementById("splash");
  if (splash) {
    splash.style.transition = "opacity 250ms ease-out";
    splash.style.opacity = "0";
    setTimeout(() => splash.remove(), 300);
  }
  import("@tauri-apps/api/window")
    .then(({ getCurrentWindow }) => getCurrentWindow().show())
    .catch(() => {}); // vitest/browser: no tauri window to show
});
