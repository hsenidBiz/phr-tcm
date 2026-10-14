// The Playwright tab's Accounts: the list, the Add / Edit / Rename / Delete
// forms, the preview that must come before Apply, and Show password. Rust
// builds the npm run users arguments and hides passwords
// (tests/suite/pw_repo_users.rs); here they are mocked commands. Every value
// is made up.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { AccountRefs, ChangeOutcome, RepoAccount } from "../../bindings";
import Accounts from "./Accounts";

afterEach(() => clearMocks());

const GENERAL: RepoAccount = {
  key: "FakeSL.performance.general",
  username: "fake.general@example.test",
  employee_no: "",
  display_name: "Fake General",
  title: "Analyst",
  supervisor: "FakeSL.performance.supervisor",
};
const SUPERVISOR: RepoAccount = {
  key: "FakeSL.performance.supervisor",
  username: "fake.supervisor@example.test",
  employee_no: "E0001",
  display_name: "",
  title: "",
  supervisor: "",
};
const REFS: AccountRefs[] = [
  { key: GENERAL.key, specs: 3, other_refs: 0 },
  { key: SUPERVISOR.key, specs: 0, other_refs: 1 },
];
const PASSWORD = "Made-Up-Pass-4417";
const CLONE = "C:\\clone";

type Call = { cmd: string; args: Record<string, unknown> };

function mount(
  opts: { change?: (args: Record<string, unknown>) => ChangeOutcome; refs?: () => AccountRefs[] | Promise<AccountRefs[]> } = {},
) {
  const calls: Call[] = [];
  let listed = [GENERAL, SUPERVISOR];
  mockIPC((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args });
    if (cmd === "pw_users_list") return listed;
    if (cmd === "pw_users_password") return PASSWORD;
    if (cmd === "pw_users_refs") return opts.refs ? opts.refs() : REFS;
    if (cmd === "pw_users_change") {
      const r = opts.change?.(args) ?? {
        applied: args.apply as boolean,
        output: args.apply ? ["add FakeSL.payroll.general", "  Wrote src/users/users.json"] : ["add FakeSL.payroll.general", "  Nothing written. Re-run with --apply to write it."],
        accounts: args.apply ? [...listed, { ...SUPERVISOR, key: "FakeSL.payroll.general" }] : null,
        list_error: null,
      };
      if (r.accounts) listed = r.accounts;
      return r;
    }
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let changed = 0;
  render(
    <QueryClientProvider client={qc}>
      <Accounts clone={CLONE} onChanged={() => (changed += 1)} />
    </QueryClientProvider>,
  );
  return { calls, changed: () => changed };
}

const changes = (calls: Call[]) => calls.filter((c) => c.cmd === "pw_users_change");

test("the list shows each account without a password", async () => {
  mount();
  expect(await screen.findByText(GENERAL.key)).toBeTruthy();
  expect(screen.getByText(SUPERVISOR.key)).toBeTruthy();
  expect(await screen.findByText("3 specs")).toBeTruthy();
  expect(screen.getByText("1 other reference")).toBeTruthy();
  const row = screen.getByRole("row", { name: (name) => name.startsWith(`${GENERAL.key} `) });
  for (const text of [GENERAL.username, GENERAL.display_name, GENERAL.title, `reports to ${SUPERVISOR.key}`]) {
    expect(within(row).getByText(text)).toBeTruthy();
  }
  // Each row's actions are named buttons, a tooltip saying which.
  for (const name of [`Show password of ${GENERAL.key}`, `Edit ${GENERAL.key}`, `Rename ${GENERAL.key}`, `Delete ${GENERAL.key}`]) {
    expect(within(row).getByRole("button", { name })).toHaveAttribute("title");
  }
  expect(document.body.textContent).not.toContain(PASSWORD);
});

test("the counts come after the list: the list is shown at once, and Used by fills in", async () => {
  let release: (r: AccountRefs[]) => void = () => {};
  const { calls } = mount({ refs: () => new Promise((resolve) => (release = resolve)) });
  expect(await screen.findByText(GENERAL.key)).toBeTruthy();
  // Asked for only after the list.
  await waitFor(() => expect(calls.some((c) => c.cmd === "pw_users_refs")).toBe(true));
  expect(calls.findIndex((c) => c.cmd === "pw_users_list")).toBeLessThan(calls.findIndex((c) => c.cmd === "pw_users_refs"));
  expect(screen.getByLabelText(`Counting what uses ${GENERAL.key}`).textContent).toBe("…");
  expect(screen.queryByText("3 specs")).toBeNull();

  release(REFS);
  expect(await screen.findByText("3 specs")).toBeTruthy();
  expect(screen.getByText("1 other reference")).toBeTruthy();
  expect(screen.queryByLabelText(`Counting what uses ${GENERAL.key}`)).toBeNull();
});

