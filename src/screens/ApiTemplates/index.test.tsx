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

function renderScreen(onOpenAiBridge = vi.fn(), project = "proj") {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ApiTemplates org="acme" project={project} onOpenAiBridge={onOpenAiBridge} />
    </QueryClientProvider>,
  );
  return { onOpenAiBridge };
}

/** Over to the Flows view, where the maps are, once the overview is in. */
async function showFlows() {
  fireEvent.click(await screen.findByRole("tab", { name: /^Flows/ }));
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

test("a row shows its effect badge and last run, and keeps its parameters and proof for its details", async () => {
  mockOverview(OVERVIEW);
  renderScreen();

  const create = await screen.findByRole("listitem", { name: "Create a draft performance cycle" });
  const badge = within(create).getByText("create");
  expect(badge.className).toContain("text-success");
  expect(badge.className).toContain("bg-success/15");
  // The count and the proof are in the details, not the line to scan.
  expect(within(create).queryByText(/parameters?$/)).toBeNull();
  expect(within(create).queryByText(/^proven /)).toBeNull();
  // The newest run is first: it succeeded.
  expect(within(create).getByText(/last run/)).toBeInTheDocument();
  expect(within(create).getByText("succeeded")).toBeInTheDocument();

  const publish = screen.getByRole("listitem", { name: "Publish a performance cycle" });
  const edit = within(publish).getByText("edit");
  expect(edit.className).toContain("text-warning");
  expect(edit.className).toContain("bg-warning/15");
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

test("the last run is the newest run - a newer prove is history, not a run", async () => {
  const runs = [
    { at: "2026-09-28 12:00:00", mode: "prove", account: "hr.admin", ok: true, outputs: { cycleId: 302 } },
    {
      at: "2026-09-28 11:00:00",
      mode: "run",
      account: "hr.manager",
      ok: false,
      failed_step: "save rules",
      detail: "Expected success true, got false.",
      outputs: {},
    },
  ];
  mockOverview({ origin: OVERVIEW.origin, templates: [{ template: template(), runs }] });
  renderScreen();

  const row = await screen.findByRole("listitem", { name: "Create a draft performance cycle" });
  // The dot and time are the run's, which failed - not the newer prove's.
  expect(within(row).getByText("failed")).toBeInTheDocument();
  expect(within(row).queryByText("succeeded")).not.toBeInTheDocument();

  // Every history line says which it was.
  fireEvent.click(within(row).getByRole("button", { name: "Show details of Create a draft performance cycle" }));
  const lines = within(within(row).getByRole("list", { name: "Runs" })).getAllByRole("listitem");
  expect(lines).toHaveLength(2);
  expect(within(lines[0]).getByText("prove")).toBeInTheDocument();
  expect(within(lines[1]).getByText("run")).toBeInTheDocument();
  expect(within(row).queryByText("Not run since it was proven.")).not.toBeInTheDocument();
});

test("a template only ever proven says it has not run since", async () => {
  const runs = [{ at: "2026-09-28 12:00:00", mode: "prove", account: "hr.admin", ok: true, outputs: { cycleId: 302 } }];
  mockOverview({ origin: OVERVIEW.origin, templates: [{ template: template(), runs }] });
  renderScreen();

  const row = await screen.findByRole("listitem", { name: "Create a draft performance cycle" });
  expect(within(row).getByText("never run")).toBeInTheDocument();

  fireEvent.click(within(row).getByRole("button", { name: "Show details of Create a draft performance cycle" }));
  const details = within(row).getByTestId("template-details");
  expect(within(details).getByText("Not run since it was proven.")).toBeInTheDocument();
  // The prove itself is still in the history, named as one.
  const lines = within(within(details).getByRole("list", { name: "Runs" })).getAllByRole("listitem");
  expect(lines).toHaveLength(1);
  expect(within(lines[0]).getByText("prove")).toBeInTheDocument();
});

test("a history line written before modes were recorded reads as a run", async () => {
  mockOverview(OVERVIEW);
  renderScreen();

  const row = await screen.findByRole("listitem", { name: "Create a draft performance cycle" });
  fireEvent.click(within(row).getByRole("button", { name: "Show details of Create a draft performance cycle" }));
  const lines = within(within(row).getByRole("list", { name: "Runs" })).getAllByRole("listitem");
  expect(within(lines[0]).getByText("run")).toBeInTheDocument();
  expect(within(lines[1]).getByText("run")).toBeInTheDocument();
});

// ---------------------------------------------------------------------------
// Flows: each module's wizard drawn as a map above its templates (spec §8).

/** The spec's performance cycle flow (§3). */
const FLOW = {
  id: "pms-performance-cycle",
  title: "Performance cycle wizard",
  module: "PMS / Performance Cycle",
  subject: { name: "cycleId", type: "number" },
  sources: ["Pages/PerformanceCycle/Index.cshtml.cs:40"],
  stages: [
    { id: "setup", title: "Cycle setup", creates: true, check: "SELECT 1" },
    { id: "rules", title: "Evaluation rules", requires: ["setup"], check: "SELECT 1" },
    { id: "competencies", title: "Competencies", requires: ["rules"], optional: true, check: "SELECT 1" },
    { id: "participants", title: "Participants", requires: ["rules"], check: "SELECT 1" },
    { id: "publish", title: "Publish", requires: ["participants"], check: "SELECT 1" },
  ],
  saved: { at: "2026-09-29 09:00:00", sample: 273 },
};

const FLOW_OVERVIEW = {
  origin: OVERVIEW.origin,
  templates: [
    {
      template: template({
        id: "pms-save-rules",
        title: "Save the rules",
        effect: "edit",
        stage: { flow: "pms-performance-cycle", id: "rules" },
      }),
      runs: [],
    },
    {
      // Its stage was in an earlier save of the flow, not this one.
      template: template({
        id: "pms-add-reviewer",
        title: "Add a reviewer",
        stage: { flow: "pms-performance-cycle", id: "reviewers" },
      }),
      runs: [],
    },
  ],
  flows: [FLOW],
};

test("the flow map's text equivalent names every stage in order, with what it requires", async () => {
  mockOverview(FLOW_OVERVIEW);
  renderScreen();
  await showFlows();

  const flow = await screen.findByRole("region", { name: "Performance cycle wizard" });
  // It sits inside its module, above the templates.
  const module = screen.getByRole("region", { name: "PMS / Performance Cycle" });
  expect(module).toContainElement(flow);

  expect(within(flow).getByText("Tracks cycleId")).toBeInTheDocument();
  expect(within(flow).getByText(/Saved 29 Sep/)).toBeInTheDocument();

  const list = within(flow).getByRole("list", { name: "Stages of Performance cycle wizard" });
  expect(list.tagName).toBe("OL");
  expect(list.className).toContain("sr-only");
  const items = within(list)
    .getAllByRole("listitem")
    .map((li) => li.textContent);
  expect(items).toEqual([
    "Cycle setup. Requires: nothing. Templates: none yet.",
    "Evaluation rules. Requires: Cycle setup. Templates: Save the rules.",
    "Competencies. Requires: Evaluation rules. Optional. Templates: none yet.",
    "Participants. Requires: Evaluation rules. Templates: none yet.",
    "Publish. Requires: Participants. Templates: none yet.",
  ]);

  // The drawing itself is for the eye only; one arrow per requires.
  const svg = within(flow).getByTestId("flow-map").querySelector("svg");
  expect(svg).not.toBeNull();
  expect(svg).toHaveAttribute("aria-hidden", "true");
  expect(svg!.querySelectorAll("path[data-edge]")).toHaveLength(4);
});

test("the map marks the optional stage and the stages no template performs yet", async () => {
  mockOverview(FLOW_OVERVIEW);
  renderScreen();
  await showFlows();

  const flow = await screen.findByRole("region", { name: "Performance cycle wizard" });
  expect(within(flow).getAllByText("Optional")).toHaveLength(1);
  const competencies = within(flow).getByRole("group", { name: "Competencies" });
  expect(competencies.className).toContain("border-dashed");
  expect(within(flow).getByRole("group", { name: "Participants" }).className).not.toContain("border-dashed");

  for (const name of ["Competencies", "Participants", "Publish"]) {
    const empty = within(within(flow).getByRole("group", { name })).getByText("No template yet");
    expect(empty.className).toContain("text-faint");
  }
  const rules = within(flow).getByRole("group", { name: "Evaluation rules" });
  expect(within(rules).queryByText("No template yet")).toBeNull();

  // A template on a stage shows its effect badge, as in the list.
  const badge = within(rules).getByText("edit");
  expect(badge.className).toContain("text-warning");
});

test("clicking a template in the map switches to Templates and opens its row", async () => {
  mockOverview(FLOW_OVERVIEW);
  renderScreen();
  await showFlows();

  const flow = await screen.findByRole("region", { name: "Performance cycle wizard" });
  // The Flows view shows flows only.
  expect(screen.queryByRole("listitem", { name: "Save the rules" })).toBeNull();

  const scroll = vi.spyOn(Element.prototype, "scrollIntoView");
  fireEvent.click(within(flow).getByRole("button", { name: /^Save the rules/ }));

  expect(screen.getByRole("tab", { name: /^Templates/ })).toHaveAttribute("aria-selected", "true");
  expect(screen.queryByRole("region", { name: "Performance cycle wizard" })).toBeNull();
  const row = screen.getByRole("listitem", { name: "Save the rules" });
  expect(row).toHaveAttribute("id", "api-template-pms-save-rules");
  expect(within(row).getByRole("button", { name: "Hide details of Save the rules" })).toHaveAttribute(
    "aria-expanded",
    "true",
  );
  expect(scroll).toHaveBeenCalledWith({ block: "nearest" });
  expect(scroll.mock.contexts[0]).toBe(row);
  scroll.mockRestore();

  // The row's own toggle still closes it.
  fireEvent.click(within(row).getByRole("button", { name: "Hide details of Save the rules" }));
  expect(within(row).getByRole("button", { name: "Show details of Save the rules" })).toHaveAttribute(
    "aria-expanded",
    "false",
  );
});

test("Templates and Flows are separate views, and the one chosen is remembered", async () => {
  mockOverview(FLOW_OVERVIEW);
  renderScreen();

  const templatesTab = await screen.findByRole("tab", { name: "Templates 2" });
  const flowsTab = screen.getByRole("tab", { name: "Flows 1" });
  expect(templatesTab).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("listitem", { name: "Save the rules" })).toBeInTheDocument();
  expect(screen.queryByRole("region", { name: "Performance cycle wizard" })).toBeNull();

  fireEvent.click(flowsTab);
  expect(flowsTab).toHaveAttribute("aria-selected", "true");
  expect(screen.getByRole("region", { name: "Performance cycle wizard" })).toBeInTheDocument();
  expect(screen.queryByRole("listitem", { name: "Save the rules" })).toBeNull();
  expect(screen.getByRole("textbox", { name: "Search flows" })).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-api-templates-view")).toBe("flows");
});

test("a project with templates but no flows says so in the Flows view", async () => {
  mockOverview(OVERVIEW);
  renderScreen();
  await showFlows();
  expect(await screen.findByText(/No flows yet/)).toBeInTheDocument();
});

test("a template row names its stage, and says when that stage is no longer saved", async () => {
  mockOverview(FLOW_OVERVIEW);
  renderScreen();

  const rules = await screen.findByRole("listitem", { name: "Save the rules" });
  // The row names the stage; its details name the flow.
  const ok = within(rules).getByText("Stage: Evaluation rules");
  expect(ok.className).not.toContain("text-warning");
  fireEvent.click(within(rules).getByRole("button", { name: "Show details of Save the rules" }));
  expect(within(rules).getByTestId("template-flow")).toHaveTextContent("Evaluation rules, in Performance cycle wizard");

  const orphan = screen.getByRole("listitem", { name: "Add a reviewer" });
  const line = within(orphan).getByText(/^Stage:/);
  expect(line).toHaveTextContent("no longer saved");
  expect(line.className).toContain("text-warning");
});

test("a template on no flow has no stage line", async () => {
  mockOverview(OVERVIEW);
  renderScreen();
  const row = await screen.findByRole("listitem", { name: "Create a draft performance cycle" });
  expect(within(row).queryByText(/^Stage:/)).toBeNull();
});

test("Remove flow asks first, says the templates stay, then removes it and reloads", async () => {
  const calls = mockOverview(FLOW_OVERVIEW, (cmd) => (cmd === "api_templates_remove_flow" ? null : undefined));
  renderScreen(vi.fn(), "Web");
  await showFlows();

  const flow = await screen.findByRole("region", { name: "Performance cycle wizard" });
  fireEvent.click(within(flow).getByRole("button", { name: /^Remove flow/ }));

  const dialog = screen.getByRole("dialog");
  expect(within(dialog).getByRole("heading", { name: "Remove Performance cycle wizard?" })).toBeInTheDocument();
  expect(dialog).toHaveTextContent(
    "2 templates perform its stages; they stay, and are refused until a flow with their stage is saved again.",
  );

  fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));
  await waitFor(() => expect(calls.filter((c) => c.cmd === "api_templates_remove_flow")).toHaveLength(1));
  expect(calls.find((c) => c.cmd === "api_templates_remove_flow")?.args).toEqual({
    organization: "acme",
    project: "Web",
    id: "pms-performance-cycle",
  });
  await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
  // Removing a flow emits no change event: the tab reads again itself.
  await waitFor(() => expect(calls.filter((c) => c.cmd === "api_templates_overview").length).toBeGreaterThan(1));
});

