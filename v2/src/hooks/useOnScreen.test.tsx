import { render, screen, act } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { useOnScreen } from "./useOnScreen";

const real = globalThis.IntersectionObserver;
afterEach(() => {
  globalThis.IntersectionObserver = real;
});

/** A stub whose callback the test can fire by hand, and which reports
 * whether anything is being watched right now. */
function stubObserver() {
  let fire: ((entries: { isIntersecting: boolean }[]) => void) | null = null;
  let watching = 0;
  globalThis.IntersectionObserver = class {
    constructor(cb: (entries: { isIntersecting: boolean }[]) => void) {
      fire = cb;
    }
    observe() {
      watching += 1;
    }
    unobserve() {
      watching -= 1;
    }
    disconnect() {
      fire = null;
      watching = 0;
    }
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
  return { get: () => fire, watching: () => watching };
}

/** The target is optional, the way the queue's action row is: it only
 * exists once there is something in the queue. */
function Probe({ target = true }: { target?: boolean }) {
  const [ref, onScreen] = useOnScreen();
  return (
    <div data-testid="probe">
      {target && <div ref={ref} data-testid="target" />}
      {onScreen ? "in view" : "off screen"}
    </div>
  );
}

test("assumes visible, then follows the observer", () => {
  const io = stubObserver();
  render(<Probe />);
  expect(screen.getByTestId("probe")).toHaveTextContent("in view");

  act(() => io.get()!([{ isIntersecting: false }]));
  expect(screen.getByTestId("probe")).toHaveTextContent("off screen");

  act(() => io.get()!([{ isIntersecting: true }]));
  expect(screen.getByTestId("probe")).toHaveTextContent("in view");
});

test("a browser with no observer reports visible and stays quiet", () => {
  // @ts-expect-error - deleting the global is the condition under test.
  delete globalThis.IntersectionObserver;
  render(<Probe />);
  expect(screen.getByTestId("probe")).toHaveTextContent("in view");
});

// The queue's action row is not in the page until the queue has a case in
// it, so the hook has to start watching when the element ARRIVES - not
// only if it happened to be there at mount. A hook that reads a ref once
// sees null here and never looks again.
test("picks up a target that only appears after mount", () => {
  const io = stubObserver();
  const { rerender } = render(<Probe target={false} />);
  expect(io.watching()).toBe(0);
  expect(screen.getByTestId("probe")).toHaveTextContent("in view");

  rerender(<Probe target />);
  expect(io.watching()).toBe(1);

  act(() => io.get()!([{ isIntersecting: false }]));
  expect(screen.getByTestId("probe")).toHaveTextContent("off screen");
});

// Nothing left to watch means nothing left to answer with, and the safe
// answer is "on screen": a floating copy that can no longer be told to
// stand down must not be the thing left on the user's screen.
test("a target that goes away goes back to assuming visible", () => {
  const io = stubObserver();
  const { rerender } = render(<Probe />);
  act(() => io.get()!([{ isIntersecting: false }]));
  expect(screen.getByTestId("probe")).toHaveTextContent("off screen");

  rerender(<Probe target={false} />);
  expect(screen.getByTestId("probe")).toHaveTextContent("in view");
});
