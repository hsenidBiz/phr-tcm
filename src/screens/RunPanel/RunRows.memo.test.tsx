import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test, vi } from "vitest";
import type { TestPoint } from "../../bindings";

// Each row draws one HistoryDots, so its calls count row renders.
const drawn: number[] = [];
vi.mock("../../components/HistoryDots", () => ({
  default: () => {
    drawn.push(1);
    return null;
  },
}));
vi.mock("./CasePreview", () => ({ default: () => null }));

const { RunRows } = await import("./RunRows");

const rows = [1, 2, 3].map(
  (i) => ({ point_id: i, test_case_id: 100 + i, test_case_name: `Case ${i}`, last_outcome: "passed" }) as TestPoint,
);
const EMPTY = new Set<number>();
const NO_HISTORY = new Map();
const TINT = {};

function Harness() {
  const [selected, setSelected] = useState(new Set<number>());
  return (
    <table>
      <RunRows
        rows={rows}
        selected={selected}
        expanded={EMPTY}
        historyByCase={NO_HISTORY}
        org="o"
        project="p"
        rowTint={TINT}
        outcomeColor={TINT}
        onRowClick={onClick(setSelected)}
        onToggleExpanded={noop}
      />
    </table>
  );
}
const noop = () => {};
let click: ((p: TestPoint) => void) | null = null;
function onClick(set: (s: Set<number>) => void) {
  click ??= (p: TestPoint) => set(new Set([p.test_case_id!]));
  return click;
}

test("a selection click draws only the row it changed", () => {
  render(<Harness />);
  expect(drawn.length).toBe(3);
  fireEvent.click(screen.getByText("Case 2"));
  expect(drawn.length).toBe(4);
});
