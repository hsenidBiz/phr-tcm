import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ListToolbar, ToolbarSwitch } from "./list-toolbar";

test("the toolbar wraps, the search slot grows and shrinks first, filters never shrink", () => {
  const { container } = render(
    <ListToolbar search={<input aria-label="Find" />}>
      <select aria-label="Outcome" />
    </ListToolbar>,
  );
  const row = container.firstElementChild!;
  expect(row).toHaveClass("flex", "flex-wrap", "items-center", "gap-2", "*:shrink-0");
  const searchSlot = screen.getByLabelText("Find").parentElement!;
  expect(searchSlot).toHaveClass("flex-1", "min-w-64");
  // A filter sits straight in the row (the row's *:shrink-0 holds it).
  expect(screen.getByLabelText("Outcome").parentElement).toBe(row);
});

test("a filter that renders nothing leaves no empty box, so the gaps stay even", () => {
  const Nothing = () => null;
  const scope = "";
  const { container } = render(
    <ListToolbar search={<input aria-label="Find" />}>
      <Nothing />
      {scope && <span>never</span>}
      <select aria-label="Outcome" />
    </ListToolbar>,
  );
  // The search slot and the one real filter: nothing in between.
  expect(container.firstElementChild!.children).toHaveLength(2);
});

test("the toolbar sticks to the top, borderless at rest, on the page's or the card's background", () => {
  const { container, rerender } = render(<ListToolbar search={<input aria-label="Find" />} />);
  const bar = container.firstElementChild!;
  expect(bar).toHaveClass("sticky", "z-20", "border", "border-transparent", "rounded-md", "bg-bg");
  // No scrolling box around it here: one pixel up, for the stuck check.
  expect((bar as HTMLElement).style.top).toBe("-1px");
  expect(bar).not.toHaveAttribute("data-stuck");
  rerender(<ListToolbar surface search={<input aria-label="Find" />} />);
  expect(container.firstElementChild).toHaveClass("bg-surface");
  expect(container.firstElementChild).not.toHaveClass("bg-bg");
});

test("the border shows only while the toolbar is stuck, and goes again at the top", () => {
  let fire: (top: number, ratio: number) => void = () => {};
  const Real = globalThis.IntersectionObserver;
  globalThis.IntersectionObserver = class {
    constructor(cb: IntersectionObserverCallback) {
      fire = (top, ratio) =>
        cb(
          [{ intersectionRatio: ratio, boundingClientRect: { top }, rootBounds: { top: 100 } } as unknown as IntersectionObserverEntry],
          this as unknown as IntersectionObserver,
        );
    }
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
  try {
    const { container } = render(<ListToolbar search={<input aria-label="Find" />} />);
    const bar = container.firstElementChild!;
    // Stuck: one pixel above the scroller's top, so not wholly inside it.
    act(() => fire(99, 0.98));
    expect(bar).toHaveClass("border-b-border", "rounded-none");
    expect(bar).toHaveAttribute("data-stuck");
    // Scrolled back to the top: wholly in view again.
    act(() => fire(124, 1));
    expect(bar).toHaveClass("border-transparent", "rounded-md");
    expect(bar).not.toHaveClass("border-b-border");
    expect(bar).not.toHaveAttribute("data-stuck");
    // Below the fold (partly hidden at the BOTTOM) is not stuck.
    act(() => fire(700, 0.5));
    expect(bar).not.toHaveAttribute("data-stuck");
  } finally {
    globalThis.IntersectionObserver = Real;
  }
});

test("fullWidth={false}: stuck, it stays in its column as a box bordered all round", () => {
  let fire: (top: number, ratio: number) => void = () => {};
  const Real = globalThis.IntersectionObserver;
  globalThis.IntersectionObserver = class {
    constructor(cb: IntersectionObserverCallback) {
      fire = (top, ratio) =>
        cb(
          [{ intersectionRatio: ratio, boundingClientRect: { top }, rootBounds: { top: 100 } } as unknown as IntersectionObserverEntry],
          this as unknown as IntersectionObserver,
        );
    }
    observe() {}
    unobserve() {}
    disconnect() {}
    takeRecords() {
      return [];
    }
  } as unknown as typeof IntersectionObserver;
  try {
    const { container } = render(<ListToolbar fullWidth={false} search={<input aria-label="Find" />} />);
    const bar = container.firstElementChild as HTMLElement;
    act(() => fire(99, 0.98));
    expect(bar).toHaveAttribute("data-stuck");
    expect(bar).toHaveClass("rounded-md", "border-border");
    expect(bar).not.toHaveClass("rounded-none");
    expect(bar.style.marginLeft).toBe("");
    expect(bar.style.marginRight).toBe("");
  } finally {
    globalThis.IntersectionObserver = Real;
  }
});

test("inside a padded scrolling box, the bar sticks at the box's edge, not under its padding", () => {
  render(
    <div style={{ overflowY: "auto", paddingTop: "24px" }}>
      <ListToolbar search={<input aria-label="Find" />} />
    </div>,
  );
  const bar = screen.getByLabelText("Find").closest("[data-list-toolbar]") as HTMLElement;
  expect(bar.style.top).toBe("-25px");
});

test("the toolbar renders with only a search", () => {
  render(<ListToolbar search={<input aria-label="Find" />} />);
  expect(screen.getByLabelText("Find")).toBeInTheDocument();
});

test("the toolbar switch is 40px tall, unboxed, and named by its label", () => {
  render(<ToolbarSwitch checked={false} onCheckedChange={() => {}} label="Group by title" />);
  const sw = screen.getByRole("switch", { name: "Group by title" });
  expect(sw).toHaveAttribute("aria-checked", "false");
  const chip = sw.parentElement!;
  expect(chip).toHaveClass("h-10", "text-sm");
  expect(chip).not.toHaveClass("border");
  expect(chip).not.toHaveClass("rounded-md");
  expect(chip).toHaveTextContent("Group by title");
});

test("clicking the switch or its label toggles", () => {
  const onChange = vi.fn();
  render(<ToolbarSwitch checked={false} onCheckedChange={onChange} label="This sprint" />);
  fireEvent.click(screen.getByRole("switch", { name: "This sprint" }));
  expect(onChange).toHaveBeenLastCalledWith(true);
  fireEvent.click(screen.getByText("This sprint"));
  expect(onChange).toHaveBeenCalledTimes(2);
});

test("a checked switch reports on, and title lands on the chip", () => {
  render(<ToolbarSwitch checked onCheckedChange={() => {}} label="Open lanes only" title="Hide done lanes" />);
  const sw = screen.getByRole("switch", { name: "Open lanes only" });
  expect(sw).toHaveAttribute("aria-checked", "true");
  expect(sw.parentElement).toHaveAttribute("title", "Hide done lanes");
});
