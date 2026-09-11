import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import BridgeStatusBadge from "./BridgeStatusBadge";

afterEach(() => clearMocks());

function renderBadge() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <BridgeStatusBadge />
    </QueryClientProvider>,
  );
}

/// The badge beside the AI Bridge title is the one place the bridge's
/// state is shown: a green glowing dot with the port while it listens.
test("a running bridge shows green with its port", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") return { port: 51234, mcp_exe: "C:\\apps\\tcm\\v2.exe" };
  });
  renderBadge();
  const badge = await screen.findByRole("status");
  expect(badge).toHaveTextContent("Running");
  expect(badge).toHaveAttribute("title", expect.stringContaining("port 51234"));
  expect(badge.className).toContain("text-success");
  expect(badge.querySelector(".status-glow")).toBeTruthy();
});

test("a bridge that failed to start shows red", async () => {
  mockIPC((cmd) => {
    if (cmd === "bridge_status") throw "bridge failed to start";
  });
  renderBadge();
  const badge = await screen.findByRole("status");
  expect(badge).toHaveTextContent("Not running");
  expect(badge.className).toContain("text-danger");
});
