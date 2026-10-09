import { expect, test } from "vitest";
import { buildAreaGroups, groupIndices, visibleOrder, type AreaGroup } from "./areaGroups";

const cases = (...areas: string[]) => areas.map((area) => ({ area }));

/** name(count)[indices]{children} - the whole tree in one comparable line. */
function shape(groups: AreaGroup[]): string {
  return groups
    .map(
      (g) =>
        `${g.name}(${g.count})[${g.indices.join(",")}]` +
        (g.children.length ? `{${shape(g.children)}}` : ""),
    )
    .join(" ");
}

test("nests one level per segment, three levels deep", () => {
  const tree = buildAreaGroups(cases("Events / Create / Form", "Events / Create", "Events"));
  expect(shape(tree)).toBe("Events(3)[2]{Create(2)[1]{Form(1)[0]}}");
  expect(tree[0].children[0].children[0].path).toBe("Events / Create / Form");
  expect(tree[0].children[0].children[0].key).toBe("events / create / form");
});

test("groups appear in the order of their first case, not A-Z", () => {
  const tree = buildAreaGroups(cases("Zeta", "Alpha", "Zeta / B", "Zeta / A", "Alpha"));
  expect(shape(tree)).toBe("Zeta(3)[0]{B(1)[2] A(1)[3]} Alpha(2)[1,4]");
});

test("cases with no area go under Ungrouped, last", () => {
  const tree = buildAreaGroups(cases("", "Reports", "  /  ", "Billing"));
  expect(shape(tree)).toBe("Reports(1)[1] Billing(1)[3] Ungrouped(2)[0,2]");
  expect(tree[2].key).toBe("");
});

test("segments match ignoring case and surrounding spaces; the first spelling is shown", () => {
  const tree = buildAreaGroups(cases("Display", "display ", " DISPLAY/ Rules", "display / rules "));
  expect(shape(tree)).toBe("Display(4)[0,1]{Rules(2)[2,3]}");
  expect(tree[0].children[0].path).toBe("Display / Rules");
});

test("counts include nested cases", () => {
  const tree = buildAreaGroups(cases("A / B / C", "A / B / C", "A / B", "A / D", "A"));
  expect(tree[0].count).toBe(5);
  expect(tree[0].children.map((c) => c.count)).toEqual([3, 1]);
});

test("groupIndices lists a group's own cases then its subgroups'", () => {
  const tree = buildAreaGroups(cases("A / B", "A", "A / C", "A / B"));
  expect(groupIndices(tree[0])).toEqual([1, 0, 3, 2]);
});

test("visibleOrder follows the groups and leaves out folded ones", () => {
  const tree = buildAreaGroups(cases("A / B", "", "A", "C", "A / B"));
  expect(visibleOrder(tree, new Set())).toEqual([2, 0, 4, 3, 1]);
  expect(visibleOrder(tree, new Set(["a / b"]))).toEqual([2, 3, 1]);
  expect(visibleOrder(tree, new Set(["a", ""]))).toEqual([3]);
});
