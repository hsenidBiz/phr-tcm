// The person's rulings on a reported divergence, on the case card.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { Divergence } from "../../../src/bindings";
import { DivergenceRow } from "../../../src/screens/AutoRun/DivergenceMark";

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

function show(d: Divergence, caseId = 147556) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "auto_run_rule_divergence" || cmd === "auto_run_undo_divergence_ruling") return [];
    return null;
  });
  const qc = new QueryClient();
  render(
    <QueryClientProvider client={qc}>
      <DivergenceRow org="acme" project="Web" caseId={caseId} divergence={d} />
    </QueryClientProvider>,
  );
  return calls;
}

test("an open data divergence asks which is right and offers three rulings", () => {
  show({ ...base, step: 3, case_id: 12 }, 12);
  expect(screen.getByText("Needs a ruling")).toBeInTheDocument();
  // The badge explains the word it uses.
  expect(screen.getByText("Needs a ruling")).toHaveAttribute("data-term", "ruling");
  expect(screen.getByText("The case and the app disagree at step 3. Which is right?")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "The app is wrong for #12 step 3" })).toHaveTextContent("The app is wrong");
  expect(screen.getByRole("button", { name: "The app is right, update case #12 for step 3" })).toHaveTextContent(
    "The app is right, update the case",
  );
  expect(screen.getByRole("button", { name: "I fixed the data for #12 step 3" })).toHaveTextContent("I fixed the data");
});

test("a reporter's proposal is shown with the divergence", () => {
  show({ ...base, proposal: "Seed 2 levels" });
  expect(screen.getByText(/Proposed: "Seed 2 levels"/)).toBeInTheDocument();
});

test("an app divergence asks no question and offers no data fix", () => {
  show({ ...base, kind: "app" });
  expect(screen.getByText("Divergence")).toBeInTheDocument();
  expect(screen.getByText("Divergence")).toHaveAttribute("data-term", "divergence");
  expect(screen.queryByText(/Which is right/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /The app is wrong/ })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /The app is right, update the case|The app is right, update case/ })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /I fixed the data/ })).not.toBeInTheDocument();
});

test("confirming the app is wrong calls the command with case_stands", async () => {
  const calls = show(base);
  fireEvent.click(screen.getByRole("button", { name: /The app is wrong/ }));
  expect(
    screen.getByText("The script keeps the case's value at step 2, so the run fails there until the app is fixed."),
  ).toBeInTheDocument();
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
  fireEvent.click(screen.getByRole("button", { name: /The app is right, update case #147556/ }));
  expect(
    screen.getByText(
      "Azure DevOps opens on the case so you can update it; the case waits until its script is repaired to match.",
    ),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "auto_run_rule_divergence")).toBe(true));
  expect(opener.openUrl.mock.calls[0][0]).toMatch(/\/_workitems\/edit\/147556$/);
  expect((calls.find((c) => c.cmd === "auto_run_rule_divergence")?.args as { ruling: string }).ruling).toBe("rewrite");
});

test("I fixed the data confirms with its own sentence and sends data_fixed", async () => {
  const calls = show(base);
  fireEvent.click(screen.getByRole("button", { name: /I fixed the data/ }));
  expect(screen.getByText("You changed the environment's data to match the case; the case runs again.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() =>
    expect((calls.find((c) => c.cmd === "auto_run_rule_divergence")?.args as { ruling: string }).ruling).toBe("data_fixed"),
  );
});

test("keep closes the confirm without ruling", () => {
  const calls = show(base);
  fireEvent.click(screen.getByRole("button", { name: /I fixed the data/ }));
  fireEvent.click(screen.getByRole("button", { name: "Keep" }));
  expect(screen.queryByRole("button", { name: "Confirm" })).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "auto_run_rule_divergence")).toBe(false);
});

test("a case waiting for a repair offers no ruling, only to undo it, and names heal", () => {
  show({ ...base, status: "awaiting_repair" });
  expect(screen.getByText("Waits for a repair")).toBeInTheDocument();
  expect(screen.getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
    "Undo the ruling on #147556 step 2",
  ]);
  expect(screen.getByText(/\/tcm:heal/)).toBeInTheDocument();
});

