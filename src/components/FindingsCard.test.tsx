import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import FindingsCard from "./FindingsCard";
import type { Finding } from "../bindings";

afterEach(() => clearMocks());

const open: Finding = {
  id: "1-0", org: "acme", project: "Web", kind: "spec",
  subject: "Step10.md 7.7", title: "AC-3 contradicts the table",
  detail: "The table says **closed**.", created_at: "2026-09-11T10:00:00Z", status: "open",
};
const done: Finding = { ...open, id: "1-1", kind: "test_case", subject: "155170", title: "Step 3 expects a toast", status: "resolved" };

function renderCard(findings: Finding[], calls: string[] = []) {
  mockIPC((cmd, args) => {
    calls.push(cmd);
    if (cmd === "list_findings") return findings;
    if (cmd === "set_finding_status") {
      const a = args as { id: string; status: string };
      return { status: "ok", data: { ...findings.find((f) => f.id === a.id)!, status: a.status } };
    }
    if (cmd === "remove_finding") return { status: "ok", data: null };
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <FindingsCard org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

/// Open findings are the default view: kind, subject, title, and the
/// detail rendered as markdown. Resolved ones wait behind a switch.
test("open findings are listed with their kind and rendered detail", async () => {
  renderCard([open, done]);
  expect(await screen.findByText("AC-3 contradicts the table")).toBeInTheDocument();
  expect(screen.getByText("Spec")).toBeInTheDocument();
  expect(screen.getByText("Step10.md 7.7")).toBeInTheDocument();
  expect(screen.getByText("closed").tagName).toBe("STRONG");
  expect(screen.queryByText("Step 3 expects a toast")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("switch", { name: "Show resolved" }));
  expect(screen.getByText("Step 3 expects a toast")).toBeInTheDocument();
  expect(screen.getByText("Test case")).toBeInTheDocument();
});

test("Resolve and Dismiss call their commands with the finding's id", async () => {
  const calls: string[] = [];
  renderCard([open], calls);
  const row = (await screen.findByText("AC-3 contradicts the table")).closest("li")!;
  fireEvent.click(within(row).getByRole("button", { name: "Resolve" }));
  await waitFor(() => expect(calls).toContain("set_finding_status"));
  fireEvent.click(within(row).getByRole("button", { name: "Dismiss" }));
  await waitFor(() => expect(calls).toContain("remove_finding"));
});

test("with nothing recorded the card says so", async () => {
  renderCard([]);
  expect(await screen.findByText(/No findings yet/)).toBeInTheDocument();
});
