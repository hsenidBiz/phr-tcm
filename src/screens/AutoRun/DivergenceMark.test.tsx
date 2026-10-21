// The person's rulings on a reported divergence, on the case card.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Divergence } from "../../bindings";
import { DivergenceRow } from "./DivergenceMark";

const opener = vi.hoisted(() => ({ openUrl: vi.fn(async (_u: string) => {}) }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

afterEach(() => {
  clearMocks();
  opener.openUrl.mockClear();
});

const base: Divergence = {
  id: "d1",
  case_id: 147556,
  step: 2,
  case_says: "exactly 4 levels",
  app_shows: "6 levels",
  kind: "data",
  reported_at: "1",
  status: "open",
};

function show(d: Divergence) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "auto_run_rule_divergence") return [];
    return null;
  });
  const qc = new QueryClient();
  render(
    <QueryClientProvider client={qc}>
      <DivergenceRow org="acme" project="Web" caseId={147556} divergence={d} />
    </QueryClientProvider>,
  );
  return calls;
}

test("an open data divergence offers three rulings", () => {
  show(base);
  expect(screen.getByText("Needs a ruling")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Rule the case is right for #147556 step 2" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Rule the data fixed for #147556 step 2" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Rewrite case #147556 in Azure DevOps" })).toBeInTheDocument();
});

test("an app divergence offers no data fixed", () => {
  show({ ...base, kind: "app" });
  expect(screen.getByText("Divergence")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /data fixed/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Rule the case is right/ })).toBeInTheDocument();
});

test("confirming the case is right calls the command with case_stands", async () => {
  const calls = show(base);
  fireEvent.click(screen.getByRole("button", { name: /Rule the case is right/ }));
  expect(screen.getByText("The script keeps the case's value at step 2, and the case runs again.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "auto_run_rule_divergence")?.args).toEqual({
      organization: "acme",
      project: "Web",
      id: "d1",
      ruling: "case_stands",
    }),
  );
});

test("rewrite opens azure devops on the case then rules", async () => {
  const calls = show(base);
  fireEvent.click(screen.getByRole("button", { name: /Rewrite case #147556/ }));
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "auto_run_rule_divergence")).toBe(true));
  expect(opener.openUrl.mock.calls[0][0]).toMatch(/\/_workitems\/edit\/147556$/);
  expect((calls.find((c) => c.cmd === "auto_run_rule_divergence")?.args as { ruling: string }).ruling).toBe("rewrite");
});

test("keep closes the confirm without ruling", () => {
  const calls = show(base);
  fireEvent.click(screen.getByRole("button", { name: /Rule the data fixed/ }));
  fireEvent.click(screen.getByRole("button", { name: "Keep" }));
  expect(screen.queryByRole("button", { name: "Confirm" })).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "auto_run_rule_divergence")).toBe(false);
});

test("a case waiting for a repair has no buttons and names heal", () => {
  show({ ...base, status: "awaiting_repair" });
  expect(screen.getByText("Waits for a repair")).toBeInTheDocument();
  expect(screen.queryByRole("button")).not.toBeInTheDocument();
  expect(screen.getByText(/\/tcm:heal/)).toBeInTheDocument();
});
