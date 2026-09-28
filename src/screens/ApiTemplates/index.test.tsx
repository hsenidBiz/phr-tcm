// The API Templates tab: the templates an assistant has proven on this
// machine, for the owner to read and - the one action there is - remove.
//
// Everything here comes from local files through two commands and one
// event, so the tests mock exactly those: the overview, the remove, and
// the change event Rust emits when a prove or run finishes.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { saveApiWrites } from "../../lib/apiTemplates";
import ApiTemplates from "./index";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

function template(over: Record<string, unknown> = {}) {
  return {
    id: "pms-create-draft-cycle",
    title: "Create a draft performance cycle",
    module: "PMS / Performance Cycle",
    effect: "create",
    description: "Cycle setup and evaluation rules; leaves the cycle in Draft.",
    sources: ["Pages/PerformanceCycle/Index.CycleSetup.cshtml.cs:95"],
    antiforgery: { page: "/hr/pmsv10/performancecycle?mode=create" },
    params: [
      { name: "cycleName", type: "string", required: true, description: "Shown in the cycle list" },
      { name: "ratingMethodId", type: "number", required: true, lookup: "SELECT id FROM rating_method" },
    ],
    steps: [
      {
        name: "save setup",
        method: "POST",
        path: "/hr/pmsv10/performancecycle",
        query: { handler: "SaveCycleSetup" },
        form: { CycleName: "{{cycleName}}", RatingMethodId: "{{ratingMethodId}}" },
        expect: { status: 200, json: { success: true } },
        capture: { cycleId: "$.cycleId" },
      },
      {
        name: "save rules",
        method: "POST",
        path: "/hr/pmsv10/performancecycle",
        query: { handler: "SaveEvaluationRules" },
        json: { cycleId: "{{cycleId}}", selfEvaluation: true },
      },
    ],
    outputs: ["cycleId"],
    proven: {
      at: "2026-09-28 10:15:00",
      origin: "https://hrmmainphdev01.phrsandbox.dev",
      account: "hr.admin",
      outputs: { cycleId: 272 },
    },
    ...over,
  };
}

const RUNS = [
  { at: "2026-09-28 11:30:00", account: "hr.admin", ok: true, outputs: { cycleId: 301 } },
  {
    at: "2026-09-28 11:00:00",
    account: "hr.manager",
    ok: false,
    failed_step: "save rules",
    detail: "Expected success true, got false.",
    outputs: { cycleId: 300 },
  },
];

const OVERVIEW = {
  origin: "https://hrmmainphdev01.phrsandbox.dev",
  templates: [
    { template: template(), runs: RUNS },
    {
      template: template({
        id: "pms-publish-cycle",
        title: "Publish a performance cycle",
        effect: "edit",
        params: [{ name: "cycleId", type: "number", required: true }],
      }),
      runs: [],
    },
    {
      template: template({
        id: "goals-remove-goal",
        title: "Remove a goal",
        module: "PMS / Goals",
        effect: "delete",
      }),
      runs: [],
    },
  ],
};

type Handler = (cmd: string, args: unknown) => unknown;

function mockOverview(overview: unknown, extra: Handler = () => undefined) {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "api_templates_overview") return overview;
      return extra(cmd, args);
    },
    { shouldMockEvents: true },
  );
  return calls;
}

function renderScreen(onOpenAiBridge = vi.fn()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ApiTemplates org="acme" project="proj" onOpenAiBridge={onOpenAiBridge} />
    </QueryClientProvider>,
  );
  return { onOpenAiBridge };
}

test("groups templates by module and filters by title, module or id", async () => {
  mockOverview(OVERVIEW);
  renderScreen();

  const cycle = await screen.findByRole("region", { name: "PMS / Performance Cycle" });
  const goals = screen.getByRole("region", { name: "PMS / Goals" });
  expect(within(cycle).getByText("Create a draft performance cycle")).toBeInTheDocument();
  expect(within(cycle).getByText("Publish a performance cycle")).toBeInTheDocument();
  expect(within(goals).getByText("Remove a goal")).toBeInTheDocument();

  const search = screen.getByRole("textbox", { name: "Search templates" });

  // By title.
  fireEvent.change(search, { target: { value: "publish" } });
  expect(screen.getByText("Publish a performance cycle")).toBeInTheDocument();
  expect(screen.queryByText("Create a draft performance cycle")).not.toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "PMS / Goals" })).not.toBeInTheDocument();

  // By module.
  fireEvent.change(search, { target: { value: "goals" } });
  expect(screen.getByText("Remove a goal")).toBeInTheDocument();
  expect(screen.queryByText("Publish a performance cycle")).not.toBeInTheDocument();

  // By id.
  fireEvent.change(search, { target: { value: "pms-create-draft" } });
  expect(screen.getByText("Create a draft performance cycle")).toBeInTheDocument();
  expect(screen.queryByText("Remove a goal")).not.toBeInTheDocument();

  // Nothing matches: says so.
  fireEvent.change(search, { target: { value: "zzzz" } });
  expect(screen.getByText(/No template matches/)).toBeInTheDocument();
});

