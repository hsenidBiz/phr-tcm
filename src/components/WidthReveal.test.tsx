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