test("counts that cannot be read leave Used by blank, with a note", async () => {
  mount({
    refs: () => {
      throw "npm run users ended with exit code 1 and said nothing.";
    },
  });
  expect(await screen.findByText(GENERAL.key)).toBeTruthy();
  const note = await screen.findByText(/^Used by could not be read: /);
  expect(note).toHaveClass("text-muted");
  expect(screen.queryByLabelText(`Counting what uses ${GENERAL.key}`)).toBeNull();
  expect(screen.queryByText("Not used")).toBeNull();
  expect(screen.queryByText("3 specs")).toBeNull();
});

test("a form opens in a dialog, says which step it is on, and Cancel closes it", async () => {
  mount();
  await screen.findByText(GENERAL.key);
  fireEvent.click(screen.getByRole("button", { name: `Rename ${GENERAL.key}` }));
  const dialog = screen.getByRole("dialog", { name: `Rename ${GENERAL.key}` });
  expect(within(dialog).getByText("1. Fill in and preview")).toHaveClass("text-accent");
  fireEvent.click(within(dialog).getByRole("button", { name: "Preview" }));
  await within(dialog).findByLabelText("Preview");
  expect(within(dialog).getByText("2. Apply")).toHaveClass("text-accent");
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

test("Show password asks for it, and Hide drops it", async () => {
  const { calls } = mount();
  await screen.findByText(GENERAL.key);
  fireEvent.click(screen.getByRole("button", { name: `Show password of ${GENERAL.key}` }));
  expect((await screen.findByLabelText(`Password of ${GENERAL.key}`)).textContent).toBe(PASSWORD);
  expect(calls.filter((c) => c.cmd === "pw_users_password")).toEqual([
    { cmd: "pw_users_password", args: { clone: CLONE, key: GENERAL.key } },
  ]);
  // Only the account asked about.
  expect(screen.queryByLabelText(`Password of ${SUPERVISOR.key}`)).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: `Hide password of ${GENERAL.key}` }));
  expect(screen.queryByLabelText(`Password of ${GENERAL.key}`)).toBeNull();
  expect(document.body.textContent).not.toContain(PASSWORD);
});

test("Add previews first, and only the preview offers Apply", async () => {
  const { calls, changed } = mount();
  await screen.findByText(GENERAL.key);
  fireEvent.click(screen.getByRole("button", { name: "Add account" }));
  const form = screen.getByRole("group", { name: "Add an account" });
  expect(within(form).queryByRole("button", { name: "Apply" })).toBeNull();

  fireEvent.change(within(form).getByLabelText("Key"), { target: { value: "FakeSL.payroll.general" } });
  fireEvent.change(within(form).getByLabelText("Username"), { target: { value: "fake.payroll@example.test" } });
  const password = within(form).getByLabelText("Password") as HTMLInputElement;
  expect(password.type).toBe("password");
  fireEvent.change(password, { target: { value: "Other-Fake-Pass-9023" } });
  fireEvent.change(within(form).getByLabelText("Title"), { target: { value: "Clerk" } });

  fireEvent.click(within(form).getByRole("button", { name: "Preview" }));
  expect((await within(form).findByLabelText("Preview")).textContent).toContain("Nothing written");
  expect(changes(calls)).toEqual([
    {
      cmd: "pw_users_change",
      args: {
        clone: CLONE,
        apply: false,
        change: {
          kind: "Add",
          key: "FakeSL.payroll.general",
          username: "fake.payroll@example.test",
          password: "Other-Fake-Pass-9023",
          employee_no: "",
          display_name: "",
          title: "Clerk",
          supervisor: "",
        },
      },
    },
  ]);

  // Editing the form again drops the preview: Apply is not offered for
  // something the preview did not describe.
  fireEvent.change(within(form).getByLabelText("Title"), { target: { value: "Clerk II" } });
  expect(within(form).queryByLabelText("Preview")).toBeNull();
  expect(within(form).queryByRole("button", { name: "Apply" })).toBeNull();
  fireEvent.click(within(form).getByRole("button", { name: "Preview" }));
  fireEvent.click(await within(form).findByRole("button", { name: "Apply" }));

  await waitFor(() => expect(screen.queryByRole("group", { name: "Add an account" })).toBeNull());
  const all = changes(calls);
  const last = all[all.length - 1];
  expect(last.args.apply).toBe(true);
  expect((last.args.change as { title: string }).title).toBe("Clerk II");
  expect(screen.getByLabelText("Applied").textContent).toContain("Wrote src/users/users.json");
  // The listing the change answered with is shown, without listing again.
  expect(await screen.findByText("FakeSL.payroll.general")).toBeTruthy();
  expect(calls.filter((c) => c.cmd === "pw_users_list")).toHaveLength(1);
  expect(changed()).toBe(1);
});