test("a row shows its effect badge, parameter count, proven line and last run", async () => {
  mockOverview(OVERVIEW);
  renderScreen();

  const create = await screen.findByRole("listitem", { name: "Create a draft performance cycle" });
  const badge = within(create).getByText("create");
  expect(badge.className).toContain("text-success");
  expect(badge.className).toContain("bg-success/15");
  expect(within(create).getByText("2 parameters")).toBeInTheDocument();
  expect(within(create).getByText("proven 28 Sep as hr.admin")).toBeInTheDocument();
  // The newest run is first: it succeeded.
  expect(within(create).getByText(/last run/)).toBeInTheDocument();
  expect(within(create).getByText("succeeded")).toBeInTheDocument();

  const publish = screen.getByRole("listitem", { name: "Publish a performance cycle" });
  const edit = within(publish).getByText("edit");
  expect(edit.className).toContain("text-warning");
  expect(edit.className).toContain("bg-warning/15");
  expect(within(publish).getByText("1 parameter")).toBeInTheDocument();
  expect(within(publish).getByText("never run")).toBeInTheDocument();

  const goal = screen.getByRole("listitem", { name: "Remove a goal" });
  const del = within(goal).getByText("delete");
  expect(del.className).toContain("text-danger");
  expect(del.className).toContain("bg-danger/15");
});

