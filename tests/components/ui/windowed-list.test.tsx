import { render, screen } from "@testing-library/react";
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
