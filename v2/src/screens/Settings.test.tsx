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
  expect(await screen.findByText("Changelog")).toBeInTheDocument();
  expect(screen.getByText("Version 1.9.0")).toBeInTheDocument();
  expect(screen.getByText("Version 1.7.1")).toBeInTheDocument();
});

test("the AI Bridge section shows the registration command with the shipped exe path", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status")
      return { port: 51234, mcp_exe: "C:\\apps\\tcm\\tcm-mcp.exe" };
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  renderSettings(qc);
  expect(await screen.findByText("AI Bridge")).toBeInTheDocument();
  // The static heading renders before the async bridge_status query
  // resolves - wait for the port text rather than asserting synchronously.
  expect(await screen.findByText(/51234/)).toBeInTheDocument();
  // The copy-able registration one-liner embeds the exe path.
  expect(screen.getByText(/tcm-mcp\.exe/)).toBeInTheDocument();
});
