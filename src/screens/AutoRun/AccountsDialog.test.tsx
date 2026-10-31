// The tester's own accounts: entering them, masking passwords, and what
// happens when the app refuses a save.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import AccountsDialog, { reseedRows } from "./AccountsDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
// Whether a run or a discovery has the Auto Run browser, as the tests say.
vi.mock("../../lib/backgroundRun", async (original) => ({
  ...(await original<typeof import("../../lib/backgroundRun")>()),
  runIsGoing: () => checkFixture.taken,
}));
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
  fireEvent.change(screen.getByLabelText("Name for account 1"), { target: { value: "Changed" } });
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

type Proposal = { key: string; label: string; username: string; role?: string | null; has_password?: boolean; no_password?: string | null; marks?: string[] };
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
    if (cmd === "env_proposals") return state.proposals?.map((p) => ({ marks: [], ...p }));
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
  expect(screen.getAllByText("Default password")).toHaveLength(2);
  // No password field until one is asked for.
  expect(screen.queryByLabelText("Password for proposed hr.admin")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Set password for hr.admin" })).toBeInTheDocument();
});

test("with no default password each proposal needs one, and shows its field open from the start", async () => {
  mountWithProposals({ hasDefault: false });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  expect(screen.getAllByText("Needs a password")).toHaveLength(2);
  expect(screen.getByLabelText("Password for proposed hr.admin")).toHaveValue("");
  expect(screen.getByLabelText("Password for proposed hr.sup")).toHaveValue("");
  // Nothing to fall back on, so nothing to set or to go back to.
  expect(screen.queryByRole("button", { name: /^Set password/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /^Use (database|default) password/ })).not.toBeInTheDocument();
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
    if (cmd === "env_proposals") return state.proposals?.map((p) => ({ marks: [], ...p }));
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
  const add = screen.getByRole("button", { name: "Add selected (0)" });
  expect(add).toBeDisabled();

  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.sup" }));
  fireEvent.click(screen.getByRole("button", { name: "Set password for hr.sup" }));
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
  fireEvent.click(screen.getByRole("button", { name: /^Add selected/ }));

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
  fireEvent.click(screen.getByRole("button", { name: /^Add selected/ }));
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
  fireEvent.click(screen.getByRole("button", { name: /^Add selected/ }));

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
  fireEvent.click(screen.getByRole("button", { name: "Set password for hr.admin" }));
  const pw = screen.getByLabelText("Password for proposed hr.admin") as HTMLInputElement;
  expect(pw.type).toBe("password");
  fireEvent.click(screen.getByRole("checkbox", { name: "Show passwords" }));
  expect((screen.getByLabelText("Password for proposed hr.admin") as HTMLInputElement).type).toBe("text");
});

// ---- A password the assistant read from the database ----------------------

const FROM_DB: Proposal[] = [
  { key: "hr.admin", label: "HR Admin", username: "kim", role: "admin", has_password: true },
  { key: "hr.sup", label: "Supervisor", username: "lee", has_password: false },
];

test("a proposal with a database password says so, and its field can still override it", async () => {
  mountWithProposals({ proposals: FROM_DB, hasDefault: true });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  expect(screen.getAllByText("From database")).toHaveLength(1);
  // The one without falls back on the environment's default.
  expect(screen.getAllByText("Default password")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Set password for hr.admin" }));
  expect(screen.getByLabelText("Password for proposed hr.admin")).toHaveValue("");
  fireEvent.change(screen.getByLabelText("Password for proposed hr.admin"), { target: { value: "mine" } });
  expect(screen.getByText("Password set")).toBeInTheDocument();
  expect(screen.queryByText("From database")).not.toBeInTheDocument();
});

test("Add selected with the field blank sends no password, and the account comes in", async () => {
  const m = mountWithProposals({ proposals: FROM_DB, hasDefault: false });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  m.state.accounts = [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "set-in-rust" }];
  m.state.proposals = [FROM_DB[1]];
  fireEvent.click(screen.getByRole("button", { name: /^Add selected/ }));

  await waitFor(() => expect(m.adds).toHaveLength(1));
  expect(m.adds[0]).toEqual({
    picks: [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "" }],
    replace: [],
  });
  expect(await screen.findByLabelText("Key for account 1")).toHaveValue("hr.admin");
  await waitFor(() => expect(screen.queryByText("From database")).not.toBeInTheDocument());
});

