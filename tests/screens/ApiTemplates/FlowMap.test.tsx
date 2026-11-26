// The flow map on the API Templates tab, rendered on its own: the tone sits
// on the stages, the arrows are muted, a legend names only the tones shown,
// a long stage title wraps instead of being cut, and a failure's reason is
// on the map and not only in a tooltip.

import { render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Flow, SavedTemplate } from "../../../src/bindings";
import { GEOMETRY, layoutFlow } from "../../../src/lib/flowLayout";
import FlowMap from "../../../src/screens/ApiTemplates/FlowMap";
import { failureLine } from "../../../src/screens/ApiTemplates/HereBadge";

const FLOW = {
  id: "cycle",
  title: "Cycle wizard",
  module: "PMS",
  subject: { name: "cycleId", type: "number" },
  sources: [],
  stages: [
    { id: "setup", title: "Cycle setup", creates: true, check: "SELECT 1" },
    {
      id: "rules",
      title: "An evaluation rules stage with a very long title that cannot fit on one line of the box",
      requires: ["setup"],
      check: "SELECT 1",
    },
    { id: "extras", title: "Extras", requires: ["rules"], optional: true, check: "SELECT 1" },
  ],
} as unknown as Flow;

function saved(id: string, title: string, effect: string, stage: string, here?: unknown): SavedTemplate {
  return {
    template: { id, title, effect, stage: { flow: "cycle", id: stage } },
    runs: [],
    here: here ?? { state: "confirmed", last_failure: null },
  } as unknown as SavedTemplate;
}

function renderMap(templates: SavedTemplate[], flow: Flow = FLOW) {
  return render(
    <FlowMap
      flow={flow}
      here={{ state: "confirmed", last_failure: null }}
      templates={templates}
      onOpenTemplate={vi.fn()}
      onView={vi.fn()}
      onRemove={vi.fn()}
    />,
  );
}

const stage = (name: RegExp | string) => within(screen.getByTestId("flow-map")).getByRole("group", { name });

test("the tone is a 3px left edge on the stage, and an open stage takes the accent", () => {
  renderMap([saved("a", "Create a cycle", "create", "setup"), saved("b", "Save rules", "edit", "rules")]);
  expect(stage("Cycle setup").className).toContain("border-l-success");
  expect(stage("Cycle setup").className).toContain("border-l-[3px]");
  expect(stage(/An evaluation rules/).className).toContain("border-l-warning");
  expect(stage("Extras").className).toContain("border-l-accent");
  // The optional stage is dashed, its tone edge stays solid.
  expect(stage("Extras").className).toContain("border-dashed");
  expect(stage("Extras").className).toContain("[border-left-style:solid]");
});

test("the arrows are muted and keep their arrowheads", () => {
  renderMap([saved("a", "Create a cycle", "create", "setup")]);
  const edges = [...screen.getByTestId("flow-map").querySelectorAll("g.flow-edge")];
  expect(edges).toHaveLength(2);
  for (const g of edges) {
    expect(g.getAttribute("class")).toContain("text-muted");
    // The curve and its arrowhead (and the pulse).
    expect(g.querySelectorAll("path").length).toBeGreaterThanOrEqual(2);
    expect(g.querySelector("path:not([data-edge]):not(.flow-pulse)")!.getAttribute("d")).toMatch(/^M.* L.* L/);
  }
});

test("the legend names only the tones the map shows", () => {
  renderMap([saved("a", "Create a cycle", "create", "setup"), saved("b", "Save rules", "edit", "rules")]);
  const legend = screen.getByRole("list", { name: "Legend" });
  const words = within(legend)
    .getAllByRole("listitem")
    .map((li) => li.textContent);
  // Create, Edit, and the open "Extras" stage (and it is optional); no Delete or Read.
  expect(words).toEqual(["Create", "Edit", "Open: no template yet, or mixed", "Optional stage"]);
});

test("a flow with only read stages names only Read", () => {
  const flow = { ...FLOW, stages: [FLOW.stages[0]] } as Flow;
  renderMap([saved("a", "Look it up", "read", "setup")], flow);
  const words = within(screen.getByRole("list", { name: "Legend" }))
    .getAllByRole("listitem")
    .map((li) => li.textContent);
  expect(words).toEqual(["Read"]);
});

