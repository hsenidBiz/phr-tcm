import { act, fireEvent, render, screen } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { Collapse, foldMs, useSettled } from "./collapse";

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
  // By the timer here - the animation's own finish does the same in a
  // browser, but never comes for a row content-visibility has skipped.
  act(() => {
    vi.advanceTimersByTime(700);
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

// ---- Timing that follows the height --------------------------------------
// Field report: a group of a few hundred cases showed no animation at all.
// It had one - the same quarter-second as a three-line detail, spent
// crossing the whole window. Now only the part on screen moves, and the
// time grows with it.

test("a taller fold takes longer, within bounds", () => {
  expect(foldMs(0)).toBe(200);
  expect(foldMs(150)).toBeGreaterThan(foldMs(0));
  expect(foldMs(900)).toBeGreaterThan(foldMs(150));
  expect(foldMs(900)).toBeGreaterThanOrEqual(450);
  expect(foldMs(50_000)).toBe(600);
});

/** jsdom has no Web Animations and no layout: stand both in, recording
 * what the fold asked for. */
function stubMotion(height: number, top = 100) {
  const calls: { el: Element; frames: Keyframe[]; opts: KeyframeAnimationOptions }[] = [];
  const realAnimate = Element.prototype.animate;
  const realRect = Element.prototype.getBoundingClientRect;
  Element.prototype.animate = function (this: Element, frames: Keyframe[], opts: KeyframeAnimationOptions) {
    calls.push({ el: this, frames, opts });
    return { cancel() {}, onfinish: null } as unknown as Animation;
  } as typeof Element.prototype.animate;
  Element.prototype.getBoundingClientRect = function () {
    return { top, bottom: top + height, height, left: 0, right: 0, width: 0, x: 0, y: top, toJSON() {} } as DOMRect;
  };
  return {
    calls,
    restore() {
      Element.prototype.animate = realAnimate;
      Element.prototype.getBoundingClientRect = realRect;
    },
  };
}

test("opening a list taller than the window grows only what is on screen, for longer", () => {
  // A group 20,000px tall, starting 100px down a 768px window.
  window.innerHeight = 768;
  const m = stubMotion(20_000);
  try {
    render(<Host />);
    const grow = m.calls.find((c) => (c.el as HTMLElement).classList.contains("t-collapse"));
    expect(grow).toBeDefined();
    // To the window's bottom edge, not to 20,000px...
    expect(grow!.frames).toEqual([{ height: "0px" }, { height: "668px" }]);
    // ...over a time sized for that distance.
    expect(grow!.opts.duration).toBe(foldMs(668));
    expect(grow!.opts.duration as number).toBeGreaterThan(foldMs(150));
  } finally {
    m.restore();
  }
});

test("a short detail grows its whole height, quickly", () => {
  window.innerHeight = 768;
  const m = stubMotion(120);
  try {
    render(<Host />);
    const grow = m.calls.find((c) => (c.el as HTMLElement).classList.contains("t-collapse"));
    expect(grow!.frames).toEqual([{ height: "0px" }, { height: "120px" }]);
    expect(grow!.opts.duration).toBe(foldMs(120));
  } finally {
    m.restore();
  }
});

test("closing a tall list shrinks its visible part and waits for exactly that long", () => {
  vi.useFakeTimers();
  window.innerHeight = 768;
  const m = stubMotion(20_000);
  try {
    render(<Host />);
    m.calls.length = 0;
    fireEvent.click(screen.getByText("Toggle"));
    const ghost = document.querySelector(".t-collapse.is-closing") as HTMLElement;
    expect(ghost).not.toBeNull();
    const shrink = m.calls.find((c) => c.el === ghost);
    expect(shrink!.frames).toEqual([{ height: "668px" }, { height: "0px" }]);
    const ms = foldMs(668);
    expect(shrink!.opts.duration).toBe(ms);
    act(() => {
      vi.advanceTimersByTime(ms - 1);
    });
    expect(document.querySelector(".is-closing")).not.toBeNull();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(document.querySelector(".is-closing")).toBeNull();
  } finally {
    m.restore();
  }
});

test("a fold entirely below the window neither animates nor leaves a copy", () => {
  window.innerHeight = 768;
  const m = stubMotion(400, 2000);
  try {
    render(<Host />);
    expect(m.calls.filter((c) => (c.el as HTMLElement).classList.contains("t-collapse"))).toHaveLength(0);
    expect(document.querySelector(".t-collapse")).not.toHaveClass("is-entering");
    fireEvent.click(screen.getByText("Toggle"));
    expect(document.querySelector(".is-closing")).toBeNull();
  } finally {
    m.restore();
  }
});
