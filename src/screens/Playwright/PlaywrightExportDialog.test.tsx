// Export to Playwright: the clone folder, the area and account mappings, the
// cases that can and cannot go, and what the summary says after a write.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PlaywrightExportDialog, { SKIP_NOTE } from "./PlaywrightExportDialog";

const pick = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: pick, save: vi.fn() }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
});

const PREVIEW = {
  environment: "env1",
  clone_ok: true,
  clone_problem: null,
  user_keys: ["REPO_ADMIN", "REPO_EMP"],
  areas: ["Definition Wizard"],
  accounts: ["hr.admin"],
  map: { areas: {}, accounts: { other: { x: "Y" } } },
  suggested: {},
  cases: [
    { case_id: 10, title: "Create a cycle", exportable: true, reason: null, seg: "sl/admin/pm/wizard", user_key: "REPO_ADMIN", add_user_command: null, has_script: true },
    { case_id: 11, title: "Open the report", exportable: false, reason: "its newest run did not pass", seg: null, user_key: null, add_user_command: null, has_script: true },
    {
      case_id: 12,
      title: "Approve the plan",
      exportable: false,
      reason: "the account kim has no user in the clone",
      seg: null,
      user_key: null,
      add_user_command: "npm run add-user -- kim <password>",
      has_script: true,
    },
    {
      case_id: 15,
      title: "",
      exportable: false,
      reason: "no script - record or write one in Auto Run",
      seg: null,
      user_key: null,
      add_user_command: null,
      has_script: false,
    },
  ],
};

const feature = (path: string[]) => ({ path, navigation_key: path.slice(-2).join("/"), spec_count: 1, case_count: 0, case_ids: [] });
/** The clone's folders, as the Overview reads them. */
const STRUCTURE = {
  features: [
    feature(["sl", "admin", "performance", "definition-wizard", "competencies"]),
    feature(["sl", "admin", "performance", "rating-methods"]),
  ],
  generated_ids: [],
  user_keys: [],
  navigation_keys: [],
  health: [],
};

/** The paths the Folder field offers as you type. */
const offered = (input: HTMLElement) =>
  Array.from(document.querySelectorAll(`datalist#${input.getAttribute("list")} option`)).map((o) => (o as HTMLOptionElement).value);

/** The Export button, which says how many cases it sends. */
const EXPORT = /^Export \(\d+\)$/;

type Call = { cmd: string; args: Record<string, unknown> };

function mount(handlers: Record<string, (args: Record<string, unknown>) => unknown> = {}, preview: unknown = PREVIEW) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (handlers[cmd]) return handlers[cmd]((args ?? {}) as Record<string, unknown>);
    if (cmd === "pw_export_preview") return preview;
    if (cmd === "get_app_settings") return { playwright_clone: "C:/repo" };
    if (cmd === "pw_repo_structure") return STRUCTURE;
    if (cmd === "pw_handoff") {
      const ids = (args as { cases: number[] }).cases;
      return [{ seg: "sl/admin/pm", stem: "wizard", case_ids: ids, prompt: `finish ${ids.join(" ")}`, problem: null, already: [] }];
    }
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PlaywrightExportDialog
        org="acme"
        project="Web"
        pbiId={7}
        caseIds={[10, 11, 12, 15]}
        modules={[[10, "Definition Wizard"], [11, ""], [12, "Performance"], [15, "Performance"]]}
        onClose={onClose}
      />
    </QueryClientProvider>,
  );
  return { calls, onClose };
}

test("lists_reasons_and_only_ticks_exportable", async () => {
  mount();
  const ok = await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(ok).toHaveAttribute("aria-checked", "true");
  expect(screen.queryByRole("checkbox", { name: /Open the report/ })).not.toBeInTheDocument();
  expect(screen.getByText("its newest run did not pass")).toBeInTheDocument();
  expect(screen.getByText("npm run add-user -- kim <password>")).toBeInTheDocument();
  expect(screen.getByText("C:/repo")).toBeInTheDocument();
});

test("a case without a script is not listed at all, only counted", async () => {
  mount();
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(screen.queryByText("#15")).not.toBeInTheDocument();
  expect(screen.queryByText(/no script - record or write one/)).not.toBeInTheDocument();
  expect(screen.getByText("1 case without a script is not shown.")).toBeInTheDocument();
  // The cases are grouped by Module, and the summary counts what goes.
  const cases = screen.getByRole("region", { name: "Cases" });
  expect(within(within(cases).getByRole("group", { name: "Definition Wizard" })).getByText("Create a cycle")).toBeInTheDocument();
  expect(within(within(cases).getByRole("group", { name: "No module" })).getByText("Open the report")).toBeInTheDocument();
  expect(within(within(cases).getByRole("group", { name: "Performance" })).getByText("Approve the plan")).toBeInTheDocument();
  const summary = screen.getByLabelText("Summary");
  expect(within(summary).getByText("to export").previousElementSibling?.textContent).toBe("1");
  expect(within(summary).getByText("cannot export yet").previousElementSibling?.textContent).toBe("2");
  expect(screen.getByRole("button", { name: "Export (1)" })).toBeEnabled();
});

