// The plain snow layer is mounted by App for a signed-in window in December
// under Light, and is absent outside the window. Its own file: a whole-app
// mount, kept out of App.test.tsx's long run.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { configure, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import App from "../src/App";
import { ALL_MODULES, resetModulesStore } from "../src/lib/modules";

vi.setConfig({ testTimeout: 15_000 });
configure({ asyncUtilTimeout: 5_000 });

afterEach(() => {
  vi.useRealTimers();
  clearMocks();
  localStorage.clear();
  resetModulesStore();
  document.documentElement.removeAttribute("data-theme");
});

function start(now: Date) {
  // Only the clock is faked: the app's own timers and promises run for real.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(now);
  localStorage.setItem("tcm-v2-tour-done", "yes");
  localStorage.setItem("tcm-v2-theme-id", "light");
  mockIPC((cmd) => {
    if (cmd === "modules_get") return [...ALL_MODULES];
    if (cmd === "resume_session") return { signed_in: true, account: "a@b.com" };
    if (cmd === "check_update") return null;
    if (cmd === "list_orgs") return [{ name: "acme", url: "" }];
    if (cmd === "list_test_case_fields") return [];
    if (cmd === "get_extras_unlocked") return false;
    if (cmd === "plugin:event|listen") return 1;
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <App />
    </QueryClientProvider>,
  );
}

test("a signed-in app in December under Light mounts the snow layer", async () => {
  const { container } = start(new Date(2026, 11, 1, 9));
  await screen.findByText("a@b.com");
  expect(container.querySelector(".snow-layer")).not.toBeNull();
  expect(container.querySelectorAll(".snow-layer .christmas-flake")).toHaveLength(20);
});

test("the same app in November has none", async () => {
  const { container } = start(new Date(2026, 10, 15, 9));
  await screen.findByText("a@b.com");
  expect(container.querySelector(".snow-layer")).toBeNull();
});
