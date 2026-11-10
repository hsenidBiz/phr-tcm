// The assistant's proposed sign-in: nothing without one, the parts it
// changes marked, the reset tick box only when offered, and Approve /
// Reject reaching the right commands.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { SignInProposalView } from "../../bindings";
import { toast } from "../../lib/toast";
import SignInProposalNotice from "./SignInProposalNotice";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const VIEW: SignInProposalView = {
  address: "/login",
  steps: ['Type into textbox "Email"', 'Click button "Sign in"'],
  signed_in: 'link "Kim"',
  signed_in_in_menu: true,
  password_reset: null,
  account: "sup",
  changes: ["steps"],
  offer_reset_switch: false,
};

const STALE = "the sign-in recipe changed after the assistant proposed this one. Ask for a new proposal.";

type Calls = { cmd: string; args: Record<string, unknown> }[];

function mount(proposal: SignInProposalView | null, approve: () => unknown = () => null) {
  const calls: Calls = [];
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    if (cmd === "auto_run_sign_in_proposal") return proposal;
    calls.push({ cmd: String(cmd), args: a });
    if (cmd === "auto_run_approve_sign_in_proposal") return approve();
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <SignInProposalNotice org="acme" project="Web" />
    </QueryClientProvider>,
  );
  return calls;
}

test("renders nothing when there is no proposal", async () => {
  mount(null);
  await new Promise((r) => setTimeout(r, 30));
  expect(screen.queryByLabelText("Proposed sign-in")).toBeNull();
});

test("shows the steps, the menu note, the account and a changes mark", async () => {
  mount(VIEW);
  expect(await screen.findByText("Proposed by the assistant")).toBeInTheDocument();
  expect(screen.getByText('Type into textbox "Email"')).toBeInTheDocument();
  expect(screen.getByText('Click button "Sign in"')).toBeInTheDocument();
  expect(screen.getByText(/inside a menu/)).toBeInTheDocument();
  expect(screen.getByText(/Tried with sup/)).toBeInTheDocument();
  expect(screen.getByTestId("changes-steps")).toHaveTextContent("changes");
  expect(screen.queryByTestId("changes-address")).toBeNull();
});

test("no reset tick box unless offered", async () => {
  mount(VIEW);
  await screen.findByText("Proposed by the assistant");
  expect(screen.queryByRole("checkbox", { name: "Also turn on Reset expired passwords" })).toBeNull();
});

test("Approve passes the tick box: true when ticked, false when not", async () => {
  const calls = mount({ ...VIEW, offer_reset_switch: true });
  const box = await screen.findByRole("checkbox", { name: "Also turn on Reset expired passwords" });
  expect(box).not.toBeChecked();
  fireEvent.click(box);
  fireEvent.click(screen.getByRole("button", { name: "Approve" }));
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "auto_run_approve_sign_in_proposal")?.args).toEqual({
      organization: "acme",
      project: "Web",
      turnOnReset: true,
    }),
  );
});

test("Approve without ticking sends false", async () => {
  const calls = mount({ ...VIEW, offer_reset_switch: true });
  await screen.findByRole("checkbox", { name: "Also turn on Reset expired passwords" });
  fireEvent.click(screen.getByRole("button", { name: "Approve" }));
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "auto_run_approve_sign_in_proposal")?.args.turnOnReset).toBe(false),
  );
});

test("Reject calls the reject command", async () => {
  const calls = mount(VIEW);
  await screen.findByText("Proposed by the assistant");
  fireEvent.click(screen.getByRole("button", { name: "Reject" }));
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "auto_run_reject_sign_in_proposal")?.args).toEqual({
      organization: "acme",
      project: "Web",
    }),
  );
});

test("a stale Approve shows its sentence in a toast and keeps the notice", async () => {
  mount(VIEW, () => {
    throw STALE;
  });
  await screen.findByText("Proposed by the assistant");
  fireEvent.click(screen.getByRole("button", { name: "Approve" }));
  await waitFor(() => expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("changed after the assistant proposed")));
  expect(screen.getByText("Proposed by the assistant")).toBeInTheDocument();
});
