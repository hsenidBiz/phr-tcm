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
