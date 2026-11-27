import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ResetNeededPanel from "../../../src/screens/AutoRun/ResetNeededPanel";
import { groupByArea, parseSetItem, parseSetName } from "../../../src/screens/AutoRun/plan";

// The owner's real items, as [name, value] pairs.
const ITEMS: [string, string][] = [
  ["000015 stage intermediate_competency of cycle 11892", "goal plan acknowledged, no competency rated, not submitted"],
  ["000027 stage 5563 assessment", "submitted by the employee, not yet rated by the manager"],
  ["000027 stage 5563", "submitted, Goals flagged, rejected"],
  ["000015 competency stage, cycle 11892", "submitted by the employee, not rated by the manager"],
];

test("the parser reads the owner's four items into chips and text", () => {
  expect(parseSetItem(...ITEMS[0])).toEqual({
    chips: ["Employee 000015", "Cycle 11892", "Stage intermediate_competency"],
    text: "Goal plan acknowledged, no competency rated, not submitted",
  });
  expect(parseSetItem(...ITEMS[1])).toEqual({
    chips: ["Employee 000027", "Stage 5563"],
    text: "Assessment: submitted by the employee, not yet rated by the manager",
  });
  expect(parseSetItem(...ITEMS[2])).toEqual({
    chips: ["Employee 000027", "Stage 5563"],
    text: "Submitted, Goals flagged, rejected",
  });
  expect(parseSetItem(...ITEMS[3])).toEqual({
    chips: ["Employee 000015", "Stage competency", "Cycle 11892"],
    text: "Submitted by the employee, not rated by the manager",
  });
});

test("a later batch's pair gives the same chips", () => {
  const [name, value] = ITEMS[0];
  expect(parseSetItem(name, value)?.chips).toEqual(["Employee 000015", "Cycle 11892", "Stage intermediate_competency"]);
  render(
    <ResetNeededPanel
      reset={{ batch: 2, set: [ITEMS[1]], names: [], changed_by: [] }}
      remaining={[]}
      titleOf={() => undefined}
      onContinue={() => {}}
      onStop={() => {}}
    />,
  );
  expect(screen.getByText("Employee 000027")).toBeInTheDocument();
  expect(screen.getByText("Stage 5563")).toBeInTheDocument();
});

test("is, to and of are never a stage; a number after cycle is not an employee", () => {
  expect(parseSetName("stage is ON")).toBeNull();
  expect(parseSetName("cycle 118920")).toEqual({ chips: ["Cycle 118920"], rest: "" });
});

test("a name the parser does not recognise is kept whole", () => {
  expect(parseSetItem("Approval workflow", "ON")).toBeNull();
  render(
    <ResetNeededPanel
      reset={{ batch: 1, set: [["Approval workflow", "ON"]], names: [], changed_by: [] }}
      remaining={[]}
      titleOf={() => undefined}
      onContinue={() => {}}
      onStop={() => {}}
    />,
  );
  expect(screen.getByText("Make sure Approval workflow is ON")).toBeInTheDocument();
});

test("an area literally named Other joins the area-less cases", () => {
  const t: Record<number, string> = { 1: "Other - One", 2: "Plain", 3: "A - Three" };
  const groups = groupByArea([1, 2, 3], (id) => t[id]);
  expect(groups.map((g) => g.area)).toEqual(["A", "Other"]);
  expect(groups[1].items).toEqual([
    { id: 1, title: "One" },
    { id: 2, title: "Plain" },
  ]);
});

const TITLES: Record<number, string> = {
  1: "My Assessment Landing Page - Opens",
  2: "My Assessment Landing Page - Shows the cycle",
  3: "Reports - Exports a file",
  4: "No area here",
  5: "My Assessment Landing Page - Closes",
  6: "My Assessment Landing Page - Lists staff",
  7: "Reports - Prints",
};
const titleOf = (id: number) => TITLES[id];

test("cases group by area in first-seen order with Other last, prefix stripped", () => {
  const groups = groupByArea([4, 1, 3, 2, 7, 5], titleOf);
  expect(groups.map((g) => [g.area, g.items.length])).toEqual([
    ["My Assessment Landing Page", 3],
    ["Reports", 2],
    ["Other", 1],
  ]);
  expect(groups[0].items).toEqual([
    { id: 1, title: "Opens" },
    { id: 2, title: "Shows the cycle" },
    { id: 5, title: "Closes" },
  ]);
  expect(groups[2].items).toEqual([{ id: 4, title: "No area here" }]);
});

function renderPanel(over: { set?: [string, string][]; remaining?: number[]; onContinue?: () => void } = {}) {
  return render(
    <ResetNeededPanel
      reset={{ batch: 1, set: over.set ?? [], names: [], changed_by: [] }}
      remaining={over.remaining ?? [1, 2, 3, 4, 5, 6, 7]}
      titleOf={titleOf}
      onContinue={over.onContinue ?? (() => {})}
      onStop={() => {}}
    />,
  );
}

test("ticking an item strikes it and the counter follows; Continue needs no ticks", () => {
  const onContinue = vi.fn();
  renderPanel({ set: [["000015 stage final of cycle 11892", "ON"], ["Other flag", "OFF"]], onContinue });
  expect(screen.getByText("Set up · 0 of 2 done")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Continue the run" }));
  expect(onContinue).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("checkbox", { name: "Make sure 000015 stage final of cycle 11892 is ON" }));
  expect(screen.getByText("Set up · 1 of 2 done")).toBeInTheDocument();
  expect(screen.getByText("ON")).toHaveClass("line-through");
});

test("the first area is open with three cases and 'and N more'; the rest are folded", () => {
  renderPanel();
  expect(screen.getByText("Still to run · 7 cases in 3 areas")).toBeInTheDocument();
  const landing = screen.getByRole("button", { name: /My Assessment Landing Page/ });
  expect(landing).toHaveAttribute("aria-expanded", "true");
  const list = screen.getByRole("list", { name: "Cases in My Assessment Landing Page" });
  expect(within(list).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    "#1 Opens",
    "#2 Shows the cycle",
    "#5 Closes",
  ]);
  expect(screen.getByRole("button", { name: /Reports/ })).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("list", { name: "Cases in Reports" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "and 1 more" }));
  const all = screen.getByRole("list", { name: "Cases in My Assessment Landing Page" });
  expect(within(all).getAllByRole("listitem")).toHaveLength(4);
  fireEvent.click(screen.getByRole("button", { name: /Reports/ }));
  expect(screen.getByRole("list", { name: "Cases in Reports" })).toBeInTheDocument();
});

test("past six areas the rest hide behind 'N more areas'", () => {
  render(
    <ResetNeededPanel
      reset={{ batch: 2, set: [], names: [], changed_by: [] }}
      remaining={[1, 2, 3, 4, 5, 6, 7, 8]}
      titleOf={(id) => `Area ${id} - Case`}
      onContinue={() => {}}
      onStop={() => {}}
    />,
  );
  expect(screen.queryByRole("button", { name: /Area 7/ })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "2 more areas" }));
  expect(screen.getByRole("button", { name: /Area 7/ })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Area 8/ })).toBeInTheDocument();
});
