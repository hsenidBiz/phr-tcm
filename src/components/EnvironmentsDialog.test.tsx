// The environments dialog: adding with the command's refusal shown, the
// Test environment warning, the default password that goes in and never
// stays in the page, and removing (which asks first).

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import EnvironmentsDialog from "./EnvironmentsDialog";

vi.mock("../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
  localStorage.clear();
});

const DATABASES = [
  { id: "dev-read", label: "Dev - read only", shipped: true, server: "s", port: null, database: "d", user: "u", trust_cert: true, has_password: true, customised: false },
  { id: "qa-read", label: "QA - read only", shipped: true, server: "s", port: null, database: "d", user: "u", trust_cert: true, has_password: true, customised: false },
];

type Env = {
  id: string;
  name: string;
  start_url: string;
  allowed_origins: string[];
  db_id: string;
  test_environment: boolean;
  test_prefix: string;
  has_default_password: boolean;
  password_expiry_query: string;
  active_session_query: string;
  reset_expired_passwords: boolean;
};

const DEFAULT: Env = {
  id: "env-00000001", name: "Default", start_url: "", allowed_origins: [],
  db_id: "dev-read", test_environment: false, test_prefix: "AUTOTEST", has_default_password: false,
  password_expiry_query: "", active_session_query: "", reset_expired_passwords: false,
};
const QA: Env = {
  id: "env-00000002", name: "QA", start_url: "https://qa.example.internal/", allowed_origins: [],
  db_id: "qa-read", test_environment: false, test_prefix: "QATEST", has_default_password: false,
  password_expiry_query: "", active_session_query: "", reset_expired_passwords: false,
};

/** A small stand-in for the Rust side: the list, plus whatever each command
 * is told to answer. */
function mount(handlers: Record<string, (args: Record<string, unknown>) => unknown> = {}, envs: Env[] = [DEFAULT, QA]) {
  const state = { envs: [...envs], active: DEFAULT.id };
  const view = () => ({ active: state.active, environments: state.envs });
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    if (handlers[cmd]) return handlers[cmd](a);
    if (cmd === "db_databases") return DATABASES;
    if (cmd === "env_list") return view();
    if (cmd === "env_set_default_password") {
      state.envs = state.envs.map((e) => (e.id === a.id ? { ...e, has_default_password: true } : e));
      return null;
    }
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <EnvironmentsDialog onClose={onClose} />
    </QueryClientProvider>,
  );
  return { calls, onClose };
}

test("lists each environment with its address, database and default-password state", async () => {
  mount();
  expect(await screen.findByText("QA")).toBeInTheDocument();
  expect(screen.getByText("https://qa.example.internal/")).toBeInTheDocument();
  expect(screen.getByText("Using the sign-in recipe's address")).toBeInTheDocument();
  expect(await screen.findByText("Dev - read only")).toBeInTheDocument();
  expect(screen.getAllByText("No default password")).toHaveLength(2);
});

