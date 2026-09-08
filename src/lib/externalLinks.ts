// Rendered work-item content (descriptions, comments) carries real <a>
// tags, and a WebView treats a click as its own navigation - the app
// window sails off to the link and the person has lost their place. Every
// web link belongs in the system browser; nothing in the app itself
// navigates by anchor, so intercepting all of them is safe.

import { openUrl } from "@tauri-apps/plugin-opener";

let wired = false;

/** Idempotent; call once per window at boot. */
export function initExternalLinks(): void {
  if (wired) return;
  wired = true;
  document.addEventListener(
    "click",
    (e) => {
      const a = (e.target as HTMLElement).closest?.("a[href]");
      if (!a) return;
      const href = a.getAttribute("href") ?? "";
      if (!/^https?:\/\//i.test(href)) return; // anchors, mailto: - not ours
      e.preventDefault();
      void openUrl(href).catch(() => {});
    },
    true,
  );
}
