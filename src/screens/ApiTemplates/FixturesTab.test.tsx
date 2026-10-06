// The Fixtures tab: each fixture with its steps, current outputs and last
// run; Run the first time and Rebuild after; a failed run that leaves the
// previous outputs on show; and Remove.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { toast } from "../../lib/toast";
import ApiTemplates from "./index";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const fixture = (id: string, name: string) => ({
  id,
  name,
  account: "hr.admin",
  steps: [{ template: "pms-create-draft-cycle", params: { name: "A" } }, { template: "pms-open-cycle" }],
});

const OK_RUN = { at: "2026-10-05 09:00:00", ok: true, outputs: { cycle_id: "272", name: "AUTOTEST A" } };

type Saved = { fixture: ReturnType<typeof fixture>; runs: unknown[] };

/** The tab over a store of fixtures `saved`; `run` answers api_fixture_run
 * and may change `saved`, as the Rust side saves a history. */
function mount(saved: Saved[], run: (id: string) => unknown = () => undefined) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      calls.push({ cmd, args: a });
      if (cmd === "api_templates_overview") return { origin: null, templates: [], flows: [] };
      if (cmd === "api_fixtures_list") return saved;
      if (cmd === "api_fixture_run") return run(a.id as string);
      if (cmd === "api_fixture_remove") {
        const i = saved.findIndex((s) => s.fixture.id === a.id);
        if (i >= 0) saved.splice(i, 1);
        return null;
      }
      return undefined;
    },
    { shouldMockEvents: true },
  );
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ApiTemplates org="acme" project="proj" onOpenAiBridge={vi.fn()} />
    </QueryClientProvider>,
  );
  return calls;
}

const report = (over: Record<string, unknown> = {}) => ({
  ok: true,
  outputs: {},
  made: [],
  steps: [],
  failed: null,
  warnings: [],
  ...over,
});

async function openTab() {
  fireEvent.click(await screen.findByRole("tab", { name: /^Fixtures/ }));
}

test("the tab lists each fixture with its account, steps, current outputs and last run", async () => {
  mount([{ fixture: fixture("draft-cycle", "Draft cycle"), runs: [OK_RUN] }]);
  await openTab();
  const row = await screen.findByRole("listitem", { name: "Draft cycle" });
  expect(row).toHaveTextContent("Runs as hr.admin");
  expect(row).toHaveTextContent("pms-create-draft-cycle, pms-open-cycle");
  expect(row).toHaveTextContent("cycle_id = 272, name = AUTOTEST A");
  expect(row).toHaveTextContent(/last run .*succeeded/);
});

