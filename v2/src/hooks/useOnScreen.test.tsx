import { render, screen, act } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, expect, test } from "vitest";
import { useOnScreen } from "./useOnScreen";

const real = globalThis.IntersectionObserver;
afterEach(() => {
  globalThis.IntersectionObserver = real;
});

/** A stub whose callback the test can fire by hand. */
function stubObserver() {
  let fire: ((entries: { isIntersecting: boolean }[]) => void) | null = null;
  globalThis.IntersectionObserver = class {
    constructor(cb: (entries: { isIntersecting: boolean }[]) => void) {
      fire = cb;
    }
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
  return () => fire;
}

function Probe() {
  const ref = useRef<HTMLDivElement | null>(null);
  const onScreen = useOnScreen(ref);
  return (
    <div ref={ref} data-testid="probe">
      {onScreen ? "in view" : "off screen"}
    </div>
  );
}

test("assumes visible, then follows the observer", () => {
  const get = stubObserver();
  render(<Probe />);
  expect(screen.getByTestId("probe")).toHaveTextContent("in view");

  act(() => get()!([{ isIntersecting: false }]));
  expect(screen.getByTestId("probe")).toHaveTextContent("off screen");

  act(() => get()!([{ isIntersecting: true }]));
  expect(screen.getByTestId("probe")).toHaveTextContent("in view");
});

test("a browser with no observer reports visible and stays quiet", () => {
  // @ts-expect-error - deleting the global is the condition under test.
  delete globalThis.IntersectionObserver;
  render(<Probe />);
  expect(screen.getByTestId("probe")).toHaveTextContent("in view");
});
