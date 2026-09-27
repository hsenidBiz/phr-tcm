// The bar is where the app-wide background checks live: the PR badge and,
// beside it, the mentions check.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import ContextBar from "./ContextBar";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

test("the context bar checks for mentions in the picked project", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_orgs") return [];
    if (cmd === "list_projects") return [];
    if (cmd === "pr_overview") return { mine: [], awaiting: [] };
    if (cmd === "recent_mentions") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ContextBar
        org="acme"
        setOrg={() => {}}
        project="Web"
        setProject={() => {}}
        pbi={null}
        setPbi={() => {}}
        account={null}
        workMode={false}
        onToggleWork={() => {}}
        onOpenSettings={() => {}}
      />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "recent_mentions")?.args).toEqual({ organization: "acme", project: "Web" }),
  );
  expect(calls.some((c) => c.cmd === "pr_overview")).toBe(true);
});

/// The gear turns forward as Settings opens and back as it closes, however
/// it closes (index.css, ico-cog-turn / ico-cog-turn-back). Each change is a
/// fresh icon, so the turn plays again; a first render plays nothing.
test("the Settings gear turns forward as Settings opens and back as it closes", () => {
  mockIPC((cmd) => {
    if (cmd === "list_orgs") return [];
    if (cmd === "list_projects") return [];
    if (cmd === "pr_overview") return { mine: [], awaiting: [] };
    if (cmd === "recent_mentions") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const bar = (settingsOpen: boolean) => (
    <QueryClientProvider client={qc}>
      <ContextBar
        org="acme"
        setOrg={() => {}}
        project="Web"
        setProject={() => {}}
        pbi={null}
        setPbi={() => {}}
        account={null}
        workMode={false}
        onToggleWork={() => {}}
        onOpenSettings={() => {}}
        settingsOpen={settingsOpen}
      />
    </QueryClientProvider>
  );
  const { rerender } = render(bar(false));
  const icon = () => screen.getByRole("button", { name: /settings/i }).querySelector("svg")!;
  expect(icon()).not.toHaveClass("ico-cog-turn");
  expect(icon()).not.toHaveClass("ico-cog-turn-back");

  rerender(bar(true));
  const opened = icon();
  expect(opened).toHaveClass("ico-cog-turn");

  rerender(bar(false));
  expect(icon()).toHaveClass("ico-cog-turn-back");
  expect(icon()).not.toBe(opened);

  // A re-render that changes nothing does not replay it.
  const closed = icon();
  rerender(bar(false));
  expect(icon()).toBe(closed);
});
