// Export to Playwright: the clone folder, the area and account mappings, the
// cases that can and cannot go, and what the summary says after a write.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import PlaywrightExportDialog from "./PlaywrightExportDialog";

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
  cases: [
    { case_id: 10, title: "Create a cycle", exportable: true, reason: null, seg: "sl/admin/pm/wizard", user_key: "REPO_ADMIN", add_user_command: null },
    { case_id: 11, title: "Open the report", exportable: false, reason: "its newest run did not pass", seg: null, user_key: null, add_user_command: null },
    {
      case_id: 12,
      title: "Approve the plan",
      exportable: false,
      reason: "the account kim has no user in the clone",
      seg: null,
      user_key: null,
      add_user_command: "npm run add-user -- kim <password>",
    },
  ],
};

type Call = { cmd: string; args: Record<string, unknown> };

function mount(handlers: Record<string, (args: Record<string, unknown>) => unknown> = {}, preview: unknown = PREVIEW) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (handlers[cmd]) return handlers[cmd]((args ?? {}) as Record<string, unknown>);
    if (cmd === "pw_export_preview") return preview;
    if (cmd === "get_app_settings") return { playwright_clone: "C:/repo" };
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <PlaywrightExportDialog org="acme" project="Web" pbiId={7} caseIds={[10, 11, 12]} onClose={onClose} />
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
  fireEvent.change(screen.getByLabelText("Module for Definition Wizard"), { target: { value: "pm" } });
  fireEvent.change(screen.getByLabelText("Feature for Definition Wizard"), { target: { value: "wizard" } });
  fireEvent.click(screen.getByRole("combobox", { name: "Side for Definition Wizard" }));
  fireEvent.click(await screen.findByRole("option", { name: "self" }));
  fireEvent.click(screen.getByRole("combobox", { name: "User for hr.admin" }));
  fireEvent.click(await screen.findByRole("option", { name: "REPO_EMP" }));
  fireEvent.click(screen.getByRole("button", { name: "Save mappings" }));
  await waitFor(() => expect(calls.find((c) => c.cmd === "pw_export_save_map")).toBeTruthy());
  expect(calls.find((c) => c.cmd === "pw_export_save_map")?.args).toEqual({
    organization: "acme",
    project: "Web",
    map: {
      areas: { "Definition Wizard": { side: "self", module: "pm", feature: "wizard" } },
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
      missing_navigation: ["pm/wizard"],
    }),
  });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  fireEvent.click(screen.getByRole("button", { name: "Export" }));
  expect(await screen.findByText("suites/sl/admin/pm/wizard/test-cases/wizard.md")).toBeInTheDocument();
  const write = calls.find((c) => c.cmd === "pw_export_write")!;
  expect(write.args).toMatchObject({ organization: "acme", project: "Web", caseIds: [10] });
  expect(screen.getByText("#10 → suites/sl/admin/pm/wizard/raw/a.spec.ts")).toBeInTheDocument();
  expect(screen.getByText("pm/wizard")).toBeInTheDocument();
  expect(screen.getByText(/test-refactorer/)).toBeInTheDocument();
  expect(screen.getByText(/lint:tests -- --require-specs/)).toBeInTheDocument();
  expect(screen.getByText(/replaces that case's whole section/)).toBeInTheDocument();
});

test("a_refused_export_shows_the_reason_and_writes_nothing_else", async () => {
  mount({
    pw_export_write: () => {
      throw new Error("case 10 cannot be exported: its area is not mapped");
    },
  });
  await screen.findByRole("checkbox", { name: /Create a cycle/ });
  fireEvent.click(screen.getByRole("button", { name: "Export" }));
  expect(await screen.findByText("case 10 cannot be exported: its area is not mapped")).toBeInTheDocument();
  expect(screen.queryByText(/test-refactorer/)).not.toBeInTheDocument();
});

test("Export is disabled when the clone is not ok or nothing is ticked", async () => {
  mount({}, { ...PREVIEW, clone_ok: false, clone_problem: "that folder is not the repo" });
  expect(await screen.findByText("that folder is not the repo")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Export" })).toBeDisabled();
});
