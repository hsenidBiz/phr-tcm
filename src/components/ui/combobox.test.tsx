// Combobox: free entry, and the keyboard hand-offs a form built on it needs.

import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import Combobox from "./combobox";

function mount(onChange = vi.fn(), onKeyDown = vi.fn()) {
  render(
    <div onKeyDown={(e) => onKeyDown(e.key)}>
      <Combobox ariaLabel="Folder" value="" onChange={onChange} options={["a/one", "a/two"]} allowCustom />
    </div>,
  );
  return { onChange, onKeyDown };
}

test("a value that is not in the list can be entered", () => {
  const { onChange } = mount();
  fireEvent.click(screen.getByRole("combobox", { name: "Folder" }));
  const box = screen.getByPlaceholderText("Search…");
  fireEvent.change(box, { target: { value: "  b/new  " } });
  fireEvent.keyDown(box, { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("b/new");
});

test("Escape in the open list closes only the list and keeps the keyboard on the field", () => {
  const { onKeyDown } = mount();
  const trigger = screen.getByRole("combobox", { name: "Folder" });
  fireEvent.click(trigger);
  fireEvent.keyDown(screen.getByPlaceholderText("Search…"), { key: "Escape" });
  expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
  // Whatever holds the combobox (a form, a dialog) is not told: it was the list that answered.
  expect(onKeyDown).not.toHaveBeenCalled();
});

test("Escape on the closed field is left for whatever holds it", () => {
  const { onKeyDown } = mount();
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Folder" }), { key: "Escape" });
  expect(onKeyDown).toHaveBeenCalledWith("Escape");
});

test("choosing with Enter hands the keyboard back to the field", () => {
  const { onChange } = mount();
  const trigger = screen.getByRole("combobox", { name: "Folder" });
  fireEvent.click(trigger);
  fireEvent.keyDown(screen.getByPlaceholderText("Search…"), { key: "Enter" });
  expect(onChange).toHaveBeenCalledWith("a/one");
  expect(trigger).toHaveFocus();
});

test("choosing with the mouse does not move focus", () => {
  mount();
  const trigger = screen.getByRole("combobox", { name: "Folder" });
  fireEvent.click(trigger);
  fireEvent.click(screen.getByRole("option", { name: "a/two" }));
  expect(trigger).not.toHaveFocus();
});
