import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import Settings from "./Settings";

afterEach(() => clearMocks());

function renderSettings(qc: QueryClient) {
  return render(
    <QueryClientProvider client={qc}>
      <Settings org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

test("manual update check seeds the [\"update\"] query the App banner reads", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  mockIPC((cmd) => {
    if (cmd === "check_update") return "9.9.9";
    if (cmd === "plugin:app|version") return "1.6.0";
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
  // The banner in App renders from this cache entry (fetched once at
  // startup with staleTime Infinity) - a manual check must write it too.
  await waitFor(() => expect(qc.getQueryData(["update"])).toBe("9.9.9"));
});

test("up-to-date check clears any stale banner state", async () => {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  qc.setQueryData(["update"], "0.0.1"); // pretend a stale value
  mockIPC((cmd) => {
    if (cmd === "check_update") return null;
    if (cmd === "plugin:app|version") return "1.6.0";
  });
  renderSettings(qc);

  fireEvent.click(screen.getByRole("button", { name: /Check for updates/ }));
  await waitFor(() => expect(qc.getQueryData(["update"])).toBeNull());
});

test("the changelog history section lists released versions", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  expect(await screen.findByRole("heading", { name: "Changelog" })).toBeInTheDocument();
  expect(screen.getByText("Version 1.9.0")).toBeInTheDocument();
  expect(screen.getByText("Version 1.7.1")).toBeInTheDocument();
});

test("Settings carries no AI Bridge content (it lives in its own tab)", async () => {
  mockIPC(() => undefined);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  await screen.findByRole("heading", { name: "Changelog" });
  // The changelog history may mention "AI Bridge" in release notes - assert
  // the section itself and the old moved-note are gone, not the words.
  expect(screen.queryByText("AI Bridge has moved to its own tab.")).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "AI Bridge" })).not.toBeInTheDocument();
  expect(screen.queryByText("Registered in Claude Code:")).not.toBeInTheDocument();
});

test("the right column switches from the changelog to the app log", async () => {
  mockIPC((cmd) => {
    if (cmd === "app_logs")
      return [
        { at: "2026-07-26 09:00:01", level: "info", message: "Test Case Manager started" },
        { at: "2026-07-26 09:02:20", level: "error", message: "Submit failed for 'X'" },
      ];
    if (cmd === "app_log_dir") return "C:\logs";
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);

  // Changelog is the default panel.
  expect(await screen.findByText(/the same notes the post-update popup shows/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Logs" }));
  expect(await screen.findByText("Test Case Manager started")).toBeInTheDocument();
  expect(screen.getByText("Submit failed for 'X'")).toBeInTheDocument();
  // The changelog panel is gone, not merely hidden below.
  expect(
    screen.queryByText(/the same notes the post-update popup shows/),
  ).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Copy log" })).toBeInTheDocument();
});
