import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, expect, test, vi } from "vitest";
import { initExternalLinks } from "./externalLinks";

afterEach(() => {
  clearMocks();
  document.body.innerHTML = "";
});

/// A description's <a> click must reach the system browser, never
/// navigate the app window away from where the person was.
test("web links open externally and the in-page navigation is stopped", async () => {
  const opened: string[] = [];
  mockIPC((cmd, args) => {
    if (String(cmd).startsWith("plugin:opener|")) {
      opened.push((args as { url: string }).url);
      return null;
    }
  });
  initExternalLinks();

  const a = document.createElement("a");
  a.href = "https://example.com/spec";
  a.textContent = "the spec";
  document.body.appendChild(a);

  const ev = new MouseEvent("click", { bubbles: true, cancelable: true });
  a.dispatchEvent(ev);
  expect(ev.defaultPrevented).toBe(true);
  await vi.waitFor(() => expect(opened).toEqual(["https://example.com/spec"]));

  // A fragment anchor is not a web link - it stays untouched.
  const frag = document.createElement("a");
  frag.setAttribute("href", "#top");
  document.body.appendChild(frag);
  const ev2 = new MouseEvent("click", { bubbles: true, cancelable: true });
  frag.dispatchEvent(ev2);
  expect(ev2.defaultPrevented).toBe(false);
});