test("choosing_a_clone_saves_it", async () => {
  pick.mockResolvedValue("D:/work/clone");
  const { calls } = mount({ set_playwright_clone: () => ({ playwright_clone: "D:/work/clone" }) });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  fireEvent.click(screen.getByRole("button", { name: "Choose…" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "set_playwright_clone")?.args).toEqual({ path: "D:/work/clone" }));
  expect(pick).toHaveBeenCalledWith({ directory: true });
  expect(await screen.findByText("D:/work/clone")).toBeInTheDocument();
  await waitFor(() => expect(calls.filter((c) => c.cmd === "pw_export_preview").length).toBeGreaterThan(1));
});

test("saving_a_mapping_sends_the_map", async () => {
  const { calls } = mount();
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  const folder = screen.getByLabelText("Folder for Definition Wizard");
  // The clone's own features are offered as you type.
  await waitFor(() =>
    expect(offered(folder)).toEqual([
      "sl/admin/performance/definition-wizard/competencies",
      "sl/admin/performance/rating-methods",
    ]),
  );
  // Typed text is trimmed, and so are trailing slashes and spaces together; the case is the person's.
  fireEvent.change(folder, { target: { value: "  sl/self/pm/wizard/ /  " } });
  fireEvent.click(screen.getByRole("combobox", { name: "User for hr.admin" }));
  fireEvent.click(await screen.findByRole("option", { name: "REPO_EMP" }));
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_export_save_map")).toBeTruthy());
  expect(calls.find((c) => c.cmd === "pw_export_save_map")?.args).toEqual({
    organization: "acme",
    project: "Web",
    map: {
      areas: { "Definition Wizard": { path: "sl/self/pm/wizard" } },
      accounts: { other: { x: "Y" }, env1: { "hr.admin": "REPO_EMP" } },
    },
  });
  await waitFor(() => expect(calls.filter((c) => c.cmd === "pw_export_preview").length).toBeGreaterThan(1));
});

test("export_sends_the_ticked_ids_and_shows_the_summary", async () => {
  const { calls } = mount({
    pw_export_write: () => ({
      files: ["suites/sl/admin/pm/wizard/raw/a.spec.ts", "suites/sl/admin/pm/wizard/test-cases/wizard.md"],
      cases: [[10, "suites/sl/admin/pm/wizard/raw/a.spec.ts"]],
      user_keys: [[10, "REPO_ADMIN"]],
      missing_navigation: ["pm/wizard"],
    }),
  });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  fireEvent.click(screen.getByRole("button", { name: EXPORT }));
  expect(await screen.findByText("suites/sl/admin/pm/wizard/test-cases/wizard.md")).toBeInTheDocument();
  const write = calls.find((c) => c.cmd === "pw_export_write")!;
  expect(write.args).toMatchObject({ organization: "acme", project: "Web", caseIds: [10], replace: [] });
  // The same Modules the preview was given, so the write picks the same areas.
  const modules = [[10, "Definition Wizard"], [11, ""], [12, "Performance"], [15, "Performance"]];
  expect(write.args.modules).toEqual(modules);
  expect(calls.find((c) => c.cmd === "pw_export_preview")?.args.modules).toEqual(modules);
  expect(screen.getByText("#10 → suites/sl/admin/pm/wizard/raw/a.spec.ts - run as REPO_ADMIN")).toBeInTheDocument();
  expect(screen.getByText("pm/wizard")).toBeInTheDocument();
  expect(screen.getByText(/test-refactorer/)).toBeInTheDocument();
  expect(screen.getByText(/lint:tests -- --require-specs/)).toBeInTheDocument();
  // Finish in the repo: the prompt for the cases just written, in the clone chosen.
  expect(((await screen.findByLabelText("Prompt for sl/admin/pm/wizard")) as HTMLTextAreaElement).value).toBe("finish 10");
  expect(calls.find((c) => c.cmd === "pw_handoff")?.args).toMatchObject({ clone: "C:/repo", org: "acme", project: "Web", cases: [10] });
  expect(screen.getByText(/rewrites its section from Azure DevOps; the repo's Added by engineer, Also and Assertion floor blocks and every other section and file stay as they are/)).toBeInTheDocument();
});

test("a_refused_export_shows_the_reason_and_writes_nothing_else", async () => {
  mount({
    pw_export_write: () => {
      throw new Error("case 10 cannot be exported: its area is not mapped");
    },
  });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  fireEvent.click(screen.getByRole("button", { name: EXPORT }));
  expect(await screen.findByText("case 10 cannot be exported: its area is not mapped")).toBeInTheDocument();
  expect(screen.queryByText(/test-refactorer/)).not.toBeInTheDocument();
});

test("Export is disabled when the clone is not ok or nothing is ticked", async () => {
  mount({}, { ...PREVIEW, clone_ok: false, clone_problem: "that folder is not the repo" });
  expect(await screen.findByText("that folder is not the repo")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: EXPORT })).toBeDisabled();
});