test("adding with an empty name shows the command's refusal inline", async () => {
  const { calls } = mount({
    env_save: () => {
      throw "an environment needs a name";
    },
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Add environment" }));
  fireEvent.click(await screen.findByRole("button", { name: "Save environment" }));
  expect(await screen.findByText("an environment needs a name")).toBeInTheDocument();
  // The form stays open for a fix.
  expect(screen.getByRole("textbox", { name: "Name" })).toBeInTheDocument();
  expect(calls.filter((c) => c.cmd === "env_save")).toHaveLength(1);
});

test("a new environment is saved with an empty id and the form's fields", async () => {
  const { calls } = mount({
    env_save: () => ({ active: DEFAULT.id, environments: [DEFAULT, QA] }),
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Add environment" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "Name" }), { target: { value: "Staging" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Website address" }), {
    target: { value: "https://stg.example.internal/" },
  });
  fireEvent.change(screen.getByRole("textbox", { name: "Also allowed" }), {
    target: { value: "https://login.example.com\n\n https://cdn.example.com " },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save environment" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "env_save")).toBe(true));
  expect(calls.find((c) => c.cmd === "env_save")!.args.env).toEqual({
    id: "",
    name: "Staging",
    start_url: "https://stg.example.internal/",
    allowed_origins: ["https://login.example.com", "https://cdn.example.com"],
    db_id: "dev-read",
    test_environment: false,
    test_prefix: "AUTOTEST",
    password_expiry_query: "",
    active_session_query: "",
  });
});

test("the two sign-in queries are boxes that start empty, show an edited environment's own, and are saved as typed", async () => {
  const withQueries: Env = {
    ...QA,
    password_expiry_query: "SELECT u AS user_id, d AS reset_due FROM t WHERE u IN ({users})",
    active_session_query: "SELECT s AS since FROM a WHERE u = {user} AND f = 1",
  };
  const { calls } = mount({ env_save: () => ({ active: DEFAULT.id, environments: [DEFAULT, withQueries] }) }, [DEFAULT, withQueries]);
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Add environment" }));
  expect(((await screen.findByRole("textbox", { name: "Password expiry query" })) as HTMLTextAreaElement).value).toBe("");
  expect((screen.getByRole("textbox", { name: "Active session query" }) as HTMLTextAreaElement).value).toBe("");
  fireEvent.click(screen.getByRole("button", { name: "Discard" }));
  fireEvent.click(await screen.findByRole("button", { name: "Edit QA" }));
  const expiry = (await screen.findByRole("textbox", { name: "Password expiry query" })) as HTMLTextAreaElement;
  expect(expiry.value).toContain("{users}");
  expect((screen.getByRole("textbox", { name: "Active session query" }) as HTMLTextAreaElement).value).toContain("{user}");
  fireEvent.change(expiry, { target: { value: "SELECT 1 FROM t WHERE u IN ({users})" } });
  fireEvent.click(screen.getByRole("button", { name: "Save environment" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "env_save")).toBe(true));
  const sent = calls.find((c) => c.cmd === "env_save")!.args.env as { password_expiry_query: string; active_session_query: string };
  expect(sent.password_expiry_query).toBe("SELECT 1 FROM t WHERE u IN ({users})");
  expect(sent.active_session_query).toContain("{user}");
});

test("a refused query shows the command's sentence and the form stays open", async () => {
  mount({
    env_save: () => {
      throw "the password expiry query of \"Staging\": A password expiry query is one SELECT statement.";
    },
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Add environment" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "Name" }), { target: { value: "Staging" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Password expiry query" }), { target: { value: "DELETE FROM t" } });
  fireEvent.click(screen.getByRole("button", { name: "Save environment" }));
  expect(await screen.findByText(/is one SELECT statement/)).toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Password expiry query" })).toBeInTheDocument();
});

test("an environment's Test name prefix starts at AUTOTEST, is saved as typed and shows a refusal", async () => {
  const { calls } = mount({
    env_save: () => {
      throw "the test name prefix must be 3 to 20 letters, digits or -";
    },
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Add environment" }));
  const field = (await screen.findByRole("textbox", { name: "Test name prefix" })) as HTMLInputElement;
  expect(field.value).toBe("AUTOTEST");
  fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "Staging" } });
  fireEvent.change(field, { target: { value: "x" } });
  fireEvent.click(screen.getByRole("button", { name: "Save environment" }));
  expect(await screen.findByText("the test name prefix must be 3 to 20 letters, digits or -")).toBeInTheDocument();
  expect((calls.find((c) => c.cmd === "env_save")!.args.env as { test_prefix: string }).test_prefix).toBe("x");
  // The form stays open for a fix.
  expect(screen.getByRole("textbox", { name: "Test name prefix" })).toBeInTheDocument();
});

test("editing an environment shows its own prefix", async () => {
  mount();
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Edit QA" }));
  expect(((await screen.findByRole("textbox", { name: "Test name prefix" })) as HTMLInputElement).value).toBe("QATEST");
});

test("Also allowed is off until there is a website address, and nothing stale is sent without one", async () => {
  const stale: Env = { ...DEFAULT, allowed_origins: ["https://stale.example.org"] };
  const { calls } = mount({ env_save: () => ({ active: DEFAULT.id, environments: [DEFAULT, QA] }) }, [stale, QA]);
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Edit Default" }));
  const also = (await screen.findByRole("textbox", { name: "Also allowed" })) as HTMLTextAreaElement;
  expect(also).toBeDisabled();
  expect(
    screen.getByText("Also allowed needs a website address - until then the sign-in recipe's are used."),
  ).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Save environment" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "env_save")).toBe(true));
  expect((calls.find((c) => c.cmd === "env_save")!.args.env as { allowed_origins: string[] }).allowed_origins).toEqual([]);

  // A new environment starts with no address, so the box starts off too.
  fireEvent.click(await screen.findByRole("button", { name: "Add environment" }));
  const fresh = (await screen.findByRole("textbox", { name: "Also allowed" })) as HTMLTextAreaElement;
  expect(fresh).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "Website address" }), {
    target: { value: "https://stg.example.internal/" },
  });
  expect(fresh).toBeEnabled();
});

