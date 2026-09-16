import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import MoreActionsMenu from "./MoreActionsMenu";

afterEach(() => {
  vi.useRealTimers();
});

function setup() {
  const picked: string[] = [];
  const rowClicks = { n: 0 };
  render(
    // The menu sits inside a clickable row, as it does in Search Suites.
    <div role="group" aria-label="row" onClick={() => rowClicks.n++}>
      <MoreActionsMenu
        label="More actions for Regression"
        actions={[
          { label: "Manage", onSelect: () => picked.push("Manage") },
          { label: "Run Tests", onSelect: () => picked.push("Run Tests") },
          { label: "Report", onSelect: () => picked.push("Report") },
        ]}
      />
    </div>,
  );
  const trigger = screen.getByRole("button", { name: "More actions for Regression" });
  return { picked, rowClicks, trigger };
}

test("hovering opens the options; leaving closes them after a short grace", () => {
  vi.useFakeTimers();
  const { trigger } = setup();
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();

  fireEvent.mouseEnter(trigger);
  expect(screen.getByRole("menu")).toBeInTheDocument();
  expect(trigger).toHaveAttribute("aria-expanded", "true");

  // Crossing from the chip into the list keeps it open.
  fireEvent.mouseLeave(trigger);
  fireEvent.mouseEnter(screen.getByRole("menu"));
  act(() => {
    vi.advanceTimersByTime(500);
  });
  expect(screen.getByRole("menu")).toBeInTheDocument();

  fireEvent.mouseLeave(screen.getByRole("menu"));
  act(() => {
    vi.advanceTimersByTime(500);
  });
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

test("a click pins the options open until something else is clicked", () => {
  vi.useFakeTimers();
  const { trigger, rowClicks } = setup();
  fireEvent.mouseEnter(trigger);
  fireEvent.click(trigger);
  fireEvent.mouseLeave(trigger);
  act(() => {
    vi.advanceTimersByTime(500);
  });
  expect(screen.getByRole("menu")).toBeInTheDocument();
  expect(rowClicks.n).toBe(0); // opening it does not click the row

  fireEvent.mouseDown(document.body);
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

test("picking an option runs it, closes the list, and does not click the row", () => {
  const { trigger, picked, rowClicks } = setup();
  fireEvent.click(trigger);
  expect(screen.getAllByRole("menuitem").map((m) => m.textContent)).toEqual(["Manage", "Run Tests", "Report"]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Report" }));
  expect(picked).toEqual(["Report"]);
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  expect(rowClicks.n).toBe(0);
});

test("Enter opens the list from the keyboard; Escape closes it", () => {
  const { trigger } = setup();
  fireEvent.keyDown(trigger, { key: "Enter" });
  expect(screen.getByRole("menu")).toBeInTheDocument();
  fireEvent.keyDown(document, { key: "Escape" });
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

/// transitions.dev's menu dropdown: the list grows from the chip's corner
/// and, on close, shrinks away for a moment before it leaves the page.
test("closing plays a short shrink: hidden and unclickable at once, gone after it", () => {
  vi.useFakeTimers();
  const { trigger } = setup();
  fireEvent.click(trigger);
  const list = screen.getByRole("menu");
  expect(list).toHaveClass("t-dropdown");
  expect(list).toHaveAttribute("data-origin", "top-right");

  fireEvent.mouseDown(document.body);
  // Out of reach straight away...
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
  expect(trigger).toHaveAttribute("aria-expanded", "false");
  // ...but still painted, shrinking.
  expect(document.querySelector("[role=menu].is-closing")).not.toBeNull();

  act(() => {
    vi.advanceTimersByTime(150);
  });
  expect(document.querySelector("[role=menu]")).toBeNull();

  // Hovering back in mid-shrink reopens it rather than letting it go.
  fireEvent.click(trigger);
  fireEvent.mouseDown(document.body);
  fireEvent.mouseEnter(trigger);
  act(() => {
    vi.advanceTimersByTime(150);
  });
  expect(screen.getByRole("menu")).not.toHaveClass("is-closing");
});
