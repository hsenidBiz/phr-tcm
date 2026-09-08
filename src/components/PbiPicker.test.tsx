import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test } from "vitest";
import type { PbiHit } from "../bindings";
import PbiPicker from "./PbiPicker";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function Harness() {
  const [pbi, setPbi] = useState<PbiHit | null>(null);
  return <PbiPicker org="acme" project="Web" pbi={pbi} onChange={setPbi} />;
}

function renderPicker() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <Harness />
    </QueryClientProvider>,
  );
}

test("picking a PBI records it; recents appear on focus", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "search_pbis" && (args as { query: string }).query === "login")
      return [{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }];
  });
  renderPicker();

  const input = screen.getByLabelText("Find PBI");
  fireEvent.change(input, { target: { value: "login" } });
  fireEvent.keyDown(input, { key: "Enter" });
  fireEvent.click(await screen.findByText(/Login flow/));

  // Chip shown; clear it.
  expect(screen.getByText("Login flow")).toBeInTheDocument();
  fireEvent.click(screen.getByLabelText("Clear PBI"));

  // Empty input focus lists the recent pick.
  fireEvent.focus(screen.getByLabelText("Find PBI"));
  expect(await screen.findByText("Recently used")).toBeInTheDocument();
  expect(screen.getByText(/Login flow/)).toBeInTheDocument();
});

test("clicking outside dismisses the results; clicking a result still picks", async () => {
  mockIPC((cmd) => {
    if (cmd === "search_pbis")
      return [{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }];
  });
  render(<div data-testid="elsewhere">background</div>);
  renderPicker();

  const input = screen.getByLabelText("Find PBI");
  fireEvent.change(input, { target: { value: "login" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(await screen.findByText(/Login flow/)).toBeInTheDocument();

  // A press on empty space closes it - the reported bug was that it didn't.
  fireEvent.pointerDown(screen.getByTestId("elsewhere"));
  expect(screen.queryByText(/Login flow/)).not.toBeInTheDocument();

  // Re-open and confirm the dismissal never eats a real selection: the
  // press lands INSIDE, so the click behind it still picks.
  fireEvent.keyDown(input, { key: "Enter" });
  const hit = await screen.findByText(/Login flow/);
  fireEvent.pointerDown(hit);
  fireEvent.click(hit);
  expect(screen.getByLabelText("Clear PBI")).toBeInTheDocument();
});

test("Escape still closes the results", async () => {
  mockIPC((cmd) => {
    if (cmd === "search_pbis")
      return [{ id: 42, title: "Login flow", work_item_type: "Product Backlog Item" }];
  });
  renderPicker();

  const input = screen.getByLabelText("Find PBI");
  fireEvent.change(input, { target: { value: "login" } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(await screen.findByText(/Login flow/)).toBeInTheDocument();

  fireEvent.keyDown(input, { key: "Escape" });
  expect(screen.queryByText(/Login flow/)).not.toBeInTheDocument();
});

/// A finished PBI otherwise squats in the recents until enough newer picks
/// push it out - the X drops it on the spot, without picking it.
test("a recent can be removed from the dropdown without picking it", async () => {
  mockIPC(() => undefined);
  localStorage.setItem(
    "tcm-v2-recent-pbis:acme/Web",
    JSON.stringify([
      { id: 42, title: "Login flow", work_item_type: "Product Backlog Item" },
      { id: 43, title: "Checkout", work_item_type: "Product Backlog Item" },
    ]),
  );
  renderPicker();

  fireEvent.focus(screen.getByLabelText("Find PBI"));
  await screen.findByText("Recently used");

  fireEvent.click(screen.getByLabelText("Remove #42 from recent PBIs"));

  // Gone from the list AND from storage; the other survives; nothing was
  // picked (no chip, dropdown still open on the remaining recent).
  expect(screen.queryByText(/Login flow/)).not.toBeInTheDocument();
  expect(screen.getByText(/Checkout/)).toBeInTheDocument();
  expect(screen.queryByLabelText("Clear PBI")).not.toBeInTheDocument();
  expect(JSON.parse(localStorage.getItem("tcm-v2-recent-pbis:acme/Web")!)).toHaveLength(1);
});
