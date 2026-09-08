import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, expect, test, vi } from "vitest";
import { initUiClickLog } from "./uiLog";

afterEach(() => {
  clearMocks();
  document.body.innerHTML = "";
});

test("clicks on named controls land in the app log by accessible name", async () => {
  const logged: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "log_ui") {
      logged.push((args as { message: string }).message);
      return null;
    }
  });
  initUiClickLog();

  const btn = document.createElement("button");
  btn.setAttribute("aria-label", "Refresh test plans");
  document.body.appendChild(btn);
  btn.click();

  // An unnamed control logs nothing - there is no name worth recording.
  const bare = document.createElement("button");
  document.body.appendChild(bare);
  bare.click();

  await vi.waitFor(() => expect(logged).toContain("click: Refresh test plans"));
  expect(logged).toHaveLength(1);
});