test("searching a stage title keeps the templates on that stage, and the flow in the Flows view", async () => {
  mockOverview(FLOW_OVERVIEW);
  renderScreen();

  await screen.findByRole("listitem", { name: "Save the rules" });
  fireEvent.change(screen.getByRole("textbox", { name: "Search templates" }), { target: { value: "evaluation" } });
  expect(screen.getByRole("listitem", { name: "Save the rules" })).toBeInTheDocument();
  expect(screen.queryByRole("listitem", { name: "Add a reviewer" })).not.toBeInTheDocument();

  await showFlows();
  const search = screen.getByRole("textbox", { name: "Search flows" });
  expect(screen.getByRole("region", { name: "Performance cycle wizard" })).toBeInTheDocument();

  // The flow's own title finds it too.
  fireEvent.change(search, { target: { value: "wizard" } });
  expect(screen.getByRole("region", { name: "Performance cycle wizard" })).toBeInTheDocument();

  // Nothing about the flow matches: it goes.
  fireEvent.change(search, { target: { value: "zzzz" } });
  expect(screen.queryByRole("region", { name: "Performance cycle wizard" })).not.toBeInTheDocument();
  expect(screen.getByText(/No flow matches/)).toBeInTheDocument();
});

test("a module with a flow and no templates still shows its map", async () => {
  mockOverview({ origin: OVERVIEW.origin, templates: [], flows: [FLOW] });
  renderScreen();
  await showFlows();

  const flow = await screen.findByRole("region", { name: "Performance cycle wizard" });
  expect(within(flow).getAllByText("No template yet")).toHaveLength(5);
  expect(screen.getByRole("region", { name: "PMS / Performance Cycle" })).toContainElement(flow);
});

