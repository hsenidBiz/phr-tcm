// Waiting for you: nothing drawn while nothing waits; a row per item with
// its icon, title and Open; and what each Open does.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { WaitingItem } from "../../bindings";
import { clearAutoRunFocus, requestAutoRunFocus } from "../../lib/autoRunFocus";
import WaitingForYou from "./WaitingForYou";

afterEach(() => {
  clearMocks();
  clearAutoRunFocus();
  vi.clearAllMocks();
});

const ACCOUNT: WaitingItem = {
  key: "hr.sup",
  title: "Account hr.sup is proposed",
  proposed_ms: Date.now() - 5 * 60_000,
  target: { kind: "accounts" },
};
const SIGN_IN: WaitingItem = {
  key: "sign-in",
  title: "A sign-in is proposed",
  proposed_ms: 0,
  target: { kind: "sign_in" },
};
const divergence = (pbi: number | null): WaitingItem => ({
  key: "d1",
  title: "Case 5 needs a ruling",
  proposed_ms: Date.now() - 3_600_000,
  target: { kind: "divergence", id: "d1", case_id: 5, step: 2, pbi },
});

function mount(items: WaitingItem[], props: { pbiId?: number | null; cases?: number[]; onOpenPbi?: (n: number) => void } = {}) {
  mockIPC((cmd) => {
    if (cmd === "auto_run_waiting_items") return { env_id: "env-1", items };
    if (cmd === "auto_run_list_accounts") return [];
    if (cmd === "env_proposals")
      return [{ key: "hr.sup", label: "Supervisor", username: "sup", role: null, has_password: false, no_password: null, marks: [] }];
    if (cmd === "auto_run_load_divergences")
      return [{ id: "d1", case_id: 5, step: 2, case_says: "A message shows", app_shows: "Nothing shows", kind: "different", reported_at: "1", status: "open" }];
    return null;
  });
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <WaitingForYou
        org="acme"
        project="Web"
        pbiId={props.pbiId ?? 7}
        cases={props.cases ?? []}
        onOpenPbi={props.onOpenPbi}
      />
    </QueryClientProvider>,
  );
}

test("nothing waiting draws nothing", async () => {
  const { container } = mount([]);
  await new Promise((r) => setTimeout(r, 20));
  expect(container.firstChild).toBeNull();
});

test("each row has an icon, its title and Open", async () => {
  const { container } = mount([ACCOUNT, SIGN_IN, divergence(7)]);
  expect(await screen.findByRole("heading", { name: "Waiting for you" })).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: /^Open: / })).toHaveLength(3);
  expect(screen.getByText("1 proposed account is waiting for you to accept")).toBeInTheDocument();
  expect(screen.getByText("5m ago")).toBeInTheDocument();
  expect(container.querySelectorAll("li svg")).toHaveLength(6);
});

test("Open on an account opens Accounts on the Proposed tab", async () => {
  mount([ACCOUNT]);
  fireEvent.click(await screen.findByRole("button", { name: "Open: 1 proposed account is waiting for you to accept" }));
  expect(await screen.findByRole("heading", { name: /Proposed by the assistant \(1\)/ })).toBeInTheDocument();
});

test("Open on a divergence under another PBI switches PBI first", async () => {
  const onOpenPbi = vi.fn();
  mount([divergence(42)], { pbiId: 7, onOpenPbi });
  fireEvent.click(await screen.findByRole("button", { name: "Open: Case 5 needs a ruling" }));
  expect(onOpenPbi).toHaveBeenCalledWith(42);
  const { useAutoRunFocus } = await import("../../lib/autoRunFocus");
  const { renderHook } = await import("@testing-library/react");
  const held = renderHook(() => useAutoRunFocus());
  expect(held.result.current).toEqual({ kind: "divergence", id: "d1", caseId: 5, pbi: 42 });
});

test("a divergence with no PBI says it is not under the selected PBI", async () => {
  mount([divergence(null)], { cases: [] });
  fireEvent.click(await screen.findByRole("button", { name: "Open: Case 5 needs a ruling" }));
  expect(await screen.findByText("This case is not under the selected PBI.")).toBeInTheDocument();
  expect(await screen.findByText("Nothing shows")).toBeInTheDocument();
});

test("a bell click on the sign-in expands it under its row", async () => {
  mount([SIGN_IN]);
  await screen.findByRole("heading", { name: "Waiting for you" });
  act(() => requestAutoRunFocus({ kind: "sign-in" }));
  await waitFor(() => expect(screen.getByRole("button", { name: /^Open: / })).toHaveAttribute("aria-expanded", "true"));
});

test("many proposed accounts are one row that names none of them", async () => {
  const more = (key: string): WaitingItem => ({ ...ACCOUNT, key, title: `Account ${key} is proposed` });
  mount([ACCOUNT, more("abigail"), more("aiden"), divergence(7)]);
  expect(await screen.findByText("3 proposed accounts are waiting for you to accept")).toBeInTheDocument();
  expect(screen.getAllByRole("listitem")).toHaveLength(2);
  expect(screen.queryByText(/abigail|aiden|hr\.sup/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Open: 3 proposed accounts are waiting for you to accept" }));
  expect(await screen.findByRole("heading", { name: /Proposed by the assistant/ })).toBeInTheDocument();
});