test("Edit sets one field, with a password field hidden as typed", async () => {
  const { calls } = mount();
  await screen.findByText(GENERAL.key);
  fireEvent.click(screen.getByRole("button", { name: `Edit ${GENERAL.key}` }));
  const form = screen.getByRole("group", { name: `Edit ${GENERAL.key}` });
  expect((within(form).getByLabelText("New value") as HTMLInputElement).type).toBe("text");
  fireEvent.click(within(form).getByRole("combobox", { name: "Field" }));
  fireEvent.click(await screen.findByRole("option", { name: "Password" }));
  const value = within(form).getByLabelText("New value") as HTMLInputElement;
  expect(value.type).toBe("password");
  fireEvent.change(value, { target: { value: "Brand-New-Fake-6650" } });
  fireEvent.click(within(form).getByRole("button", { name: "Preview" }));
  await within(form).findByLabelText("Preview");
  expect(changes(calls)[0].args.change).toEqual({ kind: "Set", key: GENERAL.key, field: "Password", value: "Brand-New-Fake-6650" });
});

test("Rename previews what the cascade rewrites, then applies", async () => {
  const output = [
    `rename ${GENERAL.key} -> ${GENERAL.key}.a`,
    "  2 reference(s) in 1 file(s) will change:",
    "  suites/sl/admin/performance/rating-methods/specs/rating-methods.spec.ts:4  (spec)",
    "  Nothing written. Re-run with --apply to write it.",
  ];
  const { calls } = mount({ change: (args) => ({ applied: args.apply as boolean, output, accounts: null, list_error: null }) });
  await screen.findByText(GENERAL.key);
  fireEvent.click(screen.getByRole("button", { name: `Rename ${GENERAL.key}` }));
  const form = screen.getByRole("group", { name: `Rename ${GENERAL.key}` });
  fireEvent.change(within(form).getByLabelText("New key"), { target: { value: `${GENERAL.key}.a` } });
  fireEvent.click(within(form).getByRole("button", { name: "Preview" }));
  expect((await within(form).findByLabelText("Preview")).textContent).toContain("rating-methods.spec.ts:4");
  expect(changes(calls)[0].args.change).toEqual({ kind: "Rename", from: GENERAL.key, to: `${GENERAL.key}.a` });
  fireEvent.click(within(form).getByRole("button", { name: "Apply" }));
  await waitFor(() => expect(changes(calls)).toHaveLength(2));
  expect(changes(calls)[1].args.apply).toBe(true);
});

test("Delete shows the repo's refusal as it is", async () => {
  const refusal = `"${SUPERVISOR.key}" is still the supervisor of ${GENERAL.key}. Repoint or remove those first.`;
  const { calls } = mount({
    change: () => {
      throw refusal;
    },
  });
  await screen.findByText(SUPERVISOR.key);
  fireEvent.click(screen.getByRole("button", { name: `Delete ${SUPERVISOR.key}` }));
  const form = screen.getByRole("group", { name: `Delete ${SUPERVISOR.key}` });
  fireEvent.click(within(form).getByRole("button", { name: "Preview" }));
  expect(await screen.findByText(refusal)).toBeTruthy();
  expect(changes(calls)[0].args.change).toEqual({ kind: "Remove", key: SUPERVISOR.key });
  expect(within(form).queryByRole("button", { name: "Apply" })).toBeNull();
});