test("unsaved mapping edits disable Export and say to save first", async () => {
  mount();
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(screen.getByRole("button", { name: EXPORT })).toBeEnabled();
  fireEvent.click(screen.getByRole("combobox", { name: "User for hr.admin" }));
  fireEvent.click(await screen.findByRole("option", { name: "REPO_EMP" }));
  expect(screen.getByText("Save the mappings first")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: EXPORT })).toBeDisabled();
});

test("a half-filled or blanked saved area row blocks the save", async () => {
  const saved = { ...PREVIEW, map: { areas: { "Definition Wizard": { path: "sl/admin/pm/wizard" } }, accounts: {} } };
  const { calls } = mount({}, saved);
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  fireEvent.change(screen.getByLabelText("Folder for Definition Wizard"), { target: { value: " / " } });
  expect(screen.getByText("needs a folder path")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save mappings" })).toBeDisabled();
  expect(calls.some((c) => c.cmd === "pw_export_save_map")).toBe(false);
});

test("a failed refresh after a write keeps the summary and is shown separately", async () => {
  let previews = 0;
  mount({
    pw_export_preview: () => {
      if (++previews > 1) throw new Error("preview broke");
      return PREVIEW;
    },
    pw_export_write: () => ({ files: ["a/b.md"], cases: [[10, "a/raw.spec.ts"]], user_keys: [], missing_navigation: [] }),
  });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  fireEvent.click(screen.getByRole("button", { name: EXPORT }));
  expect(await screen.findByText("a/b.md")).toBeInTheDocument();
  expect(await screen.findByText(/could not be refreshed: preview broke/)).toBeInTheDocument();
});

test("an_unplaced_area_is_filled_with_the_suggestion_and_must_be_saved_first", async () => {
  const suggested = {
    ...PREVIEW,
    areas: ["Definition Wizard", "Goal Setting"],
    map: { areas: { "Definition Wizard": { path: "sl/admin/pm/wizard" } }, accounts: {} },
    suggested: { "Goal Setting": { path: "sl/admin/performance/goal-setting" } },
  };
  const { calls } = mount({}, suggested);
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  // The saved row keeps what is saved; the unplaced one shows the guess.
  expect(screen.getByLabelText("Folder for Definition Wizard")).toHaveValue("sl/admin/pm/wizard");
  expect(screen.getByLabelText("Folder for Goal Setting")).toHaveValue("sl/admin/performance/goal-setting");
  // A guess is not a mapping until the person saves it.
  expect(screen.getByText("Save the mappings first")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: EXPORT })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_export_save_map")).toBeTruthy());
  expect((calls.find((c) => c.cmd === "pw_export_save_map")?.args.map as { areas: unknown }).areas).toEqual({
    "Definition Wizard": { path: "sl/admin/pm/wizard" },
    "Goal Setting": { path: "sl/admin/performance/goal-setting" },
  });
});

test("a_saved_user_the_clone_no_longer_has_is_shown_with_a_hint", async () => {
  mount({}, { ...PREVIEW, map: { areas: {}, accounts: { env1: { "hr.admin": "GONE_KEY" } } } });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(screen.getByText("GONE_KEY is not in this clone's users.json")).toBeInTheDocument();
  expect(screen.getByRole("combobox", { name: "User for hr.admin" })).toHaveTextContent("GONE_KEY");
});

const IN_REPO = {
  ...PREVIEW,
  cases: [
    { ...PREVIEW.cases[0], in_repo: false, repo_note: null, from_app: false },
    {
      case_id: 13,
      title: "Close the cycle",
      exportable: true,
      reason: null,
      seg: "sl/admin/pm/wizard",
      user_key: "REPO_ADMIN",
      add_user_command: null,
      in_repo: true,
      repo_note: "in wizard.md and specs/wizard.spec.ts",
      from_app: false,
      has_script: true,
    },
    {
      case_id: 14,
      title: "Reopen the cycle",
      exportable: true,
      reason: null,
      seg: "sl/admin/pm/wizard",
      user_key: "REPO_ADMIN",
      add_user_command: null,
      in_repo: true,
      repo_note: "in wizard.md and index.json",
      from_app: true,
      has_script: true,
    },
  ],
};