test("the Test environment switch carries the fixed warning", async () => {
  mount();
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Add environment" }));
  expect(
    await screen.findByText(
      "The AI assistant can read the full logins of this environment's accounts. Use only for test environments.",
    ),
  ).toBeInTheDocument();
  expect(screen.getByRole("switch", { name: "Test environment" })).toHaveAttribute("aria-checked", "false");
});

test("the default password is masked, sent by Set, cleared after, and only its state shows", async () => {
  const { calls } = mount();
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Edit QA" }));
  const field = (await screen.findByLabelText("Default password")) as HTMLInputElement;
  expect(field.type).toBe("password");
  fireEvent.change(field, { target: { value: "s3cret-pass" } });
  fireEvent.click(screen.getByRole("button", { name: "Set" }));

  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "env_set_default_password")?.args).toEqual({
      id: QA.id,
      password: "s3cret-pass",
    }),
  );
  await waitFor(() => expect((screen.getByLabelText("Default password") as HTMLInputElement).value).toBe(""));
  expect(await screen.findByText("Default password set")).toBeInTheDocument();
  // Nowhere in the page, not even in a value.
  expect(document.body.innerHTML).not.toContain("s3cret-pass");
  // Set is off again until something is typed.
  expect(screen.getByRole("button", { name: "Set" })).toBeDisabled();
  // And the clear control appeared.
  expect(screen.getByRole("button", { name: "Clear default password" })).toBeInTheDocument();
});

