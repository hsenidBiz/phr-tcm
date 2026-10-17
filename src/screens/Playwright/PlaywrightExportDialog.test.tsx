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
    { case_id: 10, title: "Create a cycle", exportable: true, reason: null, seg: "sl/admin/pm/wizard", user_key: "REPO_ADMIN", add_user_command: null, has_script: true, warnings: [] },
    { case_id: 11, title: "Open the report", exportable: false, reason: "its newest run did not pass", seg: null, user_key: null, add_user_command: null, has_script: true, warnings: [] },
    {
      case_id: 12,
      title: "Approve the plan",
      exportable: false,
      reason: "the account kim has no user in the clone",
      seg: null,
      user_key: null,
      add_user_command: "npm run add-user -- kim <password>",
      has_script: true,
      warnings: [],
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
      warnings: [],
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

/** A folder field: the app's own picker, named for what it sets. */
const folderField = (name: string) => screen.getByRole("combobox", { name });

/** The paths a folder field offers in its list: opened, read once the clone's folders are in, closed again. */
async function offered(name: string) {
  fireEvent.click(folderField(name));
  const options = () => within(screen.getByRole("listbox")).getAllByRole("option");
  await waitFor(() => expect(options().length).toBeGreaterThan(0));
  const paths = options().map((o) => o.textContent);
  fireEvent.click(folderField(name));
  return paths;
}

/** Types a folder into a field's search box and presses Enter, as a person does. */
function typeFolder(name: string, text: string) {
  fireEvent.click(folderField(name));
  const box = screen.getByPlaceholderText("Search…");
  fireEvent.change(box, { target: { value: text } });
  fireEvent.keyDown(box, { key: "Enter" });
}

/** Empties a folder field with its own clear button. */
const clearFolder = (name: string) => fireEvent.click(within(folderField(name)).getByRole("button", { name: "Clear" }));

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
  // The clone's own features are offered in the app's own list, never the browser's.
  expect(await offered("Folder for Definition Wizard")).toEqual([
    "sl/admin/performance/definition-wizard/competencies",
    "sl/admin/performance/rating-methods",
  ]);
  expect(document.querySelector("datalist")).toBeNull();
  // Typed text is trimmed, and so are trailing slashes and spaces together; the case is the person's.
  typeFolder("Folder for Definition Wizard", "  sl/self/pm/wizard/ /  ");
  fireEvent.click(screen.getByRole("combobox", { name: "User for hr.admin" }));
  fireEvent.click(await screen.findByRole("option", { name: "REPO_EMP" }));
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_export_save_map")).toBeTruthy());
  expect(calls.find((c) => c.cmd === "pw_export_save_map")?.args).toEqual({
    organization: "acme",
    project: "Web",
    map: {
      areas: { "Definition Wizard": { path: "sl/self/pm/wizard" } },
      cases: {},
      components: {},
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
  // The PBI the dialog was opened for, so the written notes can repeat its saved order.
  expect(write.args).toMatchObject({ organization: "acme", project: "Web", caseIds: [10], replace: [], pbiId: 7 });
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
  // Slashes and spaces alone are no path: the typed text is kept, and it blocks the save.
  typeFolder("Folder for Definition Wizard", " / ");
  expect(folderField("Folder for Definition Wizard")).toHaveTextContent("/");
  expect(screen.getByText("needs a folder path")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save mappings" })).toBeDisabled();
  expect(calls.some((c) => c.cmd === "pw_export_save_map")).toBe(false);
  // Emptied with the field's own clear button, it is the same.
  typeFolder("Folder for Definition Wizard", "sl/admin/pm/wizard");
  expect(screen.queryByText("needs a folder path")).not.toBeInTheDocument();
  clearFolder("Folder for Definition Wizard");
  expect(screen.getByText("needs a folder path")).toBeInTheDocument();
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
  expect(folderField("Folder for Definition Wizard")).toHaveTextContent("sl/admin/pm/wizard");
  expect(folderField("Folder for Goal Setting")).toHaveTextContent("sl/admin/performance/goal-setting");
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
      warnings: [],
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
      warnings: [],
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
        warnings: [],
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
  const sentence = `Area "Definition Wizard": The path "sl/_x" cannot be used: "_x" starts with _ or . (or is node_modules), so the repo's tree never reads it.`;
  mount({
    pw_export_save_map: () => {
      throw sentence;
    },
  });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  typeFolder("Folder for Definition Wizard", "sl/_x");
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  expect(await screen.findByText(/the repo's tree never reads it/)).toBeInTheDocument();
  expect(folderField("Folder for Definition Wizard")).toHaveTextContent("sl/_x");
});

test("an area placed outside the clone's features says a new folder will be created", async () => {
  mount();
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  await offered("Folder for Definition Wizard");
  typeFolder("Folder for Definition Wizard", "sl/self/pm/wizard");
  expect(screen.getByText("A new folder will be created.")).toBeInTheDocument();
  // An existing feature's path, even with a trailing slash, needs no note.
  typeFolder("Folder for Definition Wizard", "sl/admin/performance/rating-methods/");
  expect(screen.queryByText("A new folder will be created.")).not.toBeInTheDocument();
});

test("one case can be given its own folder, saved with the mappings, and handed back to its area", async () => {
  const saved = { cases: { 10: { path: "sl/admin/performance/definition-wizard/DW-competencies" } } };
  const { calls } = mount();
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  // Where it goes now: its area's folder, from the preview.
  expect(screen.getByText("sl/admin/pm/wizard")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Give #10 its own folder" }));
  expect(folderField("Folder for #10")).toHaveTextContent("sl/admin/pm/wizard");
  // Not saved yet: Export waits for the mappings.
  typeFolder("Folder for #10", saved.cases[10].path);
  expect(screen.getByRole("button", { name: EXPORT })).toBeDisabled();
  expect(screen.getByText("A new folder will be created.")).toBeInTheDocument();
  // A blanked own folder blocks the save until it has a path.
  clearFolder("Folder for #10");
  expect(screen.getByText("needs a folder path")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save mappings" })).toBeDisabled();
  // Handed back: the input goes and the case follows its area again.
  fireEvent.click(screen.getByRole("button", { name: "Use the area's folder for #10" }));
  expect(screen.queryByRole("combobox", { name: "Folder for #10" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: EXPORT })).toBeEnabled();
  // Given again and saved: the trailing slash is trimmed.
  fireEvent.click(screen.getByRole("button", { name: "Give #10 its own folder" }));
  typeFolder("Folder for #10", `${saved.cases[10].path}/`);
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "pw_export_save_map")).toHaveLength(1));
  expect(calls.find((c) => c.cmd === "pw_export_save_map")!.args.map).toMatchObject(saved);
});

test("a saved own folder is shown for editing when the dialog opens", async () => {
  mount({}, { ...PREVIEW, map: { ...PREVIEW.map, cases: { 10: { path: "sl/x/Own Folder" } } } });
  expect(await screen.findByRole("combobox", { name: "Folder for #10" })).toHaveTextContent("sl/x/Own Folder");
  expect(screen.queryByText("Save the mappings to export.")).not.toBeInTheDocument();
});

// Folders suggested from case titles: the clone has a folder per feature of
// the wizard, and the area's folder is one of them.
const AREA_SEG = "sl/admin/performance/DW-wizard-landing";
const SUGGEST_STRUCTURE = {
  ...STRUCTURE,
  features: ["DW-preview", "DW-proficiency-levels", "DW-rating-methods", "DW-wizard-landing"].map((n) => feature(["sl", "admin", "performance", n])),
};
const row = (case_id: number, title: string, extra: Record<string, unknown> = {}) => ({
  case_id,
  title,
  exportable: true,
  reason: null,
  seg: AREA_SEG,
  user_key: "REPO_ADMIN",
  add_user_command: null,
  has_script: true,
  warnings: [],
  ...extra,
});
const SUGGEST_PREVIEW = {
  ...PREVIEW,
  areas: ["Wizard"],
  map: { ...PREVIEW.map, areas: { Wizard: { path: AREA_SEG } } },
  cases: [
    row(20, "Rating Methods - delete"),
    row(21, "Proficiency Levels - add a level"),
    row(22, "Wizard landing shows steps"),
    // Held back because its area has no folder yet: nothing to suggest beside.
    row(23, "Rating Methods - edit", { exportable: false, reason: 'area "Wizard" is not placed yet', seg: null }),
  ],
};
const SUGGEST_HANDLERS = { pw_repo_structure: () => SUGGEST_STRUCTURE };

test("a case whose title names one of the clone's folders is offered it, and taking it is a draft", async () => {
  mount(SUGGEST_HANDLERS, SUGGEST_PREVIEW);
  const use = await screen.findByRole("button", { name: "Use the suggested folder for #20" });
  const line = use.parentElement as HTMLElement;
  expect(within(line).getByText("Suggested: DW-rating-methods")).toHaveAttribute("title", "sl/admin/performance/DW-rating-methods");
  expect(screen.getByRole("button", { name: "Use the suggested folder for #21" })).toBeInTheDocument();
  // The area's own folder is no suggestion, and a case with no folder yet has nothing to sit beside.
  expect(screen.queryByRole("button", { name: "Use the suggested folder for #22" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Use the suggested folder for #23" })).not.toBeInTheDocument();
  expect(screen.getAllByText(/^Suggested:/)).toHaveLength(2);
  expect(screen.getByRole("button", { name: EXPORT })).toBeEnabled();

  fireEvent.click(use);
  // It is the case's own folder now, but only a draft: the mappings still have to be saved.
  expect(folderField("Folder for #20")).toHaveTextContent("sl/admin/performance/DW-rating-methods");
  expect(screen.queryByRole("button", { name: "Use the suggested folder for #20" })).not.toBeInTheDocument();
  expect(screen.getByText("Save the mappings first")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: EXPORT })).toBeDisabled();
  // One suggestion is left: no all-at-once button for a single one.
  expect(screen.queryByRole("button", { name: /suggested folders$/ })).not.toBeInTheDocument();
});

test("all the suggestions can be taken at once, then saved with the mappings", async () => {
  const { calls } = mount(SUGGEST_HANDLERS, SUGGEST_PREVIEW);
  fireEvent.click(await screen.findByRole("button", { name: "Use 2 suggested folders" }));
  expect(folderField("Folder for #20")).toHaveTextContent("sl/admin/performance/DW-rating-methods");
  expect(folderField("Folder for #21")).toHaveTextContent("sl/admin/performance/DW-proficiency-levels");
  expect(screen.queryByText(/^Suggested:/)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /suggested folders?$/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_export_save_map")).toBeTruthy());
  expect(calls.find((c) => c.cmd === "pw_export_save_map")!.args.map).toMatchObject({
    cases: {
      20: { path: "sl/admin/performance/DW-rating-methods" },
      21: { path: "sl/admin/performance/DW-proficiency-levels" },
    },
  });
});

test("a case with its own folder, or one handed back to its area, is offered only what fits", async () => {
  const saved = { ...SUGGEST_PREVIEW, map: { ...SUGGEST_PREVIEW.map, cases: { 20: { path: "sl/admin/performance/DW-preview" } } } };
  mount(SUGGEST_HANDLERS, saved);
  expect(await screen.findByRole("button", { name: "Use the suggested folder for #21" })).toBeInTheDocument();
  // #20 has a folder of its own: nothing is suggested over it.
  expect(screen.queryByRole("button", { name: "Use the suggested folder for #20" })).not.toBeInTheDocument();
  // Handed back to its area, it is offered the suggestion again.
  fireEvent.click(screen.getByRole("button", { name: "Use the area's folder for #20" }));
  expect(await screen.findByRole("button", { name: "Use the suggested folder for #20" })).toBeInTheDocument();
});

test("suggestions follow the area's folder as it is being edited, not only as it was saved", async () => {
  mount(SUGGEST_HANDLERS, SUGGEST_PREVIEW);
  expect(await screen.findByRole("button", { name: "Use the suggested folder for #20" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Use the suggested folder for #22" })).not.toBeInTheDocument();
  // The area is moved (unsaved) to the folder #20 was being offered, and away from #22's.
  typeFolder("Folder for Wizard", "sl/admin/performance/DW-rating-methods");
  expect(folderField("Folder for Wizard")).toHaveTextContent("sl/admin/performance/DW-rating-methods");
  expect(screen.queryByRole("button", { name: "Use the suggested folder for #20" })).not.toBeInTheDocument();
  const use = screen.getByRole("button", { name: "Use the suggested folder for #22" });
  expect(within(use.parentElement as HTMLElement).getByText("Suggested: DW-wizard-landing")).toBeInTheDocument();
  // A blanked area folder has nothing to sit beside.
  clearFolder("Folder for Wizard");
  expect(screen.queryByText(/^Suggested:/)).not.toBeInTheDocument();
});

/** A preview whose cases use two of TCM's components, in a clone with two common components. */
const WITH_COMPONENTS = {
  ...PREVIEW,
  components: ["Employee search", "Pick a date"],
  common_components: ["date-picker", "employee-search"],
  map: { areas: {}, accounts: {}, components: { "Pick a date": "date-picker" } },
};

/** The component fields' picks, as they read now. */
const componentField = (name: string) => screen.getByRole("combobox", { name: `Common component for ${name}` });

test("a_component_the_cases_use_can_be_mapped_to_a_common_component_and_saved", async () => {
  const { calls } = mount({}, WITH_COMPONENTS);
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(screen.getByRole("heading", { name: "Components" })).toBeInTheDocument();
  expect(componentField("Employee search")).toHaveTextContent("Not mapped");
  expect(componentField("Pick a date")).toHaveTextContent("date-picker");
  // The app's own list: Not mapped first, then the clone's common components.
  fireEvent.click(componentField("Employee search"));
  expect(within(screen.getByRole("listbox")).getAllByRole("option").map((o) => o.textContent)).toEqual([
    "Not mapped",
    "date-picker",
    "employee-search",
  ]);
  expect(document.querySelector("datalist")).toBeNull();
  fireEvent.click(screen.getByRole("option", { name: "employee-search" }));
  // A pick is a draft until it is saved.
  expect(screen.getByText("Save the mappings first")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: EXPORT })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_export_save_map")).toBeTruthy());
  expect((calls.find((c) => c.cmd === "pw_export_save_map")?.args.map as { components: unknown }).components).toEqual({
    "Employee search": "employee-search",
    "Pick a date": "date-picker",
  });
});

test("choosing_not_mapped_takes_a_saved_component_mapping_back", async () => {
  const { calls } = mount({}, WITH_COMPONENTS);
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  fireEvent.click(componentField("Pick a date"));
  fireEvent.click(screen.getByRole("option", { name: "Not mapped" }));
  expect(componentField("Pick a date")).toHaveTextContent("Not mapped");
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_export_save_map")).toBeTruthy());
  expect((calls.find((c) => c.cmd === "pw_export_save_map")?.args.map as { components: unknown }).components).toEqual({});
});

test("a_mapping_for_a_component_these_cases_do_not_use_is_kept_when_saving", async () => {
  const kept = { ...WITH_COMPONENTS, map: { areas: {}, accounts: {}, components: { "Other component": "date-picker" } } };
  const { calls } = mount({}, kept);
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(screen.queryByRole("combobox", { name: "Common component for Other component" })).not.toBeInTheDocument();
  fireEvent.click(componentField("Employee search"));
  fireEvent.click(screen.getByRole("option", { name: "employee-search" }));
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_export_save_map")).toBeTruthy());
  expect((calls.find((c) => c.cmd === "pw_export_save_map")?.args.map as { components: unknown }).components).toEqual({
    "Other component": "date-picker",
    "Employee search": "employee-search",
  });
});

test("a_saved_common_component_the_clone_no_longer_has_is_shown_with_a_hint", async () => {
  mount({}, { ...WITH_COMPONENTS, common_components: ["employee-search"] });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(screen.getByText("date-picker is not under suites/common-components in this clone")).toBeInTheDocument();
  expect(componentField("Pick a date")).toHaveTextContent("date-picker");
});

test("a_clone_without_common_components_says_so", async () => {
  mount({}, { ...WITH_COMPONENTS, common_components: [], map: { areas: {}, accounts: {}, components: {} } });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(screen.getByText("This clone has no common components yet.")).toBeInTheDocument();
  expect(screen.queryByRole("combobox", { name: /Common component for/ })).not.toBeInTheDocument();
});

test("cases_that_use_no_component_show_no_components_section", async () => {
  mount();
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(screen.queryByRole("heading", { name: "Components" })).not.toBeInTheDocument();
});

test("a_saved_blank_component_entry_does_not_make_the_mappings_look_unsaved", async () => {
  mount({}, { ...WITH_COMPONENTS, map: { areas: {}, accounts: {}, components: { "Pick a date": "date-picker", "Employee search": "" } } });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  expect(componentField("Employee search")).toHaveTextContent("Not mapped");
  expect(screen.queryByText("Save the mappings first")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: EXPORT })).toBeEnabled();
});

test("a case's warnings show under its row, for exportable and refused cases, and never stop Export", async () => {
  const warned = {
    ...PREVIEW,
    cases: [
      { ...PREVIEW.cases[0], warnings: ["step 2 appears to publish - irreversible", "not yet passed twice in a row in TCM"] },
      { ...PREVIEW.cases[1], warnings: ["1 TCM quirk travels with this case"] },
      PREVIEW.cases[2],
    ],
  };
  const { calls } = mount({ pw_export_write: () => ({ files: [], cases: [], user_keys: [], missing_navigation: [], sections: [], record_error: null, skipped: [] }) }, warned);
  const ok = await screen.findByRole("checkbox", { name: /Create a cycle/ });
  const row = ok.closest("label") as HTMLElement;
  for (const w of ["step 2 appears to publish - irreversible", "not yet passed twice in a row in TCM"]) {
    const line = within(row).getByText(w);
    expect(line).toHaveClass("text-warning");
    // The warning icon is decoration: the sentence is what is read out.
    expect(line.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
  }
  // A case that cannot go still shows what it carries; one with none shows nothing.
  expect(screen.getByText("1 TCM quirk travels with this case")).toHaveClass("text-warning");
  expect(screen.queryAllByText(/appears to|TCM quirk|passed twice/)).toHaveLength(3);
  // Export is untouched by them.
  const exportBtn = screen.getByRole("button", { name: "Export (1)" });
  expect(exportBtn).toBeEnabled();
  fireEvent.click(exportBtn);
  await waitFor(() => expect(calls.some((c) => c.cmd === "pw_export_write")).toBe(true));
});
