import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { useRowKeys, type RowSelectKeys } from "./useRowKeys";

const IDS = [11, 12, 13];

function List({ onSelect }: { onSelect: (id: number, keys: RowSelectKeys) => void }) {
  const { listProps, tabKey, onRowFocus } = useRowKeys({ order: IDS, onSelect });
  return (
    <ul {...listProps}>
      {IDS.map((id) => (
        <li
          key={id}
          data-row-key={id}
          tabIndex={id === tabKey ? 0 : -1}
          onFocus={(e) => e.target === e.currentTarget && onRowFocus(id)}
        >
          row {id}
          <input aria-label={`note ${id}`} />
        </li>
      ))}
    </ul>
  );
}

const row = (id: number) => screen.getByText(`row ${id}`);

test("Down and Up move focus to the next and previous row", () => {
  render(<List onSelect={vi.fn()} />);
  row(11).focus();
  fireEvent.keyDown(row(11), { key: "ArrowDown" });
  expect(document.activeElement).toBe(row(12));
  fireEvent.keyDown(row(12), { key: "ArrowUp" });
  expect(document.activeElement).toBe(row(11));
});

test("Home and End go to the first and last row", () => {
  render(<List onSelect={vi.fn()} />);
  row(12).focus();
  fireEvent.keyDown(row(12), { key: "End" });
  expect(document.activeElement).toBe(row(13));
  fireEvent.keyDown(row(13), { key: "Home" });
  expect(document.activeElement).toBe(row(11));
});

test("Shift+Down selects as a Shift-click would", () => {
  const onSelect = vi.fn();
  render(<List onSelect={onSelect} />);
  row(11).focus();
  fireEvent.keyDown(row(11), { key: "ArrowDown", shiftKey: true });
  expect(onSelect).toHaveBeenCalledWith(12, { shiftKey: true, ctrlKey: false, metaKey: false });
});

test("Space selects the focused row as a Ctrl-click would", () => {
  const onSelect = vi.fn();
  render(<List onSelect={onSelect} />);
  row(12).focus();
  fireEvent.keyDown(row(12), { key: " " });
  expect(onSelect).toHaveBeenCalledWith(12, { shiftKey: false, ctrlKey: true, metaKey: false });
});

test("only one row is in the Tab order, and it follows the focus", () => {
  render(<List onSelect={vi.fn()} />);
  const tabbable = () => IDS.filter((id) => row(id).tabIndex === 0);
  expect(tabbable()).toEqual([11]);
  row(11).focus();
  fireEvent.keyDown(row(11), { key: "ArrowDown" });
  expect(tabbable()).toEqual([12]);
});

test("keys typed in a field inside a row stay with the field", () => {
  const onSelect = vi.fn();
  render(<List onSelect={onSelect} />);
  const note = screen.getByLabelText("note 11");
  note.focus();
  fireEvent.keyDown(note, { key: "ArrowDown" });
  fireEvent.keyDown(note, { key: " " });
  expect(document.activeElement).toBe(note);
  expect(onSelect).not.toHaveBeenCalled();
});
