// DateField: the app's date input - a button showing the date that opens
// XiodUI's calendar in an inline panel. Value is "" or "YYYY-MM-DD".

import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test, vi } from "vitest";
import DateField from "./datefield";

function Host({ initial = "", onChange = () => {} }: { initial?: string; onChange?: (v: string) => void }) {
  const [v, setV] = useState(initial);
  return (
    <DateField
      ariaLabel="Target date"
      value={v}
      onChange={(next) => {
        setV(next);
        onChange(next);
      }}
    />
  );
}

const iso = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;

test("empty, it says so, and the field is named for what it holds", () => {
  render(<Host />);
  expect(screen.getByRole("button", { name: "Target date" })).toHaveTextContent("Pick a date");
});

test("it opens on the value's month, weeks starting Monday; picking a day sets it, closes, and hands focus back", () => {
  const onChange = vi.fn();
  render(<Host initial="2026-09-10" onChange={onChange} />);
  const field = screen.getByRole("button", { name: "Target date" });
  fireEvent.click(field);
  expect(screen.getByText("September 2026")).toBeInTheDocument();
  expect(screen.getAllByText(/^(Mo|Tu|We|Th|Fr|Sa|Su)$/)[0]).toHaveTextContent("Mo");

  fireEvent.click(screen.getByRole("button", { name: "Tuesday, September 15, 2026" }));
  expect(onChange).toHaveBeenLastCalledWith("2026-09-15");
  expect(screen.queryByText("September 2026")).not.toBeInTheDocument();
  // The calendar held focus and is gone: focus comes back to the field.
  expect(field).toHaveFocus();
});

test("Clear empties it", () => {
  const onChange = vi.fn();
  render(<Host initial="2026-09-10" onChange={onChange} />);
  fireEvent.click(screen.getByRole("button", { name: "Target date" }));
  fireEvent.click(screen.getByRole("button", { name: "Clear" }));
  expect(onChange).toHaveBeenLastCalledWith("");
  expect(screen.getByRole("button", { name: "Target date" })).toHaveTextContent("Pick a date");
});

test("the calendar's own Today sets today", () => {
  const onChange = vi.fn();
  render(<Host onChange={onChange} />);
  fireEvent.click(screen.getByRole("button", { name: "Target date" }));
  fireEvent.click(screen.getByRole("button", { name: "Today" }));
  expect(onChange).toHaveBeenLastCalledWith(iso(new Date()));
});

// The field lives in the work-item drawer, whose window-level Escape closes
// the drawer - and its unsaved draft. With the calendar open (and holding
// focus), Escape must close the calendar only.
test("Escape closes the calendar and nothing behind it", () => {
  const behind = vi.fn();
  window.addEventListener("keydown", behind);
  try {
    render(<Host initial="2026-09-10" />);
    const field = screen.getByRole("button", { name: "Target date" });
    fireEvent.click(field);
    fireEvent.keyDown(screen.getByRole("button", { name: "Thursday, September 10, 2026" }), { key: "Escape" });
    expect(screen.queryByText("September 2026")).not.toBeInTheDocument();
    expect(field).toHaveFocus();
    expect(behind).not.toHaveBeenCalled();
  } finally {
    window.removeEventListener("keydown", behind);
  }
});

test("a mousedown outside closes it", () => {
  render(
    <>
      <Host initial="2026-09-10" />
      <p>elsewhere</p>
    </>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Target date" }));
  fireEvent.mouseDown(screen.getByText("elsewhere"));
  expect(screen.queryByText("September 2026")).not.toBeInTheDocument();
});
