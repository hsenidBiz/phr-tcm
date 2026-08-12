// Moving cases to a different PBI: pick a destination by search, confirm
// against the full case list, and get told exactly what moved.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { TestCaseFull } from "../bindings";
import RelinkDialog from "./RelinkDialog";

afterEach(() => {
  clearMocks();
  vi.restoreAllMocks();
});

function tc(id: number, title: string): TestCaseFull {
  return {
    id,
    title,
    tags: "",
    automation_status: "Not Automated",
    steps: [],
    step_ids: [],
    steps_xml: "",
    module_value: "",
    preconditions: "",
  };
}

function mount(opts: {
  outcomes?: { id: number; moved: boolean; error: null }[];
  onMoved?: () => void;
  onClose?: () => void;
}) {
  const calls: { ids: number[]; fromPbi: number; toPbi: number }[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "search_pbis")
      return [
        { id: 100, title: "The wrong PBI", work_item_type: "Product Backlog Item" },
        { id: 200, title: "The right PBI", work_item_type: "Product Backlog Item" },
      ];
    if (cmd === "relink_test_cases") {
      const a = args as { ids: number[]; fromPbi: number; toPbi: number };
      calls.push({ ids: a.ids, fromPbi: a.fromPbi, toPbi: a.toPbi });
      return opts.outcomes ?? a.ids.map((id) => ({ id, moved: true, error: null }));
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <RelinkDialog
        org="acme"
        project="Web"
        fromPbi={100}
        cases={[tc(1, "Valid login"), tc(2, "Locked account")]}
        onClose={opts.onClose ?? (() => {})}
        onMoved={opts.onMoved ?? (() => {})}
      />
    </QueryClientProvider>,
  );
  return calls;
}

test("the current PBI is not offered as a destination", async () => {
  mount({});
  fireEvent.change(screen.getByLabelText("Search for the destination PBI"), {
    target: { value: "PBI" },
  });
  // Debounce (250ms) then results: only #200 shows; moving to the PBI the
  // cases already live in would be a no-op dressed as an action.
  expect(await screen.findByText("The right PBI", {}, { timeout: 3000 })).toBeInTheDocument();
  expect(screen.queryByText("The wrong PBI")).not.toBeInTheDocument();
});

test("picking a destination shows every case, and Move sends exactly those ids", async () => {
  const onMoved = vi.fn();
  const onClose = vi.fn();
  const calls = mount({ onMoved, onClose });

  fireEvent.change(screen.getByLabelText("Search for the destination PBI"), {
    target: { value: "right" },
  });
  fireEvent.click(await screen.findByText("The right PBI", {}, { timeout: 3000 }));

  // The confirmation lists the cases by id and title - checkable, not a count.
  expect(screen.getByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Locked account")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /Move 2 to #200/ }));
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(calls[0]).toEqual({ ids: [1, 2], fromPbi: 100, toPbi: 200 });
  await waitFor(() => expect(onMoved).toHaveBeenCalled());
  expect(onClose).toHaveBeenCalled();
});

test("a partial move stays open and names what was left behind", async () => {
  const onMoved = vi.fn();
  const onClose = vi.fn();
  mount({
    outcomes: [
      { id: 1, moved: true, error: null },
      { id: 2, moved: false, error: null },
    ],
    onMoved,
    onClose,
  });

  fireEvent.change(screen.getByLabelText("Search for the destination PBI"), {
    target: { value: "right" },
  });
  fireEvent.click(await screen.findByText("The right PBI", {}, { timeout: 3000 }));
  fireEvent.click(screen.getByRole("button", { name: /Move 2 to #200/ }));

  // The failure view names the stranded case; the dialog does not close
  // over it, and what DID move still triggers the refresh.
  expect(await screen.findByText("1 could not be moved")).toBeInTheDocument();
  expect(screen.getByText("#2")).toBeInTheDocument();
  expect(onMoved).toHaveBeenCalled();
  expect(onClose).not.toHaveBeenCalled();
});

test("Move is dead until a destination is picked", () => {
  mount({});
  expect(screen.getByRole("button", { name: /Pick a PBI first/ })).toBeDisabled();
});
