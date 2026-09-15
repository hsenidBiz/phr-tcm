import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { SuiteCase } from "../../lib/suiteOrder";
import FileOrderDialog from "./FileOrderDialog";

const suite: SuiteCase[] = [1, 2, 3, 4, 5].map((id) => ({ id, title: `Case ${id}` }));
const file = (name: string, ...ids: number[]) => ({
  path: `C:\\drafts\\${name}`,
  name,
  cases: ids.map((update_id) => ({ update_id })),
});

test("lists the files with what each places, flags an empty one, and counts cases claimed twice", () => {
  render(
    <FileOrderDialog
      suiteCases={suite}
      files={[file("a.json", 1, 2), file("b.json", 2, 3), file("c.json", 99)]}
      onAddFiles={() => {}}
      onClose={() => {}}
      onApply={() => {}}
    />,
  );
  const rows = screen.getAllByRole("listitem");
  expect(rows[0]).toHaveTextContent("a.json");
  expect(rows[0]).toHaveTextContent("places 2 of 5");
  expect(rows[1]).toHaveTextContent("places 1 of 5");
  expect(rows[2]).toHaveTextContent("places nothing from this suite");
  expect(screen.getByText(/1 test case is named in more than one file/)).toBeInTheDocument();
});

test("the files can be rearranged, and Apply hands back the resulting order", () => {
  const onApply = vi.fn();
  render(
    <FileOrderDialog
      suiteCases={suite}
      files={[file("a.json", 1, 2), file("b.json", 5, 4)]}
      onAddFiles={() => {}}
      onClose={() => {}}
      onApply={onApply}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Move b.json up" }));
  expect(screen.getAllByRole("listitem")[0]).toHaveTextContent("b.json");
  fireEvent.click(screen.getByRole("button", { name: "Apply" }));
  const [order, placed] = onApply.mock.calls[0];
  expect((order as SuiteCase[]).map((c) => c.id)).toEqual([5, 4, 1, 2, 3]);
  expect(placed).toBe(4);
});

test("a file row can be dragged onto another to reorder the files", () => {
  const onApply = vi.fn();
  render(
    <FileOrderDialog
      suiteCases={suite}
      files={[file("a.json", 1, 2), file("b.json", 3), file("c.json", 4, 5)]}
      onAddFiles={() => {}}
      onClose={() => {}}
      onApply={onApply}
    />,
  );
  const rows = screen.getAllByRole("listitem");
  fireEvent.dragStart(rows[2]);
  fireEvent.dragOver(rows[0]);
  fireEvent.drop(rows[0]);
  const reordered = screen.getAllByRole("listitem");
  expect(reordered[0]).toHaveTextContent("c.json");
  expect(reordered[1]).toHaveTextContent("a.json");
  expect(reordered[2]).toHaveTextContent("b.json");

  fireEvent.click(screen.getByRole("button", { name: "Apply" }));
  const [order] = onApply.mock.calls[0];
  expect((order as SuiteCase[]).map((c) => c.id)).toEqual([4, 5, 1, 2, 3]);
});

test("Add more files asks the parent; Apply is disabled while nothing is placed", () => {
  const onAddFiles = vi.fn();
  render(
    <FileOrderDialog suiteCases={suite} files={[file("x.json", 99)]} onAddFiles={onAddFiles} onClose={() => {}} onApply={() => {}} />,
  );
  expect(screen.getByRole("button", { name: "Apply" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Add more files" }));
  expect(onAddFiles).toHaveBeenCalledTimes(1);
});