test("a wide map scrolls inside its own box", async () => {
  mockOverview(FLOW_OVERVIEW);
  renderScreen();
  await showFlows();

  const flow = await screen.findByRole("region", { name: "Performance cycle wizard" });
  const map = within(flow).getByTestId("flow-map");
  expect(map.className).toContain("overflow-x-auto");
  // The drawing is sized from the layout, never measured: four columns.
  const canvas = map.firstElementChild as HTMLElement;
  expect(canvas.style.width).toBe(`${4 * 208 + 3 * 56}px`);
});

// ---------------------------------------------------------------------------
// The map's arrows take the colour of what the stage they lead into does.

test("each arrow is coloured by what its stage does, and a pulse runs along it", async () => {
  const on = (id: string, title: string, effect: string, stage: string) => ({
    template: template({ id, title, effect, stage: { flow: "pms-performance-cycle", id: stage } }),
    runs: [],
  });
  mockOverview({
    origin: OVERVIEW.origin,
    templates: [
      on("pms-save-rules", "Save the rules", "edit", "rules"),
      on("pms-add-people", "Add participants", "create", "participants"),
      on("pms-drop-cycle", "Drop the cycle", "delete", "publish"),
    ],
    flows: [FLOW],
  });
  renderScreen();
  await showFlows();

  const flow = await screen.findByRole("region", { name: "Performance cycle wizard" });
  const edges = [...within(flow).getByTestId("flow-map").querySelectorAll("g[data-tone]")];
  const tones = edges.map((g) => g.getAttribute("data-tone"));
  // setup->rules, rules->competencies, rules->participants, participants->publish.
  expect(tones).toEqual(["edit", "none", "create", "delete"]);
  const cls = edges.map((g) => g.getAttribute("class"));
  expect(cls[0]).toContain("text-warning");
  expect(cls[1]).toContain("text-accent");
  expect(cls[2]).toContain("text-success");
  expect(cls[3]).toContain("text-danger");
  for (const c of cls) expect(c).toContain("flow-edge");
  // Each arrow carries a bright stretch that runs its length (pathLength 1,
  // so every arrow at the same pace), a later column starting later.
  const pulses = edges.map((g) => g.querySelector("path.flow-pulse") as SVGElement);
  for (const p of pulses) expect(p).toHaveAttribute("pathLength", "1");
  expect(pulses[0].getAttribute("d")).toBe(edges[0].querySelector("path[data-edge]")!.getAttribute("d"));
  expect(pulses[0]).toHaveStyle({ animationDelay: "0s" });
  expect(pulses[3].style.animationDelay).not.toBe("0s");
});

