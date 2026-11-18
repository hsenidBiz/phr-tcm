import { act, render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { WINDOW_FROM } from "../../../src/components/ui/useRowWindow";
import { WindowedList } from "../../../src/components/ui/windowed-list";

// jsdom does no layout, so the window itself (which rows are near the
// visible part) can only be seen in a real browser. What jsdom can pin is
// that a list shorter than WINDOW_FROM renders exactly as a plain <ul>.
test("a short list renders every row and no spacer", () => {
  const n = WINDOW_FROM - 1;
  render(
    <WindowedList
      count={n}
      estimate={40}
      gap={4}
      keyOf={(i) => i}
      renderRow={(i, measure) => (
        <li key={i} ref={measure} data-index={i}>
          row {i}
        </li>
      )}
    />,
  );
  expect(screen.getAllByRole("listitem")).toHaveLength(n);
  expect(document.querySelectorAll("li[aria-hidden]")).toHaveLength(0);
});

// A windowed list corrects the scroll position itself as rows are measured.
// The browser's scroll anchoring corrected it too, and the two fought: a long
// Run Tests group of tall rows bounced up and down on the way to its end. So
// the page that scrolls a windowed list has anchoring off while it is there.
function Scroller({ count, show = true }: { count: number; show?: boolean }) {
  return (
    <div data-testid="scroller" style={{ overflowY: "auto" }}>
      {show && (
        <WindowedList
          count={count}
          estimate={40}
          keyOf={(i) => i}
          renderRow={(i, measure) => (
            <li key={i} ref={measure} data-index={i}>
              row {i}
            </li>
          )}
        />
      )}
    </div>
  );
}

test("a windowed list turns scroll anchoring off on its scroller, and back on when it goes", () => {
  const { rerender } = render(<Scroller count={WINDOW_FROM} />);
  const scroller = screen.getByTestId("scroller");
  expect(scroller.style.overflowAnchor).toBe("none");
  rerender(<Scroller count={WINDOW_FROM} show={false} />);
  expect(scroller.style.overflowAnchor).toBe("");
});

test("a short list leaves scroll anchoring alone", () => {
  render(<Scroller count={WINDOW_FROM - 1} />);
  expect(screen.getByTestId("scroller").style.overflowAnchor).toBe("");
});

// Where a windowed list starts on its page moves when something above it
// folds shut - another group closing - without the list itself rendering
// again. A list that kept the old start drew too few rows until the page was
// scrolled. So the list watches the page's content for size changes and
// measures its start again then.
test("a windowed list measures where it starts again when the page above it changes size", () => {
  const observed: Element[] = [];
  let fire: () => void = () => {};
  const RO = globalThis.ResizeObserver;
  globalThis.ResizeObserver = class {
    constructor(cb: ResizeObserverCallback) {
      fire = () => cb([], this as unknown as ResizeObserver);
    }
    observe(el: Element) {
      observed.push(el);
    }
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver;
  try {
    render(
      <div data-testid="scroller" style={{ overflowY: "auto" }}>
        <div data-testid="content">
          <WindowedList
            count={WINDOW_FROM}
            estimate={40}
            keyOf={(i) => i}
            renderRow={(i, measure) => (
              <li key={i} ref={measure} data-index={i}>
                row {i}
              </li>
            )}
          />
        </div>
      </div>,
    );
    expect(observed).toContain(screen.getByTestId("content"));
    // A size change re-measures without throwing (jsdom has no layout to
    // move; the real-browser check is in the commit that added this).
    act(() => fire());
  } finally {
    globalThis.ResizeObserver = RO;
  }
});
