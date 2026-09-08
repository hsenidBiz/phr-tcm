// UI breadcrumbs into the Rust app log (the one Settings shows and bug
// reports ship): which screen was opened, which control was clicked. One
// capture-phase listener covers every button in the window - controls are
// identified by their accessible name, so nothing typed is ever logged.

import { commands } from "../bindings";

export function logUi(message: string): void {
  void commands.logUi(message.slice(0, 200)).catch(() => {});
}

let wired = false;

/** Idempotent; call once per window at boot. */
export function initUiClickLog(): void {
  if (wired) return;
  wired = true;
  document.addEventListener(
    "click",
    (e) => {
      const el = (e.target as HTMLElement).closest?.("button, [role=button]");
      if (!el) return;
      const name =
        el.getAttribute("aria-label") ?? el.textContent?.trim().replace(/\s+/g, " ") ?? "";
      if (name) logUi(`click: ${name.slice(0, 80)}`);
    },
    true,
  );
}
