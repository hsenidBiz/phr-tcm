import { afterEach, expect, test } from "vitest";
import type { BoardItem, BoardParent } from "../bindings";
import {
  NO_PARENT,
  cardCount,
  collapsedLanesKey,
  groupIntoLanes,
  laneIdOf,
  laneLabel,
  laneToggleName,
  loadCollapsedLanes,
  loadSwimlanes,
  saveCollapsedLanes,
  saveSwimlanes,
  type Lane,
} from "./boardLanes";

afterEach(() => localStorage.clear());

function card(id: number, parent: BoardParent | null): BoardItem {
  return {
    id,
    title: `Card ${id}`,
    work_item_type: "Task",
    state: "To Do",
    state_color: "",
    column: "To Do",
    assigned_to: "",
    tags: "",
    priority: null,
    changed_date: "",
    parent,
  };
}

/** A card shaped before `parent` existed: the field is missing, not null. */
function legacy(id: number): BoardItem {
  return Object.fromEntries(
    Object.entries(card(id, null)).filter(([k]) => k !== "parent"),
  ) as unknown as BoardItem;
}

const leave: BoardParent = { id: 500, title: "Leave requests", work_item_type: "Product Backlog Item" };
const payroll: BoardParent = { id: 600, title: "Payroll export", work_item_type: "Product Backlog Item" };

test("cards group under their parent, lanes in board order, No parent last", () => {
  const lanes = groupIntoLanes([card(1, null), card(2, leave), card(3, payroll), card(4, leave)]);
  expect(lanes.map((l) => l.id)).toEqual([500, 600, NO_PARENT]);
  expect(lanes[0].items.map((i) => i.id)).toEqual([2, 4]);
  expect(lanes[0].parent).toEqual(leave);
  expect(lanes[2].parent).toBeNull();
  expect(lanes[2].items.map((i) => i.id)).toEqual([1]);
});

test("only lanes with cards exist, so a lane the filters emptied is not there", () => {
  expect(groupIntoLanes([])).toEqual([]);
  expect(groupIntoLanes([card(3, payroll)]).map((l) => l.id)).toEqual([600]);
});

test("a card that is itself a parent sits in its own parent's lane", () => {
  const feature: BoardParent = { id: 50, title: "Absence", work_item_type: "Feature" };
  const pbi = { ...card(500, feature), title: "Leave requests", work_item_type: "Product Backlog Item" };
  const lanes = groupIntoLanes([card(2, leave), pbi]);
  expect(lanes.map((l) => [l.id, l.items.map((i) => i.id)])).toEqual([
    [500, [2]],
    [50, [500]],
  ]);
});

/// Review focus 1: the tour's board, or any data shaped before this
/// change, has no `parent` at all.
test("a card with no parent field at all goes to No parent", () => {
  expect(laneIdOf(legacy(7))).toBe(NO_PARENT);
  const lanes = groupIntoLanes([legacy(7), card(2, leave)]);
  expect(lanes.map((l) => l.id)).toEqual([500, NO_PARENT]);
  expect(lanes[1].items.map((i) => i.id)).toEqual([7]);
});

test("a lane is named by its parent's title, its id when unreadable, or No parent", () => {
  const lane = (id: number, parent: BoardParent | null, n: number): Lane => ({
    id,
    parent,
    items: Array.from({ length: n }, (_, i) => card(i + 1, parent)),
  });
  expect(laneLabel(lane(500, leave, 1))).toBe("Leave requests");
  expect(laneLabel(lane(900, { id: 900, title: "", work_item_type: "" }, 1))).toBe("#900");
  expect(laneLabel(lane(NO_PARENT, null, 1))).toBe("No parent");
  expect(laneToggleName(lane(500, leave, 4), false)).toBe("Leave requests, 4 cards, collapse");
  expect(laneToggleName(lane(NO_PARENT, null, 1), true)).toBe("No parent, 1 card, expand");
  expect(cardCount(1)).toBe("1 card");
  expect(cardCount(2)).toBe("2 cards");
});

test("collapsed lanes are remembered per organisation and project, 0 standing for No parent", () => {
  expect(collapsedLanesKey("acme", "Web")).toBe("tcm-v2-board-lanes-collapsed:acme/Web");
  saveCollapsedLanes("acme", "Web", new Set([600, NO_PARENT, 500]));
  expect(localStorage.getItem("tcm-v2-board-lanes-collapsed:acme/Web")).toBe("[0,500,600]");
  expect(loadCollapsedLanes("acme", "Web")).toEqual(new Set([0, 500, 600]));
  expect(loadCollapsedLanes("acme", "Mobile")).toEqual(new Set());
});

test("unreadable stored lanes are ignored rather than trusted", () => {
  localStorage.setItem("tcm-v2-board-lanes-collapsed:acme/Web", "{not json");
  expect(loadCollapsedLanes("acme", "Web")).toEqual(new Set());
  localStorage.setItem("tcm-v2-board-lanes-collapsed:acme/Web", JSON.stringify([500, "600", -1, 1.5]));
  expect(loadCollapsedLanes("acme", "Web")).toEqual(new Set([500]));
});

test("the Swimlanes switch starts off and is remembered on this machine", () => {
  expect(loadSwimlanes()).toBe(false);
  saveSwimlanes(true);
  expect(localStorage.getItem("tcm-v2-board-swimlanes")).toBe("on");
  expect(loadSwimlanes()).toBe(true);
  saveSwimlanes(false);
  expect(loadSwimlanes()).toBe(false);
});