test("a stage whose templates disagree takes the theme's colour", async () => {
  const { edgeTone } = await import("./FlowMap");
  expect(edgeTone([])).toBe("none");
  expect(edgeTone([{ effect: "create" }, { effect: "create" }])).toBe("create");
  expect(edgeTone([{ effect: "create" }, { effect: "delete" }])).toBe("none");
});

// ---------------------------------------------------------------------------
// Module groups fold, like the test case screens' groups.

test("a module group folds from its header and stays folded", async () => {
  mockOverview(OVERVIEW);
  renderScreen();

  await screen.findByText("Remove a goal");
  const fold = screen.getByRole("button", { name: "Collapse group PMS / Goals" });
  expect(fold).toHaveAttribute("aria-expanded", "true");
  fireEvent.click(fold);

  expect(screen.queryByText("Remove a goal")).not.toBeInTheDocument();
  // The group itself stays, so it can be opened again.
  expect(screen.getByRole("region", { name: "PMS / Goals" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Expand group PMS / Goals" })).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByText("Create a draft performance cycle")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-api-templates-collapsed-groups")).toContain("PMS / Goals");

  // The group's title folds it too.
  fireEvent.click(screen.getByRole("heading", { name: /PMS \/ Performance Cycle/ }));
  expect(screen.queryByText("Create a draft performance cycle")).not.toBeInTheDocument();

  // A search opens every group, so a match is never hidden in a fold.
  fireEvent.change(screen.getByRole("textbox", { name: "Search templates" }), { target: { value: "goal" } });
  expect(screen.getByText("Remove a goal")).toBeInTheDocument();
});

test("Collapse all folds every open group, then becomes Expand all", async () => {
  mockOverview(OVERVIEW);
  renderScreen();

  await screen.findByText("Remove a goal");
  fireEvent.click(screen.getByRole("button", { name: /Collapse all \(2\)/ }));
  expect(screen.queryByText("Remove a goal")).not.toBeInTheDocument();
  expect(screen.queryByText("Create a draft performance cycle")).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: /Expand all \(2\)/ }));
  expect(screen.getByText("Remove a goal")).toBeInTheDocument();
  expect(screen.getByText("Create a draft performance cycle")).toBeInTheDocument();
});

