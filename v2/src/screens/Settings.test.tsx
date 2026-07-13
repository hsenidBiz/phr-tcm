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