test("a long stage title wraps over two lines and keeps its tooltip, never a truncate", () => {
  renderMap([]);
  const long = FLOW.stages[1].title;
  const title = within(stage(/An evaluation rules/)).getByText(long);
  expect(title.className).toContain("line-clamp-2");
  expect(title.className).not.toContain("truncate");
  expect(title).toHaveAttribute("title", long);
});

test("a failed template shows its reason under it, and the box makes room for it", () => {
  const failed = saved("b", "Save rules", "edit", "rules", {
    state: "failed",
    failure: { at: "2026-11-06 10:00:00", step: "3", detail: "HTTP 500 from the rules endpoint" },
  });
  renderMap([failed]);
  const line = screen.getByText("failed at step 3: HTTP 500 from the rules endpoint");
  // The tooltip keeps the whole sentence.
  expect(line).toHaveAttribute("title", expect.stringContaining("Failed on this environment at 3: HTTP 500"));
  expect(stage(/An evaluation rules/)).toContainElement(line);
  // The screen-reader list carries it too.
  expect(
    screen.getByText(/Templates: Save rules \(failed here, failed at step 3: HTTP 500 from the rules endpoint\)\./, {
      selector: "li",
    }),
  ).toBeInTheDocument();

  const plain = layoutFlow(FLOW, { rules: 1 });
  const withFailure = layoutFlow(FLOW, { rules: 1 }, { rules: 1 });
  const h = (l: ReturnType<typeof layoutFlow>) => l.boxes.find((b) => b.id === "rules")!.h;
  expect(h(withFailure) - h(plain)).toBe(GEOMETRY.perFailure);
});

test("an unconfirmed or confirmed template has no failure line", () => {
  renderMap([saved("b", "Save rules", "edit", "rules", { state: "unconfirmed" })]);
  expect(screen.queryByText(/^failed/)).not.toBeInTheDocument();
});

test("failureLine words the step and the reason, cutting a long reason", () => {
  expect(failureLine({ at: "t", step: "3", detail: "boom" })).toBe("failed at step 3: boom");
  expect(failureLine({ at: "t", step: "save rules", detail: null })).toBe("failed at save rules");
  expect(failureLine({ at: "t", step: null, detail: "  two\n words " })).toBe("failed: two words");
  expect(failureLine({ at: "t", step: null, detail: null })).toBeNull();
  const long = failureLine({ at: "t", step: null, detail: "x".repeat(300) })!;
  expect(long.endsWith("…")).toBe(true);
  expect([...long].length).toBeLessThan(135);
});

test("the failure line says failed at once and never prints the application's response", () => {
  // What the assistant's run records: message() as the detail, the runner's own reason beside it.
  const failure = {
    at: "t",
    step: "Evaluation rules",
    detail:
      "cycleId 274 created; failed at Evaluation rules (SaveEvalRulesProgress): expected status 200, got 400 - the response began: <html>Server secret page</html>",
    reason: "expected status 200, got 400",
  };
  renderMap([saved("b", "Save rules", "edit", "rules", { state: "failed", failure })]);
  const line = screen.getByText("failed at Evaluation rules: expected status 200, got 400");
  expect(line.textContent!.match(/failed at/g)).toHaveLength(1);
  expect(line.textContent).not.toMatch(/SaveEvalRulesProgress|cycleId|response began|secret/);
  // The title keeps the full sentence; the reader's list carries the short line only.
  expect(line).toHaveAttribute("title", expect.stringContaining("cycleId 274 created; failed at Evaluation rules"));
  const list = screen.getByText(/Templates: Save rules \(failed here/, { selector: "li" });
  expect(list.textContent).toContain("failed here, failed at Evaluation rules: expected status 200, got 400).");
  expect(list.textContent).not.toContain("secret");
});

test("an older record with only the full message is cut down to the reason", () => {
  expect(
    failureLine({
      at: "t",
      step: "Evaluation rules",
      detail: "cycleId 274 created; failed at Evaluation rules (h): expected status 200, got 400 - the response began: <b>x</b>",
      reason: null,
    }),
  ).toBe("failed at Evaluation rules: expected status 200, got 400");
});