test("a fixture that was never built offers Run, and Run calls the command then reads Rebuild", async () => {
  const saved: Saved[] = [{ fixture: fixture("draft-cycle", "Draft cycle"), runs: [] }];
  const calls = mount(saved, (id) => {
    saved[0].runs = [OK_RUN];
    return report({ outputs: OK_RUN.outputs, made: [] , warnings: [] , steps: [], failed: null, ok: true, id });
  });
  await openTab();
  const row = await screen.findByRole("listitem", { name: "Draft cycle" });
  expect(row).toHaveTextContent("never run");
  expect(row).toHaveTextContent("none yet");
  fireEvent.click(within(row).getByRole("button", { name: "Run Draft cycle" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Rebuild Draft cycle" })).toBeInTheDocument());
  expect(calls.find((c) => c.cmd === "api_fixture_run")!.args).toEqual({
    organization: "acme",
    project: "proj",
    id: "draft-cycle",
  });
  expect(screen.getByRole("listitem", { name: "Draft cycle" })).toHaveTextContent("cycle_id = 272");
});

test("while one fixture runs its button is busy and the others are off", async () => {
  let finish: (v: unknown) => void = () => {};
  const saved: Saved[] = [
    { fixture: fixture("a", "Alpha"), runs: [OK_RUN] },
    { fixture: fixture("b", "Beta"), runs: [OK_RUN] },
  ];
  mockIPC(
    (cmd, args) => {
      if (cmd === "api_templates_overview") return { origin: null, templates: [], flows: [] };
      if (cmd === "api_fixtures_list") return saved;
      if (cmd === "api_fixture_run") {
        void args;
        return new Promise((resolve) => {
          finish = resolve;
        });
      }
      return undefined;
    },
    { shouldMockEvents: true },
  );
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <ApiTemplates org="acme" project="proj" onOpenAiBridge={vi.fn()} />
    </QueryClientProvider>,
  );
  await openTab();
  await screen.findByRole("listitem", { name: "Alpha" });
  fireEvent.click(screen.getByRole("button", { name: "Rebuild Alpha" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Rebuild Alpha" })).toHaveTextContent("Running…"));
  expect(screen.getByRole("button", { name: "Rebuild Alpha" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Rebuild Beta" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Remove Beta" })).toBeDisabled();
  finish(report());
  await waitFor(() => expect(screen.getByRole("button", { name: "Rebuild Beta" })).toBeEnabled());
});

test("a failed Rebuild shows the failure and keeps the previous outputs", async () => {
  const saved: Saved[] = [{ fixture: fixture("draft-cycle", "Draft cycle"), runs: [OK_RUN] }];
  mount(saved, () => {
    saved[0].runs = [{ at: "2026-10-06 09:00:00", ok: false, failed_step: 2, detail: "x", outputs: {} }, OK_RUN];
    return report({ ok: false, failed: "step 2: the cycle could not be opened", warnings: [] });
  });
  await openTab();
  const row = await screen.findByRole("listitem", { name: "Draft cycle" });
  fireEvent.click(within(row).getByRole("button", { name: "Rebuild Draft cycle" }));
  expect(await within(row).findByRole("alert")).toHaveTextContent("step 2: the cycle could not be opened");
  // The last run now reads failed, the outputs are the last good build's.
  await waitFor(() => expect(row).toHaveTextContent(/last run .*failed/));
  expect(row).toHaveTextContent("cycle_id = 272, name = AUTOTEST A");
  expect(screen.getByRole("button", { name: "Rebuild Draft cycle" })).toBeEnabled();
});

test("a refused run shows Rust's sentence in the row", async () => {
  mount([{ fixture: fixture("draft-cycle", "Draft cycle"), runs: [OK_RUN] }], () => {
    throw "another template is running - wait for it to finish";
  });
  await openTab();
  const row = await screen.findByRole("listitem", { name: "Draft cycle" });
  fireEvent.click(within(row).getByRole("button", { name: "Rebuild Draft cycle" }));
  expect(await within(row).findByRole("alert")).toHaveTextContent("another template is running - wait for it to finish");
});

test("warnings from a run, such as the prefix warning, show in the row", async () => {
  mount([{ fixture: fixture("draft-cycle", "Draft cycle"), runs: [OK_RUN] }], () =>
    report({ warnings: ["step 1: the name does not start with AUTOTEST"] }),
  );
  await openTab();
  const row = await screen.findByRole("listitem", { name: "Draft cycle" });
  fireEvent.click(within(row).getByRole("button", { name: "Rebuild Draft cycle" }));
  expect(await within(row).findByText("step 1: the name does not start with AUTOTEST")).toBeInTheDocument();
});

test("Remove asks first, then removes the fixture and its row goes", async () => {
  const saved: Saved[] = [
    { fixture: fixture("a", "Alpha"), runs: [OK_RUN] },
    { fixture: fixture("b", "Beta"), runs: [] },
  ];
  const calls = mount(saved);
  await openTab();
  await screen.findByRole("listitem", { name: "Alpha" });
  fireEvent.click(screen.getByRole("button", { name: "Remove Alpha" }));
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("Remove Alpha?");
  expect(calls.some((c) => c.cmd === "api_fixture_remove")).toBe(false);
  fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(screen.queryByRole("listitem", { name: "Alpha" })).not.toBeInTheDocument());
  expect(calls.find((c) => c.cmd === "api_fixture_remove")!.args).toEqual({
    organization: "acme",
    project: "proj",
    id: "a",
  });
  expect(toast.success).toHaveBeenCalledWith("Removed Alpha.");
  expect(screen.getByRole("listitem", { name: "Beta" })).toBeInTheDocument();
});

test("keeping a fixture in the dialog removes nothing", async () => {
  const calls = mount([{ fixture: fixture("a", "Alpha"), runs: [] }]);
  await openTab();
  await screen.findByRole("listitem", { name: "Alpha" });
  fireEvent.click(screen.getByRole("button", { name: "Remove Alpha" }));
  fireEvent.click(await screen.findByRole("button", { name: "Keep it" }));
  expect(calls.some((c) => c.cmd === "api_fixture_remove")).toBe(false);
  expect(screen.getByRole("listitem", { name: "Alpha" })).toBeInTheDocument();
});