test("a refused default password is not kept in the field either", async () => {
  mount({
    env_set_default_password: () => {
      throw "that environment is not there any more";
    },
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Edit QA" }));
  fireEvent.change(await screen.findByLabelText("Default password"), { target: { value: "oops-pass" } });
  fireEvent.click(screen.getByRole("button", { name: "Set" }));
  expect(await screen.findByText("that environment is not there any more")).toBeInTheDocument();
  expect((screen.getByLabelText("Default password") as HTMLInputElement).value).toBe("");
});

test("a rejected default-password call leaves nothing in the field either", async () => {
  mount({
    env_set_default_password: () => {
      throw new Error("the call never arrived");
    },
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Edit QA" }));
  fireEvent.change(await screen.findByLabelText("Default password"), { target: { value: "lost-pass" } });
  fireEvent.click(screen.getByRole("button", { name: "Set" }));
  expect(await screen.findByText("the call never arrived")).toBeInTheDocument();
  expect((screen.getByLabelText("Default password") as HTMLInputElement).value).toBe("");
  expect(document.body.innerHTML).not.toContain("lost-pass");
});

test("renaming the active environment whose database is gone never writes that id to the card", async () => {
  const gone = { ...DEFAULT, db_id: "removed-login" };
  mount(
    {
      env_save: (a) => ({ active: DEFAULT.id, environments: [{ ...gone, ...(a.env as object) }, QA] }),
    },
    [gone, QA],
  );
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Edit Default" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "Name" }), { target: { value: "Default renamed" } });
  fireEvent.click(screen.getByRole("button", { name: "Save environment" }));
  await waitFor(() => expect(screen.queryByRole("textbox", { name: "Name" })).not.toBeInTheDocument());
  expect(localStorage.getItem("tcm-v2-db-selected")).toBeNull();
});

test("a rename of the active environment leaves a chosen database on the card alone", async () => {
  localStorage.setItem("tcm-v2-db-selected", "qa-read");
  mount({
    env_save: (a) => ({ active: DEFAULT.id, environments: [{ ...DEFAULT, ...(a.env as object) }, QA] }),
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Edit Default" }));
  fireEvent.change(await screen.findByRole("textbox", { name: "Name" }), { target: { value: "Dev" } });
  fireEvent.click(screen.getByRole("button", { name: "Save environment" }));
  await waitFor(() => expect(screen.queryByRole("textbox", { name: "Name" })).not.toBeInTheDocument());
  expect(localStorage.getItem("tcm-v2-db-selected")).toBe("qa-read");
});

test("moving the active environment to another existing database moves the card", async () => {
  mount({
    env_save: (a) => ({ active: DEFAULT.id, environments: [{ ...DEFAULT, ...(a.env as object) }, QA] }),
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Edit Default" }));
  fireEvent.click(await screen.findByRole("combobox", { name: "Environment database" }));
  fireEvent.click(await screen.findByRole("option", { name: "QA - read only" }));
  fireEvent.click(screen.getByRole("button", { name: "Save environment" }));
  await waitFor(() => expect(localStorage.getItem("tcm-v2-db-selected")).toBe("qa-read"));
});

test("a new environment says to save it before a default password", async () => {
  mount();
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Add environment" }));
  expect(await screen.findByText("Save the environment first, then set its default password.")).toBeInTheDocument();
  expect(screen.queryByLabelText("Default password")).not.toBeInTheDocument();
});

test("Remove asks first, and removing the active environment shows the refusal", async () => {
  const { calls } = mount({
    env_remove: () => {
      throw "the active environment cannot be removed - switch to another one first";
    },
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Remove Default" }));
  // Asked, nothing sent yet.
  expect(await screen.findByText(/Its accounts and saved sign-ins go now/)).toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "env_remove")).toBe(false);

  fireEvent.click(screen.getByRole("button", { name: "Remove" }));
  expect(
    await screen.findByText("the active environment cannot be removed - switch to another one first"),
  ).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "env_remove")!.args).toEqual({ id: DEFAULT.id });
});

test("Keep backs out of a removal without sending anything", async () => {
  const { calls } = mount();
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Remove QA" }));
  fireEvent.click(await screen.findByRole("button", { name: "Keep" }));
  expect(screen.queryByRole("button", { name: "Remove" })).not.toBeInTheDocument();
  expect(calls.some((c) => c.cmd === "env_remove")).toBe(false);
});

test("a removed environment leaves the list", async () => {
  mount({
    env_remove: () => ({ active: DEFAULT.id, environments: [DEFAULT] }),
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Remove QA" }));
  fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
  await waitFor(() => expect(screen.queryByText("QA")).not.toBeInTheDocument());
  const rows = screen.getAllByRole("listitem");
  expect(within(rows[0]).getByText("Default")).toBeInTheDocument();
});

/// The database select reads `db_databases`, so a person's own databases
/// are offered beside the shipped ones, by name.
test("the database select offers your own databases after the shipped ones", async () => {
  const own = [
    { id: "own", label: "Your own database", shipped: false, server: "", port: null, database: "", user: "", trust_cert: false, has_password: false, customised: false },
    { id: "custom-1a2b3c4d", label: "Staging", shipped: false, server: "s", port: null, database: "d", user: "u", trust_cert: false, has_password: true, customised: true },
  ];
  mount({ db_databases: () => [...DATABASES, ...own] });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Add environment" }));
  fireEvent.click(await screen.findByRole("combobox", { name: "Environment database" }));
  const options = (await screen.findAllByRole("option")).map((o) => o.textContent);
  expect(options).toEqual(["Dev - read only", "QA - read only", "Your own database", "Staging"]);
});

test("says the Auto Run data of a removed environment is kept for 30 days", async () => {
  mount();
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Remove QA" }));
  expect(
    await screen.findByText(
      "Remove QA? Its accounts and saved sign-ins go now. Its Auto Run scripts, areas and templates are kept for 30 days and can be copied into another environment from Copy Auto Run data…",
    ),
  ).toBeInTheDocument();
});

test("removing the Playwright export environment shows the refusal and keeps it listed", async () => {
  mount({
    env_remove: () => {
      throw "this is the Playwright export environment - name another one on the Playwright tab first";
    },
  });
  await screen.findByText("QA");
  fireEvent.click(screen.getByRole("button", { name: "Remove QA" }));
  fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("name another one on the Playwright tab first");
  expect(screen.getByText("QA")).toBeInTheDocument();
});

test("each environment has a Copy Auto Run data button that opens the copy dialog for it", async () => {
  const { calls } = mount({
    env_copy_choices: () => ({
      target_empty: true,
      sources: [{ key: DEFAULT.id, name: "Default", removed: false, days_left: null }],
    }),
  });
  await screen.findByText("QA");
  expect(screen.getByRole("button", { name: "Copy Auto Run data into Default" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Copy Auto Run data into QA" }));
  expect(await screen.findByRole("combobox", { name: "Copy from" })).toBeInTheDocument();
  expect(calls.find((c) => c.cmd === "env_copy_choices")!.args).toEqual({ targetId: QA.id });
});

test("the reset switch shows only for a saved test environment, is off by default and moves by its own command", async () => {
  const TEST_QA: Env = { ...QA, test_environment: true };
  const { calls } = mount(
    {
      env_set_reset_expired_passwords: (a) => ({
        active: DEFAULT.id,
        environments: [DEFAULT, { ...TEST_QA, reset_expired_passwords: a.on === true }],
      }),
    },
    [DEFAULT, TEST_QA],
  );
  await screen.findByText("QA");
  // Not a test environment: no switch.
  fireEvent.click(await screen.findByRole("button", { name: "Edit Default" }));
  await screen.findByRole("textbox", { name: "Name" });
  expect(screen.queryByRole("switch", { name: "Reset expired passwords" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Discard" }));
  fireEvent.click(await screen.findByRole("button", { name: "Edit QA" }));
  const sw = await screen.findByRole("switch", { name: "Reset expired passwords" });
  expect(sw.getAttribute("aria-checked")).toBe("false");
  expect(screen.getByText(/sets a simple new password, saves it on that account/)).toBeTruthy();
  fireEvent.click(sw);
  await waitFor(() => expect(calls.some((c) => c.cmd === "env_set_reset_expired_passwords")).toBe(true));
  expect(calls.find((c) => c.cmd === "env_set_reset_expired_passwords")!.args).toEqual({ id: TEST_QA.id, on: true });
  await waitFor(() =>
    expect(screen.getByRole("switch", { name: "Reset expired passwords" }).getAttribute("aria-checked")).toBe("true"),
  );
  // A save of the form never carries it.
  fireEvent.click(screen.getByRole("button", { name: "Save environment" }));
  await waitFor(() => expect(calls.some((c) => c.cmd === "env_save")).toBe(true));
  expect(Object.keys(calls.find((c) => c.cmd === "env_save")!.args.env as object)).not.toContain("reset_expired_passwords");
});