test("new cases are ticked and cases the repo has are not, each saying where", async () => {
  mount({}, IN_REPO);
  expect(await screen.findByRole("checkbox", { name: /Create a cycle/ })).toHaveAttribute("aria-checked", "true");
  expect(screen.getByRole("checkbox", { name: /Close the cycle/ })).toHaveAttribute("aria-checked", "false");
  expect(screen.getByRole("checkbox", { name: /Reopen the cycle/ })).toHaveAttribute("aria-checked", "false");
  expect(screen.getByText(SKIP_NOTE)).toBeInTheDocument();
  expect(SKIP_NOTE).toBe("Cases already in the repo are skipped. Tick one to replace it.");
  expect(screen.getByText("New")).toBeInTheDocument();
  expect(screen.getAllByText("In the repo")).toHaveLength(2);
  expect(screen.getByText("in wizard.md and specs/wizard.spec.ts")).toBeInTheDocument();
  expect(screen.getByText("in wizard.md and index.json (exported by this app)")).toBeInTheDocument();
  expect(document.body.textContent).not.toMatch(/[–—]/);
});

test("ticking a case the repo has sends it as a replace", async () => {
  const { calls } = mount(
    { pw_export_write: () => ({ files: ["a/b.md"], cases: [], user_keys: [], missing_navigation: [] }) },
    IN_REPO,
  );
  // Left as it is, only the new case goes and nothing is replaced.
  fireEvent.click(await screen.findByRole("button", { name: EXPORT }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "pw_export_write")).toHaveLength(1));
  expect(calls.filter((c) => c.cmd === "pw_export_write")[0].args).toMatchObject({ caseIds: [10], replace: [] });

  fireEvent.click(screen.getByRole("checkbox", { name: /Close the cycle/ }));
  expect(screen.getByRole("checkbox", { name: /Close the cycle/ })).toHaveAttribute("aria-checked", "true");
  fireEvent.click(screen.getByRole("button", { name: EXPORT }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "pw_export_write")).toHaveLength(2));
  expect(calls.filter((c) => c.cmd === "pw_export_write")[1].args).toMatchObject({ caseIds: [10, 13], replace: [13] });
});

test("a case the repo excludes is unticked, cannot be ticked and is never sent", async () => {
  const excluded = {
    ...IN_REPO,
    cases: [
      IN_REPO.cases[0],
      {
        case_id: 16,
        title: "Copy from Previous",
        exportable: false,
        reason: null,
        seg: "sl/admin/pm/wizard",
        user_key: "REPO_ADMIN",
        add_user_command: null,
        in_repo: true,
        repo_note: "Excluded in the repo (in pc-cycle-setup.md)",
        from_app: false,
        excluded: true,
        has_script: true,
      },
    ],
  };
  const { calls } = mount(
    { pw_export_write: () => ({ files: ["a/b.md"], cases: [], user_keys: [], missing_navigation: [] }) },
    excluded,
  );
  const box = await screen.findByRole("checkbox", { name: /Copy from Previous/ });
  expect(box).toHaveAttribute("aria-checked", "false");
  expect(box.hasAttribute("disabled") || box.getAttribute("aria-disabled") === "true" || box.hasAttribute("data-disabled")).toBe(true);
  expect(screen.getByText("Excluded in the repo (in pc-cycle-setup.md)")).toBeInTheDocument();
  fireEvent.click(box);
  fireEvent.click(screen.getByRole("button", { name: EXPORT }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "pw_export_write")).toHaveLength(1));
  expect(calls.find((c) => c.cmd === "pw_export_write")!.args).toMatchObject({ caseIds: [10], replace: [] });
});

test("a refused path shows its sentence and nothing is lost", async () => {
  const sentence = 'The folder "Sl" must be lowercase letters, digits and hyphens, not starting or ending with a hyphen.';
  mount({
    pw_export_save_map: () => {
      throw sentence;
    },
  });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  const folder = screen.getByLabelText("Folder for Definition Wizard");
  fireEvent.change(folder, { target: { value: "Sl/admin/x" } });
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  expect(await screen.findByText(/must be lowercase letters, digits and hyphens/)).toBeInTheDocument();
  expect(folder).toHaveValue("Sl/admin/x");
});