test("expanding a row shows params, steps, sources, evidence and runs, read-only", async () => {
  mockOverview(OVERVIEW);
  renderScreen();

  const row = await screen.findByRole("listitem", { name: "Create a draft performance cycle" });
  // Folded to start with: no detail on show.
  expect(within(row).queryByText("Cycle setup and evaluation rules; leaves the cycle in Draft.")).not.toBeInTheDocument();

  const toggle = within(row).getByRole("button", { name: "Show details of Create a draft performance cycle" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");

  const details = within(row).getByTestId("template-details");
  expect(within(details).getByText("Cycle setup and evaluation rules; leaves the cycle in Draft.")).toBeInTheDocument();

  // Parameters: name, type, required, description, lookup hint.
  const params = within(details).getByRole("table", { name: "Parameters" });
  expect(within(params).getByText("cycleName")).toBeInTheDocument();
  expect(within(params).getByText("Shown in the cycle list")).toBeInTheDocument();
  expect(within(params).getByText("ratingMethodId")).toBeInTheDocument();
  expect(within(params).getByText("SELECT id FROM rating_method")).toBeInTheDocument();

  // Steps in order: method, path + handler, body fields, expect, captures.
  const steps = within(details).getByRole("list", { name: "Steps" });
  const items = within(steps).getAllByRole("listitem");
  expect(items).toHaveLength(2);
  expect(items[0]).toHaveTextContent("POST");
  expect(items[0]).toHaveTextContent("/hr/pmsv10/performancecycle?handler=SaveCycleSetup");
  expect(items[0]).toHaveTextContent("CycleName, RatingMethodId");
  expect(items[0]).toHaveTextContent("status 200");
  expect(items[0]).toHaveTextContent("cycleId ← $.cycleId");
  expect(items[1]).toHaveTextContent("?handler=SaveEvaluationRules");
  expect(items[1]).toHaveTextContent("cycleId, selfEvaluation");

  // Sources as file:line.
  expect(within(details).getByText("Pages/PerformanceCycle/Index.CycleSetup.cshtml.cs:95")).toBeInTheDocument();

  // The proving evidence: what it created.
  const proof = within(details).getByTestId("template-proof");
  expect(proof).toHaveTextContent("cycleId: 272");
  expect(proof).toHaveTextContent("hr.admin");

  // The runs: time, account, ok or failed at a step, outputs.
  const runs = within(details).getByRole("list", { name: "Runs" });
  const runItems = within(runs).getAllByRole("listitem");
  expect(runItems).toHaveLength(2);
  expect(runItems[0]).toHaveTextContent("hr.admin");
  expect(runItems[0]).toHaveTextContent("cycleId: 301");
  expect(runItems[1]).toHaveTextContent("hr.manager");
  expect(runItems[1]).toHaveTextContent("failed at save rules");
  expect(runItems[1]).toHaveTextContent("Expected success true, got false.");

  // Read-only: nothing to type into, nothing to edit or run.
  expect(within(details).queryByRole("textbox")).not.toBeInTheDocument();
  expect(within(details).queryByRole("button")).not.toBeInTheDocument();
  expect(within(row).queryByRole("button", { name: /edit/i })).not.toBeInTheDocument();
  expect(within(row).queryByRole("button", { name: /^run/i })).not.toBeInTheDocument();

  fireEvent.click(within(row).getByRole("button", { name: "Hide details of Create a draft performance cycle" }));
  expect(within(row).queryByTestId("template-details")).not.toBeInTheDocument();
});

test("Remove asks first, names the template and its effect, then removes it", async () => {
  const calls = mockOverview(OVERVIEW, (cmd) => (cmd === "api_templates_remove" ? null : undefined));
  renderScreen();

  const row = await screen.findByRole("listitem", { name: "Create a draft performance cycle" });
  fireEvent.click(within(row).getByRole("button", { name: "Remove Create a draft performance cycle" }));

  let dialog = screen.getByRole("dialog");
  expect(within(dialog).getByRole("heading", { name: "Remove Create a draft performance cycle?" })).toBeInTheDocument();
  expect(dialog).toHaveTextContent("This template creates data.");
  expect(dialog).toHaveTextContent(
    "It is removed from this machine with its run history. There is no undo; the assistant can prove it again.",
  );

  // Keep it: nothing is removed.
  fireEvent.click(within(dialog).getByRole("button", { name: "Keep it" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  expect(calls.filter((c) => c.cmd === "api_templates_remove")).toHaveLength(0);

  // Remove: once, with the right id.
  fireEvent.click(within(row).getByRole("button", { name: "Remove Create a draft performance cycle" }));
  dialog = screen.getByRole("dialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "api_templates_remove")).toHaveLength(1));
  expect(calls.find((c) => c.cmd === "api_templates_remove")?.args).toEqual({
    organization: "acme",
    project: "proj",
    id: "pms-create-draft-cycle",
  });
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  // The list is read again after a removal.
  await waitFor(() => expect(calls.filter((c) => c.cmd === "api_templates_overview").length).toBeGreaterThan(1));
});

test("the remove dialog names the effect of an edit and a delete template", async () => {
  mockOverview(OVERVIEW);
  renderScreen();

  const publish = await screen.findByRole("listitem", { name: "Publish a performance cycle" });
  fireEvent.click(within(publish).getByRole("button", { name: "Remove Publish a performance cycle" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("This template edits data.");
  fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Keep it" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

  const goal = screen.getByRole("listitem", { name: "Remove a goal" });
  fireEvent.click(within(goal).getByRole("button", { name: "Remove Remove a goal" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("This template deletes data.");
});

test("the list refreshes when the change event arrives", async () => {
  // The answer changes mid-test, as it does when a prove saves a new one.
  let overview: unknown = { origin: OVERVIEW.origin, templates: [OVERVIEW.templates[0]] };
  mockIPC((cmd) => (cmd === "api_templates_overview" ? overview : undefined), { shouldMockEvents: true });
  renderScreen();

  await screen.findByText("Create a draft performance cycle");
  expect(screen.queryByText("Remove a goal")).not.toBeInTheDocument();

  overview = OVERVIEW;
  await act(() => emit("api-templates-changed", { id: "goals-remove-goal" }));
  expect(await screen.findByText("Remove a goal")).toBeInTheDocument();
});

test("the header shows the environment host and the switch state, which opens AI Bridge", async () => {
  mockOverview(OVERVIEW);
  const { onOpenAiBridge } = renderScreen();

  expect(await screen.findByText("hrmmainphdev01.phrsandbox.dev")).toBeInTheDocument();
  expect(screen.getByText("proj")).toBeInTheDocument();

  const pill = screen.getByRole("button", { name: /API templates off/ });
  fireEvent.click(pill);
  expect(onOpenAiBridge).toHaveBeenCalledTimes(1);

  // The switch lives on the AI Bridge tab; flipping it there shows here at once.
  act(() => saveApiWrites(true));
  expect(screen.getByRole("button", { name: /API templates on/ })).toBeInTheDocument();
});

test("a project with no sign-in recipe says so in place of a host", async () => {
  mockOverview({ origin: null, templates: [] });
  renderScreen();
  expect(await screen.findByText("no sign-in recipe yet")).toBeInTheDocument();
});

test("an empty project explains where templates come from", async () => {
  mockOverview({ origin: OVERVIEW.origin, templates: [] });
  const { onOpenAiBridge } = renderScreen();

  expect(
    await screen.findByText(
      "Your assistant builds these from the application's code and proves each one before it appears here. Connect one and turn on API templates on the AI Bridge tab.",
    ),
  ).toBeInTheDocument();
  expect(screen.queryByRole("textbox", { name: "Search templates" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Open AI Bridge" }));
  expect(onOpenAiBridge).toHaveBeenCalledTimes(1);
});