test("a password typed over a database one is the one sent", async () => {
  const m = mountWithProposals({ proposals: FROM_DB });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  fireEvent.click(screen.getByRole("button", { name: "Set password for hr.admin" }));
  fireEvent.change(screen.getByLabelText("Password for proposed hr.admin"), { target: { value: "typed-over" } });
  fireEvent.click(screen.getByRole("button", { name: /^Add selected/ }));
  await waitFor(() => expect(m.adds).toHaveLength(1));
  expect(m.adds[0].picks).toEqual([{ key: "hr.admin", label: "HR Admin", username: "kim", password: "typed-over" }]);
});

test("Set password then Use database password clears what was typed and closes the field", async () => {
  const m = mountWithProposals({ proposals: FROM_DB, hasDefault: true });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  fireEvent.click(screen.getByRole("button", { name: "Set password for hr.admin" }));
  fireEvent.change(screen.getByLabelText("Password for proposed hr.admin"), { target: { value: "typed" } });
  fireEvent.click(screen.getByRole("button", { name: "Use database password for hr.admin" }));
  expect(screen.queryByLabelText("Password for proposed hr.admin")).not.toBeInTheDocument();
  expect(screen.getByText("From database")).toBeInTheDocument();
  // Opened again, the field is empty: the typed value is gone, not hidden.
  fireEvent.click(screen.getByRole("button", { name: "Set password for hr.admin" }));
  expect(screen.getByLabelText("Password for proposed hr.admin")).toHaveValue("");

  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  fireEvent.click(screen.getByRole("button", { name: /^Add selected/ }));
  await waitFor(() => expect(m.adds).toHaveLength(1));
  expect(m.adds[0].picks).toEqual([{ key: "hr.admin", label: "HR Admin", username: "kim", password: "" }]);
});

test("the reason the assistant gave for having no password shows beside the account, and no password does", async () => {
  mountWithProposals({
    proposals: [
      { key: "hr.admin", label: "HR Admin", username: "kim", has_password: true, no_password: null, marks: [] },
      { key: "hr.sup", label: "Supervisor", username: "lee", has_password: false, no_password: "the database holds only a hash", marks: ["expired"] },
    ],
    hasDefault: false,
  });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  const row = screen.getByText("hr.sup").closest("li")!;
  expect(within(row).getByText("No password from the assistant: the database holds only a hash")).toBeInTheDocument();
  // What the site's queries said of a login shows beside it; an unmarked one has no such line.
  expect(within(row).getByText("Checked against the site: expired")).toBeInTheDocument();
  expect(within(screen.getByText("hr.admin").closest("li")!).queryByText(/Checked against the site/)).not.toBeInTheDocument();
  // The account with a password has no reason line, and nothing reveals a password.
  const other = screen.getByText("hr.admin").closest("li")!;
  expect(within(other).queryByText(/No password from the assistant/)).not.toBeInTheDocument();
  expect(within(other).getByText("From database")).toBeInTheDocument();
  expect(within(row).getByLabelText("Password for proposed hr.sup")).toHaveValue("");
  expect(document.body.textContent).not.toMatch(/\$2a\$|Db-Pw/);
});

test("a login without a database password goes back to the default with Use default password", async () => {
  mountWithProposals({ proposals: FROM_DB, hasDefault: true });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  fireEvent.click(screen.getByRole("button", { name: "Set password for hr.sup" }));
  fireEvent.change(screen.getByLabelText("Password for proposed hr.sup"), { target: { value: "typed" } });
  fireEvent.click(screen.getByRole("button", { name: "Use default password for hr.sup" }));
  expect(screen.queryByLabelText("Password for proposed hr.sup")).not.toBeInTheDocument();
  expect(screen.getByText("Default password")).toBeInTheDocument();
});

