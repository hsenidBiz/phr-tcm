import { act, fireEvent, render } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import WidthReveal from "./WidthReveal";

function mockReduced(reduced: boolean) {
  window.matchMedia = vi.fn().mockImplementation((q: string) => ({
    matches: reduced && q.includes("reduce"),
    media: q,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    onchange: null,
    dispatchEvent: vi.fn(),
  })) as unknown as typeof window.matchMedia;
}

afterEach(() => {
  vi.restoreAllMocks();
});

test("renders nothing while hidden at mount", () => {
  mockReduced(false);
  const { container } = render(<WidthReveal show={false}>hello</WidthReveal>);
  expect(container).toBeEmptyDOMElement();
});

test("keeps children until transitionend, then removes them", () => {
  mockReduced(false);
  const { container, rerender } = render(<WidthReveal show={false}>one</WidthReveal>);
  rerender(<WidthReveal show>one</WidthReveal>);
  expect(container).toHaveTextContent("one");
  rerender(<WidthReveal show={false}>zero</WidthReveal>);
  // Still there for the collapse, with the last shown content, hidden.
  expect(container).toHaveTextContent("one");
  expect(container).not.toHaveTextContent("zero");
  const el = container.querySelector("[data-width-reveal]") as HTMLElement;
  expect(el).toHaveAttribute("aria-hidden", "true");
  act(() => {
    fireEvent.transitionEnd(el);
  });
  expect(container).toBeEmptyDOMElement();
});

test("reduced motion removes children immediately", () => {
  mockReduced(true);
  const { container, rerender } = render(<WidthReveal show>one</WidthReveal>);
  expect(container).toHaveTextContent("one");
  rerender(<WidthReveal show={false}>one</WidthReveal>);
  expect(container).toBeEmptyDOMElement();
});

test("settled is false while animating and after hide, true once the enter ends", () => {
  mockReduced(false);
  const seen: boolean[] = [];
  const kids = (s: boolean) => {
    seen.push(s);
    return <span>{s ? "settled" : "moving"}</span>;
  };
  const { container, rerender } = render(<WidthReveal show={false}>{kids}</WidthReveal>);
  rerender(<WidthReveal show>{kids}</WidthReveal>);
  expect(container).toHaveTextContent("moving");
  act(() => {
    fireEvent.transitionEnd(container.querySelector("[data-width-reveal]") as HTMLElement);
  });
  expect(container).toHaveTextContent("settled");
  rerender(<WidthReveal show={false}>{kids}</WidthReveal>);
  expect(container).toHaveTextContent("moving");
});

test("settled is true at once under reduced motion", () => {
  mockReduced(true);
  const { container } = render(
    <WidthReveal show>{(s: boolean) => <span>{s ? "settled" : "moving"}</span>}</WidthReveal>,
  );
  expect(container).toHaveTextContent("settled");
});

test("collapsed footprint is 0 and the open gap is the parent's: margin carries the gap, no inner padding", () => {
  mockReduced(false);
  const { container, rerender } = render(<WidthReveal show={false}>x</WidthReveal>);
  rerender(<WidthReveal show>x</WidthReveal>);
  const el = container.querySelector("[data-width-reveal]") as HTMLElement;
  // Entering (not yet open): pulled left by exactly one gap, so gap + margin = 0.
  expect(el.style.marginLeft).toMatch(/^calc\((-1 \* )?-?0\.5rem\)$/);
  // A grid item cannot shrink below its padding, so the clip box must have none.
  const inner = el.firstElementChild as HTMLElement;
  expect(inner.style.paddingLeft).toBe("");
  expect(inner.className).toContain("min-w-0");
  expect(inner.className).toContain("overflow-hidden");
  expect(el.className).toContain("grid-cols-[0fr]");
});
