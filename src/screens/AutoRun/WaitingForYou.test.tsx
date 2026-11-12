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
function mount(items: WaitingItem[]) {
  mockIPC((cmd) => {
    if (cmd === "auto_run_waiting_items") return { env_id: "env-1", items };
    if (cmd === "auto_run_list_accounts") return [];
    if (cmd === "env_proposals")
      return [{ key: "hr.sup", label: "Supervisor", username: "sup", role: null, has_password: false, no_password: null, marks: [] }];
    return null;
  });
  return render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <WaitingForYou org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

test("nothing waiting draws nothing", async () => {
  const { container } = mount([]);
  await new Promise((r) => setTimeout(r, 20));
  expect(container.firstChild).toBeNull();
});

test("each row has an icon, its title and Open", async () => {
  const { container } = mount([ACCOUNT, SIGN_IN]);
  expect(await screen.findByRole("heading", { name: "Waiting for you" })).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: /^Open: / })).toHaveLength(2);
  expect(screen.getByText("1 proposed account is waiting for you to accept")).toBeInTheDocument();
  expect(screen.getByText("5m ago")).toBeInTheDocument();
  expect(container.querySelectorAll("li svg")).toHaveLength(4);
});

test("Open on an account opens Accounts on the Proposed tab", async () => {
  mount([ACCOUNT]);
  fireEvent.click(await screen.findByRole("button", { name: "Open: 1 proposed account is waiting for you to accept" }));
  expect(await screen.findByRole("heading", { name: /Proposed by the assistant \(1\)/ })).toBeInTheDocument();
});

test("accounts and a sign-in show no divergence text", async () => {
  mount([ACCOUNT, SIGN_IN]);
  await screen.findByRole("heading", { name: "Waiting for you" });
  expect(screen.queryByText(/differs|ruling|divergence|not under the selected PBI/i)).toBeNull();
});

test("a bell click on the sign-in expands it under its row", async () => {
  mount([SIGN_IN]);
  await screen.findByRole("heading", { name: "Waiting for you" });
  act(() => requestAutoRunFocus({ kind: "sign-in" }));
  await waitFor(() => expect(screen.getByRole("button", { name: /^Open: / })).toHaveAttribute("aria-expanded", "true"));
});

test("many proposed accounts are one row that names none of them", async () => {
  const more = (key: string): WaitingItem => ({ ...ACCOUNT, key, title: `Account ${key} is proposed` });
  mount([ACCOUNT, more("abigail"), more("aiden"), SIGN_IN]);
  expect(await screen.findByText("3 proposed accounts are waiting for you to accept")).toBeInTheDocument();
  expect(screen.getAllByRole("listitem")).toHaveLength(2);
  expect(screen.queryByText(/abigail|aiden|hr\.sup/)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Open: 3 proposed accounts are waiting for you to accept" }));
  expect(await screen.findByRole("heading", { name: /Proposed by the assistant/ })).toBeInTheDocument();
});