test("a login that needs a password shows its field open while the others wait for Set password", async () => {
  mountWithProposals({ proposals: FROM_DB, hasDefault: false });
  await screen.findByRole("heading", { name: "Proposed by the assistant (2)" });
  expect(screen.getByText("From database")).toBeInTheDocument();
  expect(screen.getByText("Needs a password")).toBeInTheDocument();
  expect(screen.getByLabelText("Password for proposed hr.sup")).toBeInTheDocument();
  expect(screen.queryByLabelText("Password for proposed hr.admin")).not.toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Password for proposed hr.sup"), { target: { value: "typed" } });
  expect(screen.getByText("Password set")).toBeInTheDocument();
});

// ---- Searching and selecting ----------------------------------------------

const MANY: Proposal[] = [
  { key: "hr.admin", label: "HR Admin", username: "kim", role: "E001", has_password: true },
  { key: "hr.sup", label: "Supervisor", username: "lee", role: "E002", has_password: true },
  { key: "pay.clerk", label: "Payroll Clerk", username: "ann", role: "E003", has_password: true },
];

const search = (text: string) =>
  fireEvent.change(screen.getByRole("textbox", { name: "Search proposed logins" }), { target: { value: text } });

test("the search box filters by name, key, role and username, and says when nothing matches", async () => {
  mountWithProposals({ proposals: MANY });
  await screen.findByRole("heading", { name: "Proposed by the assistant (3)" });
  expect(screen.getByRole("textbox", { name: "Search proposed logins" })).toHaveAttribute("placeholder", "Search logins");

  search("payroll");
  expect(screen.getByRole("checkbox", { name: "Add pay.clerk" })).toBeInTheDocument();
  expect(screen.queryByRole("checkbox", { name: "Add hr.admin" })).not.toBeInTheDocument();

  search("HR.");
  expect(screen.getByRole("checkbox", { name: "Add hr.admin" })).toBeInTheDocument();
  expect(screen.getByRole("checkbox", { name: "Add hr.sup" })).toBeInTheDocument();
  expect(screen.queryByRole("checkbox", { name: "Add pay.clerk" })).not.toBeInTheDocument();

  search("e002");
  expect(screen.getByRole("checkbox", { name: "Add hr.sup" })).toBeInTheDocument();
  expect(screen.queryByRole("checkbox", { name: "Add hr.admin" })).not.toBeInTheDocument();

  search("ann");
  expect(screen.getByRole("checkbox", { name: "Add pay.clerk" })).toBeInTheDocument();
  expect(screen.queryByRole("checkbox", { name: "Add hr.sup" })).not.toBeInTheDocument();

  search("nobody");
  expect(screen.getByText('No logins match "nobody".')).toBeInTheDocument();
  // The heading still counts every proposal.
  expect(screen.getByRole("heading", { name: "Proposed by the assistant (3)" })).toBeInTheDocument();
});

