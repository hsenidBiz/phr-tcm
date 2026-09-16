import { act, fireEvent, render, screen } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { Collapse, useSettled } from "./collapse";

afterEach(() => {
  vi.useRealTimers();
});

function Host({ animateIn = true, start = true }: { animateIn?: boolean; start?: boolean }) {
  const [open, setOpen] = useState(start);
  return (
    <div data-testid="group">
      <button onClick={() => setOpen((o) => !o)}>Toggle</button>
      <Collapse open={open} animateIn={animateIn}>
        <ul>
          <li>Row one</li>
          <li>Row two</li>
        </ul>
      </Collapse>
      <p>After</p>
    </div>
  );
}

test("open content grows in; closing shrinks a copy in place, then removes it", () => {
  vi.useFakeTimers();
  render(<Host />);
  const panel = document.querySelector(".t-collapse") as HTMLElement;
  expect(panel).toHaveClass("is-entering");
  expect(screen.getByText("Row one")).toBeInTheDocument();
  // Grown: the clip comes off, so a dropdown inside can paint past the box.
  // By the timer here - the animation's own end event does the same in a
  // browser, but never fires for a row content-visibility has skipped.
  act(() => {
    vi.advanceTimersByTime(300);
  });
  expect(panel).not.toHaveClass("is-entering");

  fireEvent.click(screen.getByText("Toggle"));
  // The rows are out of the page at once...
  expect(screen.queryByText("Row one")).not.toBeInTheDocument();
  // ...and their copy shrinks where they were: between the button and the
  // paragraph, not at the end of the document.
  const ghost = document.querySelector(".t-collapse.is-closing") as HTMLElement;
  expect(ghost).not.toBeNull();
  expect(ghost.parentElement).toBe(screen.getByTestId("group"));
  expect(ghost.nextElementSibling?.textContent).toBe("After");
  expect(ghost).toHaveAttribute("aria-hidden", "true");

  act(() => {
    vi.advanceTimersByTime(250);
  });
  expect(document.querySelector(".t-collapse")).toBeNull();

  // Opening again grows in afresh.
  fireEvent.click(screen.getByText("Toggle"));
  expect(document.querySelector(".t-collapse")).toHaveClass("is-entering");
});

test("animateIn off mounts the content settled, with no grow", () => {
  render(<Host animateIn={false} />);
  const panel = document.querySelector(".t-collapse");
  expect(panel).not.toHaveClass("is-entering");
  expect(screen.getByText("Row two")).toBeInTheDocument();
});

test("StrictMode's rehearsal unmount leaves no copy behind", () => {
  vi.useFakeTimers();
  render(
    <StrictMode>
      <Host />
    </StrictMode>,
  );
  expect(document.querySelectorAll(".t-collapse")).toHaveLength(1);
  expect(document.querySelector(".is-closing")).toBeNull();
  act(() => {
    vi.advanceTimersByTime(300);
  });
  expect(document.querySelectorAll(".t-collapse")).toHaveLength(1);
});

/// A screen's groups arrive with its data; they must not all unfold then.
/// Only what the user opens afterwards grows in.
test("useSettled turns true one commit after the content is ready", () => {
  const seen: boolean[] = [];
  function Probe({ ready }: { ready: boolean }) {
    seen.push(useSettled(ready));
    return null;
  }
  const { rerender } = render(<Probe ready={false} />);
  rerender(<Probe ready={false} />);
  expect(seen).toEqual([false, false]);
  rerender(<Probe ready />);
  // The commit that brings the data renders unsettled; the effect after it
  // settles, and the re-render that follows is what a later toggle sees.
  expect(seen.slice(2)).toEqual([false, true]);
});

/// Run Tests unfolds a preview beneath a table row, where no div can go:
/// the row form puts the box in a cell of its own row, and the copy that
/// shrinks on close is the whole row, in place in the table.
test("the row form folds inside a table row and shrinks as a row", () => {
  vi.useFakeTimers();
  function Table() {
    const [open, setOpen] = useState(true);
    return (
      <table>
        <tbody>
          <tr>
            <td>
              <button onClick={() => setOpen((o) => !o)}>Toggle</button>
            </td>
          </tr>
          <Collapse row={3} open={open}>
            <p>Preview</p>
          </Collapse>
          <tr>
            <td>After</td>
          </tr>
        </tbody>
      </table>
    );
  }
  render(<Table />);
  const row = document.querySelector("tr.t-collapse-row") as HTMLTableRowElement;
  expect(row.querySelector("td")).toHaveAttribute("colspan", "3");
  expect(row.querySelector(".t-collapse .t-collapse-inner")?.textContent).toBe("Preview");

  fireEvent.click(screen.getByText("Toggle"));
  expect(screen.queryByText("Preview")).not.toBeInTheDocument();
  const ghost = document.querySelector("tr.t-collapse-row.is-closing");
  expect(ghost).not.toBeNull();
  expect(ghost?.parentElement?.tagName).toBe("TBODY");
  expect(ghost?.nextElementSibling?.textContent).toBe("After");
  act(() => {
    vi.advanceTimersByTime(250);
  });
  expect(document.querySelector("tr.t-collapse-row")).toBeNull();
});
