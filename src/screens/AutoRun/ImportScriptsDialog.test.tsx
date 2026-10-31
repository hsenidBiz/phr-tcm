// Importing a folder of scripts: the three counts come first, Import writes
// without replacing unless the box is ticked, and a refusal stays in the dialog.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ImportScriptsPlan } from "../../bindings";
import { setActiveEnvId } from "../../lib/activeEnv";
import { toast } from "../../lib/toast";
import ImportScriptsDialog from "./ImportScriptsDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  vi.clearAllMocks();
  localStorage.clear();
});

const PLAN: ImportScriptsPlan = { new: [1, 4], same: [2], differ: [3, 5, 6], unreadable: 0 };

function mount(plan: ImportScriptsPlan, importResult?: () => unknown) {
  setActiveEnvId("env-00000001");
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    if (cmd === "auto_run_import_folder_plan") return plan;
    if (cmd === "auto_run_import_folder") return importResult ? importResult() : plan;
    return null;
  });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ImportScriptsDialog folder="D:/scripts" envName="QA" onClose={onClose} />
    </QueryClientProvider>,
  );
  return { calls, onClose };
}

const importCalls = <T extends { cmd: string }>(calls: T[]) => calls.filter((c) => c.cmd === "auto_run_import_folder");

test("shows how many scripts are new, the same, and different here", async () => {
  mount(PLAN);
  expect(await screen.findByText("2 new scripts will be added.")).toBeInTheDocument();
  expect(screen.getByText("1 is already the same.")).toBeInTheDocument();
  expect(screen.getByText("3 cases already have a different script here and are left as they are.")).toBeInTheDocument();
});

test("Import without the box ticked does not replace", async () => {
  const { calls, onClose } = mount(PLAN);
  fireEvent.click(await screen.findByRole("button", { name: "Import" }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(importCalls(calls)[0].args).toMatchObject({ folder: "D:/scripts", replace: false });
  expect(toast.success).toHaveBeenCalledWith("Imported 2 scripts into QA");
});

test("ticking Replace replaces and says how many", async () => {
  const { calls, onClose } = mount(PLAN);
  fireEvent.click(await screen.findByRole("checkbox", { name: "Replace those 3 scripts" }));
  fireEvent.click(screen.getByRole("button", { name: "Import" }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(importCalls(calls)[0].args).toMatchObject({ replace: true });
  expect(toast.success).toHaveBeenCalledWith("Imported 5 scripts into QA, replaced 3");
});

test("there is no Replace box when no case has a different script", async () => {
  mount({ ...PLAN, differ: [] });
  await screen.findByText("2 new scripts will be added.");
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
});

test("a refusal is shown in the dialog and Import stays available", async () => {
  const { onClose } = mount(PLAN, () => {
    throw "case 3 is being replayed right now - wait for the replay to end";
  });
  fireEvent.click(await screen.findByRole("button", { name: "Import" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("case 3 is being replayed right now");
  expect(onClose).not.toHaveBeenCalled();
  expect(toast.success).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Import" })).toBeEnabled();
});