test("Select all shown ticks only the shown logins, and the picks stay across a search change", async () => {
  const m = mountWithProposals({ proposals: MANY });
  await screen.findByRole("heading", { name: "Proposed by the assistant (3)" });
  search("hr.");
  fireEvent.click(screen.getByRole("checkbox", { name: "Select all shown" }));
  expect(screen.getByRole("checkbox", { name: "Add hr.admin" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Add hr.sup" })).toBeChecked();
  expect(screen.getByText("2 selected")).toBeInTheDocument();

  search("");
  expect(screen.getByRole("checkbox", { name: "Add hr.admin" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Add hr.sup" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Add pay.clerk" })).not.toBeChecked();
  // Some but not all of what is shown: the box says so.
  expect(screen.getByRole("checkbox", { name: "Select all shown" })).toHaveAttribute("aria-checked", "mixed");

  // Clearing under a search clears only what it shows.
  search("supervisor");
  expect(screen.getByRole("checkbox", { name: "Select all shown" })).toBeChecked();
  fireEvent.click(screen.getByRole("checkbox", { name: "Select all shown" }));
  search("");
  expect(screen.getByRole("checkbox", { name: "Add hr.admin" })).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Add hr.sup" })).not.toBeChecked();

  fireEvent.click(screen.getByRole("button", { name: "Add selected (1)" }));
  await waitFor(() => expect(m.adds).toHaveLength(1));
  expect(m.adds[0].picks.map((p) => p.key)).toEqual(["hr.admin"]);
});

test("Add selected counts the picks", async () => {
  mountWithProposals({ proposals: MANY });
  await screen.findByRole("heading", { name: "Proposed by the assistant (3)" });
  expect(screen.getByRole("button", { name: "Add selected (0)" })).toBeDisabled();
  expect(screen.getByText("0 selected")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  expect(screen.getByRole("button", { name: "Add selected (1)" })).toBeEnabled();
  fireEvent.click(screen.getByRole("checkbox", { name: "Add pay.clerk" }));
  expect(screen.getByRole("button", { name: "Add selected (2)" })).toBeEnabled();
  expect(screen.getByText("2 selected")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("checkbox", { name: "Add hr.admin" }));
  expect(screen.getByRole("button", { name: "Add selected (1)" })).toBeEnabled();
});

// ---- Check sign-in -------------------------------------------------------

const checkFixture = {
  taken: false,
};

type Seen = { cmd: string; args: Record<string, unknown> };

/** A project with two areas and two saved accounts. `answer` is what the
 * app says for one account's check. */
function mountForCheck(answer: (key: string) => unknown, marks: Record<string, unknown> = {}) {
  const seen: Seen[] = [];
  mockIPC((cmd, args) => {
    seen.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === "auto_run_list_accounts") {
      return [
        { key: "admin", label: "Administrator", username: "kim", password: "p1" },
        { key: "sup", label: "", username: "lee", password: "p2" },
      ];
    }
    if (cmd === "auto_run_load_nav") {
      return {
        direct_urls: false,
        modules: [
          { area: "Leave", module: "Leave", clicks: [], arrived: "/hr/leave", recorded: "x", repaired_by_assistant: null },
          { area: "Definition Wizard", module: "Settings", clicks: [], arrived: "/hr/wizard", recorded: "x", repaired_by_assistant: null },
        ],
        save_words: [],
        built_in_save_words: [],
        protected: [],
      };
    }
    if (cmd === "auto_run_sign_in_checks") return marks;
    if (cmd === "auto_run_check_sign_in") return answer((args as { accountKey: string }).accountKey);
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AccountsDialog org="acme" project="Web" onClose={vi.fn()} />
    </QueryClientProvider>,
  );
  return seen;
}

function checked(key: string, summary: string, signedIn = true, stopped = false) {
  return {
    check: {
      account: key,
      signed_in: signedIn,
      landed: "/hr/home/index",
      stopped: null,
      password: null,
      area: stopped ? { kind: "stopped", area: "Definition Wizard", at: "x" } : { kind: "reached", area: "Definition Wizard" },
    },
    summary,
  };
}

test("Check sign-in shows the summary under the account", async () => {
  mountForCheck((key) =>
    key === "admin"
      ? checked("admin", "signed in, landed on /hr/home/index; reaches Definition Wizard")
      : checked("sup", "signed in, landed on /hr/home/index; stopped on the way to Definition Wizard: click 2", true, true),
  );
  await screen.findByRole("checkbox", { name: "Check admin" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Check admin" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Check sup" }));
  fireEvent.click(screen.getByRole("button", { name: "Check sign-in (2)" }));
  expect(await screen.findByText("signed in, landed on /hr/home/index; reaches Definition Wizard")).toBeInTheDocument();
  expect(await screen.findByText(/stopped on the way to Definition Wizard: click 2/)).toBeInTheDocument();
});

test("the area picker sends the area, and none sends the sign-in alone", async () => {
  const seen = mountForCheck((key) => checked(key, "signed in"));
  await screen.findByRole("checkbox", { name: "Check admin" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Check admin" }));
  fireEvent.click(screen.getByRole("button", { name: "Check sign-in (1)" }));
  await waitFor(() => expect(seen.filter((s) => s.cmd === "auto_run_check_sign_in")).toHaveLength(1));
  expect(seen.find((s) => s.cmd === "auto_run_check_sign_in")!.args).toMatchObject({
    organization: "acme",
    project: "Web",
    accountKey: "admin",
    area: null,
  });

  fireEvent.click(screen.getByRole("combobox", { name: "Walk to" }));
  fireEvent.click(await screen.findByRole("option", { name: "Definition Wizard" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Check sign-in (1)" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Check sign-in (1)" }));
  await waitFor(() => expect(seen.filter((s) => s.cmd === "auto_run_check_sign_in")).toHaveLength(2));
  expect(seen.filter((s) => s.cmd === "auto_run_check_sign_in")[1].args.area).toBe("Definition Wizard");
});

test("accounts are checked one after another, never together", async () => {
  let running = 0;
  let most = 0;
  mockIPC(() => null);
  const seen = mountForCheck((key) => {
    running += 1;
    most = Math.max(most, running);
    running -= 1;
    return checked(key, "signed in");
  });
  await screen.findByRole("checkbox", { name: "Check admin" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Check admin" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Check sup" }));
  fireEvent.click(screen.getByRole("button", { name: "Check sign-in (2)" }));
  await waitFor(() => expect(seen.filter((s) => s.cmd === "auto_run_check_sign_in")).toHaveLength(2));
  const keys = seen.filter((s) => s.cmd === "auto_run_check_sign_in").map((s) => s.args.accountKey);
  expect(keys).toEqual(["admin", "sup"]);
  expect(most).toBe(1);
});

test("a refused check is said under its account", async () => {
  mountForCheck(() => {
    throw new Error("an unattended run, a replay or a discovery is using the Auto Run browser - wait for it, or stop it first");
  });
  await screen.findByRole("checkbox", { name: "Check admin" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Check admin" }));
  fireEvent.click(screen.getByRole("button", { name: "Check sign-in (1)" }));
  expect(await screen.findByText(/using the Auto Run browser/)).toBeInTheDocument();
});

test("the last check of an account is shown until it is checked again", async () => {
  mountForCheck(() => checked("admin", "signed in"), {
    admin: { checked_ms: 1761900000000, reached: true, area: "Leave", outcome: "signed in, landed on /hr/home/index; reaches Leave" },
  });
  expect(await screen.findByText(/Last checked .*reaches Leave/)).toBeInTheDocument();
});

test("Check sign-in is disabled while a run is going", async () => {
  checkFixture.taken = true;
  try {
    mountForCheck((key) => checked(key, "signed in"));
    await screen.findByRole("checkbox", { name: "Check admin" });
    fireEvent.click(screen.getByRole("checkbox", { name: "Check admin" }));
    expect(screen.getByRole("button", { name: "Check sign-in (1)" })).toBeDisabled();
  } finally {
    checkFixture.taken = false;
  }
});

test("the first refusal ends the loop: the accounts after it are not asked", async () => {
  const seen = mountForCheck(() => {
    throw new Error("a sign-in check is running (sup) - wait for it or cancel it");
  });
  await screen.findByRole("checkbox", { name: "Check admin" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Check admin" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Check sup" }));
  fireEvent.click(screen.getByRole("button", { name: "Check sign-in (2)" }));
  expect(await screen.findByText(/sign-in check is running/)).toBeInTheDocument();
  await waitFor(() => expect(screen.getByRole("button", { name: "Check sign-in (2)" })).toBeEnabled());
  expect(seen.filter((s) => s.cmd === "auto_run_check_sign_in")).toHaveLength(1);
});

test("Cancel check stops the loop after the account going and tells the app", async () => {
  let release: (v: unknown) => void = () => {};
  const gate = new Promise((r) => (release = r));
  const seen = mountForCheck((key) => (key === "admin" ? gate.then(() => checked("admin", "signed in")) : checked(key, "signed in")));
  await screen.findByRole("checkbox", { name: "Check admin" });
  fireEvent.click(screen.getByRole("checkbox", { name: "Check admin" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Check sup" }));
  fireEvent.click(screen.getByRole("button", { name: "Check sign-in (2)" }));
  fireEvent.click(await screen.findByRole("button", { name: "Cancel check" }));
  await waitFor(() => expect(seen.some((s) => s.cmd === "auto_run_cancel_check_sign_in")).toBe(true));
  // The accounts are fixed while checking.
  expect(screen.getByRole("checkbox", { name: "Check sup" })).toHaveAttribute("aria-disabled", "true");
  release(null);
  await waitFor(() => expect(screen.getByRole("button", { name: "Check sign-in (2)" })).toBeEnabled());
  expect(seen.filter((s) => s.cmd === "auto_run_check_sign_in")).toHaveLength(1);
});

test("the results are announced politely", async () => {
  mountForCheck((key) => checked(key, "signed in"));
  const box = await screen.findByRole("checkbox", { name: "Check admin" });
  expect(box.closest("ul")).toHaveAttribute("aria-live", "polite");
});

test("a last check over a week old is shown as stale", async () => {
  mountForCheck(() => checked("admin", "signed in"), {
    admin: { checked_ms: Date.now() - 10 * 24 * 3600 * 1000, signed_in: true, reached: true, area: "Leave", outcome: "reaches Leave" },
  });
  expect(await screen.findByText(/\(stale\)/)).toBeInTheDocument();
});

test("Save is off until something is changed, and sends the version the list was read at", async () => {
  const sent: Record<string, unknown>[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_accounts_version") return "12-345";
    if (cmd === "auto_run_list_accounts") return [{ key: "admin", label: "A", username: "kim", password: "p" }];
    if (cmd === "auto_run_save_accounts") {
      sent.push(args as Record<string, unknown>);
      return [];
    }
    return null;
  });
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <AccountsDialog onClose={() => {}} />
    </QueryClientProvider>,
  );
  await screen.findByDisplayValue("kim");
  const save = screen.getByRole("button", { name: "Save accounts" });
  expect(save).toBeDisabled();
  fireEvent.change(screen.getByLabelText("Name for account 1"), { target: { value: "B" } });
  expect(save).not.toBeDisabled();
  fireEvent.click(save);
  await waitFor(() => expect(sent).toHaveLength(1));
  expect(sent[0].loadedVersion).toBe("12-345");
});

test("a list that changed under the dialog takes the saved password into untouched rows and untouched passwords", () => {
  const base = [
    { key: "a", label: "A", username: "u", password: "old" },
    { key: "b", label: "B", username: "v", password: "pb" },
  ];
  const fresh = [
    { key: "a", label: "A", username: "u", password: "Abc@123" },
    { key: "b", label: "B", username: "v", password: "pb" },
  ];
  // Nothing touched: the saved list.
  expect(reseedRows(base, base, fresh)).toEqual(fresh);
  // A label edited: kept, but the password is the saved one.
  const edited = [{ ...base[0], label: "Mine" }, base[1]];
  expect(reseedRows(edited, base, fresh)[0]).toEqual({ ...fresh[0], label: "Mine" });
  // A password the person typed stays theirs.
  const typed = [{ ...base[0], password: "typed" }, base[1]];
  expect(reseedRows(typed, base, fresh)[0].password).toBe("typed");
  // A removed row stays removed; an added row stays.
  expect(reseedRows([base[1]], base, fresh).map((r) => r.key)).toEqual(["b"]);
  const added = { key: "c", label: "", username: "", password: "" };
  expect(reseedRows([...base, added], base, fresh).map((r) => r.key)).toEqual(["a", "b", "c"]);
});