// ---------------------------------------------------------------------------
// View flow: the flow on its own page in the browser, as the review page's
// tree view opens the Test map.

test("View flow asks Rust to open the flow's own page, in the app's palette", async () => {
  const calls = mockOverview(FLOW_OVERVIEW, (cmd) => (cmd === "api_templates_open_flow" ? null : undefined));
  renderScreen(vi.fn(), "Web");
  await showFlows();

  const flow = await screen.findByRole("region", { name: "Performance cycle wizard" });
  fireEvent.click(within(flow).getByRole("button", { name: "View flow Performance cycle wizard in the browser" }));

  await waitFor(() => expect(calls.filter((c) => c.cmd === "api_templates_open_flow")).toHaveLength(1));
  const args = calls.find((c) => c.cmd === "api_templates_open_flow")?.args as Record<string, unknown>;
  expect(args).toMatchObject({ organization: "acme", project: "Web", id: "pms-performance-cycle" });
  expect(args.palette).toMatchObject({ light: expect.any(Object), dark: expect.any(Object) });
});

test("a flow page that cannot be opened says why", async () => {
  const { toast } = await import("../../lib/toast");
  mockOverview(FLOW_OVERVIEW, (cmd) => {
    if (cmd === "api_templates_open_flow") throw "the flow pms-performance-cycle is no longer saved";
  });
  renderScreen();
  await showFlows();

  const flow = await screen.findByRole("region", { name: "Performance cycle wizard" });
  fireEvent.click(within(flow).getByRole("button", { name: /^View flow/ }));
  await waitFor(() => expect(toast.error).toHaveBeenCalledWith("the flow pms-performance-cycle is no longer saved"));
});

test("the details count the parameters and show what an optional one sends when left out", async () => {
  mockOverview({
    ...OVERVIEW,
    templates: [
      {
        template: template({
          params: [
            { name: "cycleId", type: "number", required: true },
            { name: "comments", type: "list", required: false, default: [] },
          ],
        }),
        runs: [],
      },
    ],
  });
  renderScreen();

  const row = await screen.findByRole("listitem", { name: "Create a draft performance cycle" });
  fireEvent.click(within(row).getByRole("button", { name: "Show details of Create a draft performance cycle" }));
  const details = within(row).getByTestId("template-details");
  expect(within(details).getByText("Parameters (2)")).toBeInTheDocument();
  const table = within(details).getByRole("table", { name: "Parameters" });
  const header = within(table).getAllByRole("columnheader").map((h) => h.textContent);
  expect(header).toContain("Default");
  const comments = within(table).getByText("comments").closest("tr")!;
  const cells = within(comments).getAllByRole("cell").map((c) => c.textContent);
  expect(cells.slice(0, 4)).toEqual(["comments", "list", "no", "[]"]);
});