test("undoing the rewrite ruling calls the command and asks for nothing else", async () => {
  const calls = show({ ...base, status: "awaiting_repair" });
  fireEvent.click(screen.getByRole("button", { name: "Undo the ruling on #147556 step 2" }));
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "auto_run_undo_divergence_ruling")?.args).toEqual({
      organization: "acme",
      project: "Web",
      id: "d1",
    }),
  );
  expect(calls.some((c) => c.cmd === "auto_run_rule_divergence")).toBe(false);
  expect(opener.openUrl).not.toHaveBeenCalled();
});

test("a failed undo says why and stays", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_undo_divergence_ruling") throw "only a ruling that waits for a repair can be undone";
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DivergenceRow org="acme" project="Web" caseId={147556} divergence={{ ...base, status: "awaiting_repair" }} />
    </QueryClientProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Undo the ruling on #147556 step 2" }));
  expect(await screen.findByText("only a ruling that waits for a repair can be undone")).toBeInTheDocument();
});

const byAssistant: Divergence = {
  ...base,
  status: "ruled",
  ruling: "case_stands",
  ruled_by: "assistant",
  said: "a different cycle name is fine",
};

test("an assistant's ruling shows the person's words and offers undo", () => {
  show(byAssistant);
  expect(screen.getByText('Ruled by the assistant, on your words: "a different cycle name is fine"')).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Undo the ruling on #147556 step 2" })).toBeInTheDocument();
});

test("an assistant's ruling shows its note after the person's words", () => {
  show({ ...byAssistant, note: "Cycle names are free text." });
  expect(
    screen.getByText(
      'Ruled by the assistant, on your words: "a different cycle name is fine" Its note: Cycle names are free text.',
    ),
  ).toBeInTheDocument();
});

test("undoing an assistant's ruling calls the undo command", async () => {
  const calls = show(byAssistant);
  fireEvent.click(screen.getByRole("button", { name: "Undo the ruling on #147556 step 2" }));
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "auto_run_undo_divergence_ruling")?.args).toEqual({
      organization: "acme",
      project: "Web",
      id: "d1",
    }),
  );
});

test("an assistant's rewrite that a repair has closed shows its words and offers no undo", () => {
  show({ ...byAssistant, ruling: "rewrite" });
  expect(screen.getByText('Ruled by the assistant, on your words: "a different cycle name is fine"')).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /Undo the ruling/ })).not.toBeInTheDocument();
});

test("an assistant's rewrite that still waits for its repair offers undo", () => {
  show({ ...byAssistant, status: "awaiting_repair", ruling: "rewrite" });
  expect(screen.getByRole("button", { name: "Undo the ruling on #147556 step 2" })).toBeInTheDocument();
});

test("a person's own ruling offers no undo", () => {
  show({ ...byAssistant, ruled_by: null, said: null });
  expect(screen.queryByRole("button", { name: /Undo the ruling/ })).not.toBeInTheDocument();
  expect(screen.queryByText(/Ruled by the assistant/)).not.toBeInTheDocument();
});

test("a compact row waiting on a repair keeps its badge and /tcm:heal line without Details", () => {
  const qc = new QueryClient();
  render(
    <QueryClientProvider client={qc}>
      <DivergenceRow
        org="acme"
        project="Web"
        caseId={147556}
        divergence={{ ...byAssistant, status: "awaiting_repair", ruling: "rewrite" }}
        compact
      />
    </QueryClientProvider>,
  );
  expect(screen.getByText("Waits for a repair")).toBeInTheDocument();
  expect(screen.getByText(/Ask the assistant to repair the script \(\/tcm:heal\)\./)).toBeInTheDocument();
});

test("a compact row says Step N by default and a given tag instead", () => {
  const qc = new QueryClient();
  const row = (tag?: string) => (
    <QueryClientProvider client={qc}>
      <DivergenceRow org="acme" project="Web" caseId={147556} divergence={{ ...byAssistant, ruling: "rewrite" }} compact tag={tag} />
    </QueryClientProvider>
  );
  const { rerender } = render(row());
  expect(screen.getByText("Step 2")).toBeInTheDocument();
  rerender(row("Ruling"));
  expect(screen.getByText("Ruling")).toBeInTheDocument();
  expect(screen.queryByText("Step 2")).not.toBeInTheDocument();
});
