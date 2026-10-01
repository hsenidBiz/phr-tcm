// The tester's own accounts: entering them, masking passwords, and what
// happens when the app refuses a save.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AccountsDialog from "./AccountsDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => { clearMocks(); vi.clearAllMocks(); });

function mount(existing: unknown[], onSave: (accounts: unknown[]) => unknown = () => []) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_list_accounts") return existing;
    if (cmd === "auto_run_save_accounts") return onSave((args as { accounts: unknown[] }).accounts);
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AccountsDialog onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

// Row fields are named by POSITION ("Key for account 1"), not by the
// account's own key: a name that changed while the key itself was being
// typed would make the field un-findable mid-edit.
test("existing accounts load with their passwords masked until asked", async () => {
  mount([{ key: "admin", label: "Administrator", username: "kim", password: "p1" }]);
  expect(await screen.findByDisplayValue("kim")).toBeInTheDocument();
  const pw = screen.getByLabelText("Password for account 1") as HTMLInputElement;
  expect(pw.type).toBe("password");
  fireEvent.click(screen.getByRole("checkbox", { name: "Show passwords" }));
  expect((screen.getByLabelText("Password for account 1") as HTMLInputElement).type).toBe("text");
});

test("adding an account and saving sends the whole list", async () => {
  const saved: unknown[][] = [];
  const onClose = mount([], (a) => { saved.push(a); return []; });
  await screen.findByText("No accounts yet.");
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  fireEvent.change(screen.getByLabelText("Key for account 1"), { target: { value: "hr.admin" } });
  fireEvent.change(screen.getByLabelText("Name for account 1"), { target: { value: "HR Admin" } });
  fireEvent.change(screen.getByLabelText("Username for account 1"), { target: { value: "kim" } });
  fireEvent.change(screen.getByLabelText("Password for account 1"), { target: { value: "p1" } });
  fireEvent.click(screen.getByRole("button", { name: "Save accounts" }));
  await waitFor(() => expect(saved).toEqual([[{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p1" }]]));
  expect(onClose).toHaveBeenCalled();
});

test("what the app refuses is shown and the dialog stays open", async () => {
  const onClose = mount([{ key: "admin", label: "A", username: "kim", password: "p" }], () => {
    throw new Error('the account "admin" has no username');
  });
  await screen.findByDisplayValue("kim");
  fireEvent.click(screen.getByRole("button", { name: "Save accounts" }));
  expect(await screen.findByText(/has no username/)).toBeInTheDocument();
  expect(onClose).not.toHaveBeenCalled();
});

test("removing an account takes its row away", async () => {
  mount([{ key: "admin", label: "A", username: "kim", password: "p" }]);
  await screen.findByDisplayValue("kim");
  fireEvent.click(screen.getByRole("button", { name: "Remove admin" }));
  expect(screen.queryByDisplayValue("kim")).not.toBeInTheDocument();
});

test("each field carries a visible label for when the row stacks, and keeps its accessible name", async () => {
  mount([{ key: "admin", label: "A", username: "kim", password: "p" }]);
  await screen.findByDisplayValue("kim");
  for (const [shown, name] of [
    ["Key", "Key for account 1"],
    ["Name", "Name for account 1"],
    ["Username", "Username for account 1"],
    ["Password", "Password for account 1"],
  ]) {
    const input = screen.getByLabelText(name);
    // The visible word sits in the same <label> as the field it names.
    const label = input.closest("label");
    expect(label).not.toBeNull();
    expect(within(label!).getByText(shown)).toBeInTheDocument();
  }
});

// ---- Proposed by the assistant -------------------------------------------

type Proposal = { key: string; label: string; username: string; role?: string | null };
type Saved = { key: string; label: string; username: string; password: string };

const PROPOSED: Proposal[] = [
  { key: "hr.admin", label: "HR Admin", username: "kim", role: "admin" },
  { key: "hr.sup", label: "Supervisor", username: "lee" },
];

/** A mount with an environment (with or without a default password), the
 * proposals the assistant left, and recorders for the proposal commands.
 * `addProposals` answers env_add_proposals; its default asks about nothing. */
function mountWithProposals(opts: {
  accounts?: Saved[];
  proposals?: Proposal[];
  hasDefault?: boolean;
  addProposals?: (args: { picks: Saved[]; replace: string[] }) => unknown;
}) {
  const state = { proposals: opts.proposals ?? PROPOSED, accounts: opts.accounts ?? [] };
  const adds: { picks: Saved[]; replace: string[] }[] = [];
  const log: string[] = [];
  mockIPC((cmd, args) => {
    log.push(cmd);
    if (cmd === "auto_run_list_accounts") return state.accounts;
    if (cmd === "env_list") {
      return {
        active: "qa",
        environments: [
          {
            id: "qa",
            name: "QA",
            start_url: "",
            allowed_origins: [],
            db_id: "db",
            test_environment: false,
            has_default_password: opts.hasDefault ?? true,
          },
        ],
      };
    }
    if (cmd === "env_proposals") return state.proposals;
    if (cmd === "env_dismiss_proposals") {
      state.proposals = [];
      return null;
    }
    if (cmd === "env_add_proposals") {
      const a = args as { picks: Saved[]; replace: string[] };
      adds.push(a);
      return opts.addProposals ? opts.addProposals(a) : [];
    }
    return null;
  });
  const onClose = vi.fn();
  const utils = render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AccountsDialog onClose={onClose} />
    </QueryClientProvider>,
  );
  return { onClose, adds, log, state, ...utils };
}

test("the proposals are listed under their heading, each saying where its password comes from", async () => {
  mountWithProposals({ hasDefault: true });
  expect(await screen.findByRole("heading", { name: "Proposed by the assistant (2)" })).toBeInTheDocument();
  expect(screen.getByText("hr.admin")).toBeInTheDocument();
  expect(screen.getByText("Supervisor")).toBeInTheDocument();
  expect(screen.getByLabelText("Password for proposed hr.admin")).toHaveAttribute("placeholder", "default password");
  expect(screen.getByLabelText("Password for proposed hr.sup")).toHaveAttribute("placeholder", "default password");
});

test("with no default password each proposal asks for one to be typed", async () => {
  mountWithProposals({ hasDefault: false });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  expect(screen.getByLabelText("Password for proposed hr.admin")).toHaveAttribute(
    "placeholder",
    "no default password set - type one",
  );
});

test("no proposal section at all when the assistant proposed nothing", async () => {
  mountWithProposals({ proposals: [] });
  await screen.findByText("No accounts yet.");
  expect(screen.queryByText(/Proposed by the assistant/)).not.toBeInTheDocument();
});

test("the proposals are read afresh each time the dialog opens", async () => {
  // ONE client across both opens: only a refetch on open can show the new one.
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const state = { proposals: [PROPOSED[0]] };
  mockIPC((cmd) => {
    if (cmd === "auto_run_list_accounts") return [];
    if (cmd === "env_proposals") return state.proposals;
    return null;
  });
  const open = () =>
    render(
      <QueryClientProvider client={client}>
        <AccountsDialog onClose={vi.fn()} />
      </QueryClientProvider>,
    );
  const first = open();
  expect(await screen.findByRole("heading", { name: "Proposed by the assistant (1)" })).toBeInTheDocument();
  first.unmount();
  // The assistant proposed one more while the dialog was shut.
  state.proposals = PROPOSED;
  open();
  expect(await screen.findByRole("heading", { name: "Proposed by the assistant (2)" })).toBeInTheDocument();
});

test("Add selected sends the picks with typed passwords (empty means the default) and brings them in as accounts", async () => {
  const m = mountWithProposals({});
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  const add = screen.getByRole("button", { name: "Add selected" });
  expect(add).toBeDisabled();

  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.sup" }));
  fireEvent.change(screen.getByLabelText("Password for proposed hr.sup"), { target: { value: "typed-1" } });
  m.state.accounts = [
    { key: "hr.admin", label: "HR Admin", username: "kim", password: "from-default" },
    { key: "hr.sup", label: "Supervisor", username: "lee", password: "typed-1" },
  ];
  m.state.proposals = [];
  fireEvent.click(add);

  await waitFor(() => expect(m.adds).toHaveLength(1));
  expect(m.adds[0]).toEqual({
    picks: [
      { key: "hr.admin", label: "HR Admin", username: "kim", password: "" },
      { key: "hr.sup", label: "Supervisor", username: "lee", password: "typed-1" },
    ],
    replace: [],
  });
  // They are accounts in the list now, and the proposal section is gone.
  expect(await screen.findByLabelText("Key for account 1")).toHaveValue("hr.admin");
  expect(screen.getByLabelText("Key for account 2")).toHaveValue("hr.sup");
  await waitFor(() => expect(screen.queryByText(/Proposed by the assistant/)).not.toBeInTheDocument());
});

test("a key that is already an account asks Replace per key, and only a confirmed key is replaced", async () => {
  const saved: Saved[] = [
    { key: "hr.admin", label: "Old admin", username: "old", password: "old-pw" },
    { key: "hr.sup", label: "Old sup", username: "old2", password: "old-pw2" },
  ];
  const m = mountWithProposals({
    accounts: saved,
    // First call: both keys exist and nothing is written; ask about both.
    addProposals: ({ replace }) => (replace.length === 0 ? ["hr.admin", "hr.sup"] : []),
  });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.sup" }));
  fireEvent.click(screen.getByRole("button", { name: "Add selected" }));

  expect(await screen.findByText("Replace hr.admin?")).toBeInTheDocument();
  expect(screen.getByText("Replace hr.sup?")).toBeInTheDocument();
  // Nothing is replaced until each is answered.
  expect(m.adds).toHaveLength(1);

  fireEvent.click(screen.getByRole("button", { name: "Replace hr.sup" }));
  fireEvent.click(screen.getByRole("button", { name: "Keep hr.admin" }));

  await waitFor(() => expect(m.adds).toHaveLength(2));
  expect(m.adds[1]).toEqual({
    picks: [{ key: "hr.sup", label: "Supervisor", username: "lee", password: "" }],
    replace: ["hr.sup"],
  });
});

test("keeping every key asks for nothing more and replaces nothing", async () => {
  const m = mountWithProposals({
    accounts: [{ key: "hr.admin", label: "Old", username: "old", password: "pw" }],
    proposals: [PROPOSED[0]],
    addProposals: () => ["hr.admin"],
  });
  await screen.findByRole("heading", { name: "Proposed by the assistant (1)" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  fireEvent.click(screen.getByRole("button", { name: "Add selected" }));
  await screen.findByText("Replace hr.admin?");
  fireEvent.click(screen.getByRole("button", { name: "Keep hr.admin" }));
  await waitFor(() => expect(screen.queryByText("Replace hr.admin?")).not.toBeInTheDocument());
  expect(m.adds).toHaveLength(1);
  // The proposal stays proposed, and the existing account is untouched.
  expect(screen.getByRole("heading", { name: "Proposed by the assistant (1)" })).toBeInTheDocument();
  expect(screen.getByLabelText("Username for account 1")).toHaveValue("old");
});

test("a refused add shows the sentence inline and keeps what was picked and typed", async () => {
  const m = mountWithProposals({
    hasDefault: false,
    addProposals: () => {
      throw new Error('"hr.sup": no default password set - type one');
    },
  });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.sup" }));
  fireEvent.change(screen.getByLabelText("Password for proposed hr.admin"), { target: { value: "typed-a" } });
  fireEvent.click(screen.getByRole("button", { name: "Add selected" }));

  expect(await screen.findByText('"hr.sup": no default password set - type one')).toBeInTheDocument();
  expect(m.adds).toHaveLength(1);
  // The selection and the typed password are still there to finish.
  expect(screen.getByRole("checkbox", { name: "Add hr.admin" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Add hr.sup" })).toBeChecked();
  expect(screen.getByLabelText("Password for proposed hr.admin")).toHaveValue("typed-a");
  expect(screen.getByRole("heading", { name: "Proposed by the assistant (2)" })).toBeInTheDocument();
});

test("Dismiss asks the app to drop the whole proposal", async () => {
  const m = mountWithProposals({});
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  await waitFor(() => expect(m.log).toContain("env_dismiss_proposals"));
  await waitFor(() => expect(screen.queryByText(/Proposed by the assistant/)).not.toBeInTheDocument());
});

test("a typed proposal password is masked, and shown only with Show passwords", async () => {
  mountWithProposals({});
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  const pw = screen.getByLabelText("Password for proposed hr.admin") as HTMLInputElement;
  expect(pw.type).toBe("password");
  fireEvent.click(screen.getByRole("checkbox", { name: "Show passwords" }));
  expect((screen.getByLabelText("Password for proposed hr.admin") as HTMLInputElement).type).toBe("text");
});
