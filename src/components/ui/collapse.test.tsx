import { act, fireEvent, render, screen } from "@testing-library/react";
import { StrictMode, useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { Collapse, EASE, EASE_TALL, foldMs, useRegroupMotion, useSettled, type FoldMotion } from "./collapse";

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

// ---- The curve a tall fold grows on ---------------------------------------
// Field report: a group of about 120 cases grew, slowed to a crawl partway,
// then showed the rest at once. Part of that was the curve: the strong
// ease-out covers ~96% of the distance in half the time, so the second half
// of a window-tall grow barely moved. A tall span grows on a more even curve;
// a short detail keeps the quick settle it always had.

const growOf = (m: ReturnType<typeof stubMotion>) =>
  m.calls.find((c) => (c.el as HTMLElement).classList.contains("t-collapse"));

/** Run `fn` in a window `height` tall, putting the real height back after. */
function inWindow(height: number, fn: () => void) {
  const real = window.innerHeight;
  window.innerHeight = height;
  try {
    fn();
  } finally {
    window.innerHeight = real;
  }
}

test("a tall fold grows on the even curve, height and fade together", () => {
  // A 1000px group at the top of an 800px window: 800px on screen.
  inWindow(800, () => {
    const m = stubMotion(1000, 0);
    try {
      render(<Host />);
      const grow = growOf(m);
      expect(grow!.frames).toEqual([{ height: "0px" }, { height: "800px" }]);
      expect(EASE_TALL).not.toBe(EASE);
      expect(grow!.opts.easing).toBe(EASE_TALL);
      const fade = m.calls.find((c) => (c.el as HTMLElement).classList.contains("t-collapse-inner"));
      expect(fade!.opts.easing).toBe(EASE_TALL);
    } finally {
      m.restore();
    }
  });
});

test("a short fold keeps the strong ease-out", () => {
  inWindow(800, () => {
    const m = stubMotion(120, 0);
    try {
      render(<Host />);
      expect(growOf(m)!.opts.easing).toBe(EASE);
    } finally {
      m.restore();
    }
  });
});

test("under prefers-reduced-motion a fold opens without animating", () => {
  inWindow(800, () => {
    const realMatchMedia = window.matchMedia;
    window.matchMedia = ((q: string) => ({
      matches: q.includes("prefers-reduced-motion"),
      media: q,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    })) as unknown as typeof window.matchMedia;
    const m = stubMotion(1000, 0);
    try {
      render(<Host />);
      expect(m.calls).toHaveLength(0);
      expect(document.querySelector(".t-collapse")).not.toHaveClass("is-entering");
      expect(screen.getByText("Row one")).toBeInTheDocument();
      expect(document.querySelector("[data-unfolding]")).toBeNull();
    } finally {
      m.restore();
      window.matchMedia = realMatchMedia;
    }
  });
});

// ---- Which rows render up front ---------------------------------------------
// The other half of that report: the group's rows are content-visibility:
// auto, and a growing fold clips them, so they rendered a band at a time as
// the clip edge moved. The rows the grow can reach are marked to render
// before it starts (index.css) - but only those: a group of hundreds must not
// lay out every row in its opening frame for a grow that shows one window.

/** A 100-row group at the top of the window, each row 40px, with jsdom's
 * missing layout and Web Animations stood in. */
function stubRows() {
  const anims: { el: Element; anim: { cancel(): void; onfinish: (() => void) | null } }[] = [];
  const realAnimate = Element.prototype.animate;
  const realRect = Element.prototype.getBoundingClientRect;
  Element.prototype.animate = function (this: Element) {
    const anim = { cancel() {}, onfinish: null as (() => void) | null };
    anims.push({ el: this, anim });
    return anim as unknown as Animation;
  } as typeof Element.prototype.animate;
  Element.prototype.getBoundingClientRect = function (this: Element) {
    const y = (this as HTMLElement).dataset?.y;
    const top = y == null ? 0 : Number(y);
    const height = y == null ? 4000 : 40;
    return { top, bottom: top + height, height, left: 0, right: 0, width: 0, x: 0, y: top, toJSON() {} } as DOMRect;
  };
  return {
    grow: () => anims.find((a) => (a.el as HTMLElement).classList.contains("t-collapse"))!.anim,
    restore() {
      Element.prototype.animate = realAnimate;
      Element.prototype.getBoundingClientRect = realRect;
    },
  };
}

function Group() {
  const [open, setOpen] = useState(true);
  return (
    <div>
      <button onClick={() => setOpen((o) => !o)}>Toggle</button>
      <Collapse open={open}>
        <ul>
          {Array.from({ length: 100 }, (_, i) => (
            <li key={i} className="cv-row" data-y={i * 40}>
              Case {i}
            </li>
          ))}
        </ul>
      </Collapse>
    </div>
  );
}

const rows = () => [...document.querySelectorAll<HTMLElement>(".t-collapse:not(.is-closing) .cv-row")];
const marked = () => rows().filter((r) => r.hasAttribute("data-unfolding"));

test("a tall grow renders up front only the rows it can reach, until it ends", () => {
  inWindow(800, () => {
    const m = stubRows();
    try {
      render(<Group />);
      // 800px on screen plus one more window: rows starting above 1600px.
      expect(marked()).toEqual(rows().slice(0, 40));
      expect(rows()[40]).not.toHaveAttribute("data-unfolding");
      expect(rows()[99]).not.toHaveAttribute("data-unfolding");
      act(() => m.grow().onfinish?.());
      expect(document.querySelector("[data-unfolding]")).toBeNull();
    } finally {
      m.restore();
    }
  });
});

test("the marks come off by the timer when the grow never reports its end", () => {
  vi.useFakeTimers();
  inWindow(800, () => {
    const m = stubRows();
    try {
      render(<Group />);
      expect(marked()).toHaveLength(40);
      act(() => {
        vi.advanceTimersByTime(700);
      });
      expect(document.querySelector("[data-unfolding]")).toBeNull();
    } finally {
      m.restore();
    }
  });
});

test("closing mid-grow leaves no marked row behind, in the copy or the page", () => {
  vi.useFakeTimers();
  inWindow(800, () => {
    const m = stubRows();
    try {
      render(<Group />);
      expect(marked()).toHaveLength(40);
      fireEvent.click(screen.getByText("Toggle"));
      expect(document.querySelector(".is-closing")).not.toBeNull();
      expect(document.querySelector("[data-unfolding]")).toBeNull();
    } finally {
      m.restore();
    }
  });
});

/// Group by title switched on again: the groups it mounts are already open,
/// so none plays its own unfold after the regroup - the list plays one
/// entrance instead. A plain re-render replays nothing.
test("regrouping mounts groups open and plays one entrance for the list", () => {
  function Harness() {
    const [grouped, setGrouped] = useState(true);
    const [tick, setTick] = useState(0);
    const settled = useSettled(true);
    const regroup = useRegroupMotion(grouped);
    return (
      <>
        <button onClick={() => setGrouped((g) => !g)}>toggle</button>
        <button onClick={() => setTick((t) => t + 1)}>rerender {tick}</button>
        <div data-testid="list" ref={regroup.ref}>
          {grouped ? (
            ["A", "B"].map((g) => (
              <Collapse key={g} open animateIn={settled && !regroup.regrouping}>
                <p>group {g}</p>
              </Collapse>
            ))
          ) : (
            <p>flat</p>
          )}
        </div>
      </>
    );
  }
  render(<Harness />);
  const list = screen.getByTestId("list");
  expect(list).not.toHaveClass("t-panel-in");

  fireEvent.click(screen.getByText("toggle")); // flat
  expect(list).toHaveClass("t-panel-in");
  fireEvent.click(screen.getByText("toggle")); // grouped again
  expect(list).toHaveClass("t-panel-in");
  for (const panel of document.querySelectorAll(".t-collapse")) {
    expect(panel).not.toHaveClass("is-entering");
  }

  list.classList.remove("t-panel-in");
  fireEvent.click(screen.getByText(/rerender/));
  expect(list).not.toHaveClass("t-panel-in");
});

// ---- Reporting the motion, for motion planned around it ---------------------
// Settings' cards slide in step with the changelog's fold: the fold says how
// its edge will move (onGrow / onShrink), and the grow can be held shut
// while a card clears the way.

function Reporting({
  onGrow,
  onShrink,
}: {
  onGrow?: (g: FoldMotion) => number | void;
  onShrink?: (s: FoldMotion) => void;
}) {
  const [open, setOpen] = useState(true);
  return (
    <div>
      <button onClick={() => setOpen((o) => !o)}>Toggle</button>
      <Collapse open={open} onGrow={onGrow} onShrink={onShrink}>
        <p>Row one</p>
      </Collapse>
    </div>
  );
}

test("onGrow hears the grow's measure, and the time it returns holds the fold shut first", () => {
  window.innerHeight = 768;
  const m = stubMotion(120);
  const onGrow = vi.fn(() => 150);
  try {
    render(<Reporting onGrow={onGrow} />);
    expect(onGrow).toHaveBeenCalledWith({ span: 120, margin: 0, ms: foldMs(120), easing: EASE });
    const grow = m.calls.find((c) => (c.el as HTMLElement).classList.contains("t-collapse"))!;
    expect(grow.opts).toMatchObject({ duration: foldMs(120), delay: 150, fill: "backwards" });
    // The content's fade waits with it.
    const fade = m.calls.find((c) => (c.el as HTMLElement).classList.contains("t-collapse-inner"))!;
    expect(fade.opts).toMatchObject({ delay: 150, fill: "backwards" });
  } finally {
    m.restore();
  }
});

test("without a hold the grow starts at once, as before", () => {
  window.innerHeight = 768;
  const m = stubMotion(120);
  try {
    render(<Reporting onGrow={() => undefined} />);
    const grow = m.calls.find((c) => (c.el as HTMLElement).classList.contains("t-collapse"))!;
    expect(grow.opts).toEqual({ duration: foldMs(120), easing: EASE });
  } finally {
    m.restore();
  }
});

test("onShrink hears the closing copy's measure", () => {
  window.innerHeight = 768;
  const m = stubMotion(20_000);
  const onShrink = vi.fn();
  try {
    render(<Reporting onShrink={onShrink} />);
    fireEvent.click(screen.getByText("Toggle"));
    expect(onShrink).toHaveBeenCalledWith({ span: 668, margin: 0, ms: foldMs(668), easing: EASE });
  } finally {
    m.restore();
  }
});

test("the closing copy shrinks its outer margin away with it, so nothing below jumps at the end", () => {
  window.innerHeight = 768;
  const style = document.createElement("style");
  style.textContent = ".t-collapse { margin-bottom: 16px; }";
  document.head.appendChild(style);
  const m = stubMotion(120);
  const onShrink = vi.fn();
  try {
    render(<Reporting onShrink={onShrink} />);
    m.calls.length = 0;
    fireEvent.click(screen.getByText("Toggle"));
    const ghost = document.querySelector(".t-collapse.is-closing") as HTMLElement;
    const shrink = m.calls.find((c) => c.el === ghost)!;
    expect(shrink.frames).toEqual([
      { height: "120px", marginTop: "0px", marginBottom: "16px" },
      { height: "0px", marginTop: "0px", marginBottom: "0px" },
    ]);
    expect(onShrink).toHaveBeenCalledWith(expect.objectContaining({ span: 120, margin: 16 }));
  } finally {
    m.restore();
    style.remove();
  }
});

/** A fold in a spaced list, as Tailwind's space-y spells it: every child
 * but the last gets the gap. `last` puts the fold at the end. */
function Spaced({ last }: { last: boolean }) {
  const [open, setOpen] = useState(true);
  return (
    <div>
      <button onClick={() => setOpen((o) => !o)}>Toggle</button>
      <div className="spaced">
        <p>Before</p>
        <Collapse open={open}>
          <p>Row one</p>
        </Collapse>
        {!last && <p>After</p>}
      </div>
    </div>
  );
}

test.each([
  [true, 0],
  [false, 16],
])("the closing copy starts from the fold's own margin in a spaced list (last child: %s)", (last, margin) => {
  window.innerHeight = 768;
  const style = document.createElement("style");
  style.textContent = ".spaced > :not(:last-child) { margin-bottom: 16px; }";
  document.head.appendChild(style);
  const m = stubMotion(120);
  try {
    render(<Spaced last={last} />);
    m.calls.length = 0;
    fireEvent.click(screen.getByText("Toggle"));
    const ghost = document.querySelector(".t-collapse.is-closing") as HTMLElement;
    const shrink = m.calls.find((c) => c.el === ghost)!;
    // Read before the copy went in: a last child is not spaced, even though
    // the copy briefly follows it - reading it then started the close with
    // a 16px jump.
    expect(shrink.frames[0]).toEqual(
      margin ? { height: "120px", marginTop: "0px", marginBottom: `${margin}px` } : { height: "120px" },
    );
  } finally {
    m.restore();
    style.remove();
  }
});

test("under StrictMode the grow keeps the hold onGrow gave it, though the effect runs twice", () => {
  window.innerHeight = 768;
  const m = stubMotion(120);
  // The caller plans once: asked again, it has nothing left to hold for.
  const onGrow = vi.fn().mockReturnValueOnce(150).mockReturnValue(0);
  try {
    render(
      <StrictMode>
        <Reporting onGrow={onGrow} />
      </StrictMode>,
    );
    expect(onGrow).toHaveBeenCalledTimes(1);
    const grows = m.calls.filter((c) => (c.el as HTMLElement).classList.contains("t-collapse"));
    expect(grows.length).toBeGreaterThan(1);
    expect(grows[grows.length - 1].opts).toMatchObject({ delay: 150, fill: "backwards" });
  } finally {
    m.restore();
  }
});
