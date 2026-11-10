import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { WINDOW_FROM } from "./useRowWindow";
import { WindowedList } from "./windowed-list";

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
