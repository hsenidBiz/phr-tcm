import { getCurrentWindow } from "@tauri-apps/api/window";
import { loadRunnerPinned, saveRunnerSession, type RunnerSession } from "./runnerSession";

/** Open (or focus) the compact always-on-top runner window on the #runner
 * hash route. Dynamically imports the webview API so vitest never touches it. */
export async function openRunnerWindow(session: RunnerSession) {
  saveRunnerSession(session);
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");

  const existing = await WebviewWindow.getByLabel("runner");
  if (existing) {
    await existing.setFocus();
    return;
  }

  const runner = new WebviewWindow("runner", {
    url: "index.html#runner",
    title: "Test Runner",
    width: 460,
    height: 720,
    // Honour the user's last pin toggle instead of forcing on-top.
    alwaysOnTop: loadRunnerPinned(),
    resizable: true,
    focus: true,
    decorations: false,
  });
  runner.once("tauri://error", (e) => {
    // eslint-disable-next-line no-console
    console.error("runner window error", e);
  });
  // Keep the main window usable behind the runner.
  await getCurrentWindow().setFocus().catch(() => {});
}
