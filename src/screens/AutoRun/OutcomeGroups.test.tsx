// A step's action lines in a run: the lines a component ran sit under one
// line naming it, indented, and the step's own lines stay as they were.

import { render, screen, within } from "@testing-library/react";
import { expect, test } from "vitest";
import type { ActionOutcome } from "../../bindings";
import OutcomeGroups, { groupOutcomes } from "./OutcomeGroups";

const o = (detail: string, component?: string | null): ActionOutcome => ({ ok: true, detail, component });

const OUTCOMES = [
  o("navigated to /leave"),
  o("clicked the date field", "pick-date"),
  o("clicked 12", "pick-date"),
  o("filled the reason"),
  o("clicked the toast's close", "close-toast"),
  o("clicked the date field", "pick-date"),
];

test("consecutive outcomes of one component form one group, in order", () => {
  expect(
    groupOutcomes(OUTCOMES).map((g) => [g.component, g.items.map(({ index }) => index)]),
  ).toEqual([
    [null, [0]],
    ["pick-date", [1, 2]],
    [null, [3]],
    ["close-toast", [4]],
    ["pick-date", [5]],
  ]);
  // Older run files have no component at all.
  expect(groupOutcomes([{ ok: true, detail: "x" }]).map((g) => g.component)).toEqual([null]);
});

test("a_component_step_shows_its_name_with_its_lines_under_it", () => {
  render(
    <div data-testid="step">
      <OutcomeGroups outcomes={OUTCOMES} line={(out, i) => <p key={i}>{`${i + 1}. ${out.detail}`}</p>} />
    </div>,
  );
  const groups = screen.getAllByRole("group");
  expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual([
    "Component pick-date",
    "Component close-toast",
    "Component pick-date",
  ]);
  const first = groups[0];
  expect(within(first).getByText("pick-date")).toBeInTheDocument();
  // The lines keep their place in the step: numbered as the step ran them.
  expect(within(first).getByText("2. clicked the date field")).toBeInTheDocument();
  expect(within(first).getByText("3. clicked 12")).toBeInTheDocument();
  // Indented under the name.
  expect(within(first).getByText("3. clicked 12").parentElement?.className).toContain("pl-");
  // The step's own lines are not in any group.
  const own = screen.getByText("1. navigated to /leave");
  expect(own.closest("[role='group']")).toBeNull();
  expect(screen.getByText("4. filled the reason").closest("[role='group']")).toBeNull();
  expect(screen.getByTestId("step").textContent).toBe(
    "1. navigated to /leave" +
      "pick-date2. clicked the date field3. clicked 12" +
      "4. filled the reason" +
      "close-toast5. clicked the toast's close" +
      "pick-date6. clicked the date field",
  );
});

test("a step with no component renders its lines as they are", () => {
  render(
    <div data-testid="step">
      <OutcomeGroups outcomes={[o("a"), o("b")]} line={(out, i) => <p key={i}>{out.detail}</p>} />
    </div>,
  );
  expect(screen.queryByRole("group")).not.toBeInTheDocument();
  expect(screen.getByTestId("step").textContent).toBe("ab");
});
