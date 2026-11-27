// Nested modals must not fight over Escape. A modal has no idea another one
// is nested inside it - the bug this pins is Escape closing BOTH the inner
// dialog and whatever opened it, in one press.

import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test, vi } from "vitest";
import { Modal } from "../../../src/components/ui/modal";

function Nested({ onOuterClose, onInnerClose }: { onOuterClose: () => void; onInnerClose: () => void }) {
  const [innerOpen, setInnerOpen] = useState(true);
  return (
    <Modal onClose={onOuterClose} className="outer">
      <p>Outer</p>
      {innerOpen && (
        <Modal onClose={onInnerClose} className="inner">
          <p>Inner</p>
          <button onClick={() => setInnerOpen(false)}>Close inner without Escape</button>
        </Modal>
      )}
    </Modal>
  );
}

const escape = () => fireEvent.keyDown(window, { key: "Escape" });

test("a single modal closes on Escape", async () => {
  const onClose = vi.fn();
  render(<Modal onClose={onClose}>content</Modal>);
  escape();
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("Escape closes only the innermost of two nested modals", async () => {
  const onOuterClose = vi.fn();
  const onInnerClose = vi.fn();
  render(<Nested onOuterClose={onOuterClose} onInnerClose={onInnerClose} />);
  expect(await screen.findByText("Inner")).toBeInTheDocument();

  escape();
  expect(onInnerClose).toHaveBeenCalledTimes(1);
  expect(onOuterClose).not.toHaveBeenCalled();
});

test("once the inner modal is gone, Escape reaches the outer one", async () => {
  const onOuterClose = vi.fn();
  const onInnerClose = vi.fn();
  render(<Nested onOuterClose={onOuterClose} onInnerClose={onInnerClose} />);
  await screen.findByText("Inner");

  // Unmounted by something other than Escape - a button inside it, say -
  // so the outer modal's own listener has to notice on its own.
  fireEvent.click(screen.getByRole("button", { name: "Close inner without Escape" }));
  expect(screen.queryByText("Inner")).not.toBeInTheDocument();

  escape();
  expect(onOuterClose).toHaveBeenCalledTimes(1);
  expect(onInnerClose).not.toHaveBeenCalled();
});

test("a modal is named by the heading it points at, or by a label", () => {
  const { unmount } = render(
    <Modal onClose={vi.fn()} labelledBy="modal-title">
      <h2 id="modal-title">Execution order</h2>
    </Modal>,
  );
  expect(screen.getByRole("dialog", { name: "Execution order" })).toBeInTheDocument();
  unmount();
  render(
    <Modal onClose={vi.fn()} label="Screenshot">
      <p>picture</p>
    </Modal>,
  );
  expect(screen.getByRole("dialog", { name: "Screenshot" })).toBeInTheDocument();
});

// A dialog's backdrop once covered the whole window, title bar included -
// the window could not be dragged, minimised or closed while any dialog was
// open. It starts below the title bar now, at the height the mounted title
// bar publishes. jsdom cannot drag a window or hit-test; what is held here
// is the value the backdrop is positioned by.
test("the backdrop starts below the title bar, at the height the title bar publishes", async () => {
  const { default: TitleBar } = await import("../../../src/components/TitleBar");
  const { TITLE_BAR_HEIGHT, TITLE_BAR_VAR } = await import("../../../src/lib/titleBar");
  const { unmount } = render(
    <>
      <TitleBar title="Test Case Manager" />
      <Modal onClose={() => {}} label="A dialog">
        content
      </Modal>
    </>,
  );
  expect(document.documentElement.style.getPropertyValue(TITLE_BAR_VAR)).toBe(TITLE_BAR_HEIGHT);
  const backdrop = screen.getByRole("dialog", { name: "A dialog" }).parentElement!;
  expect(getComputedStyle(backdrop).top).toBe(`var(${TITLE_BAR_VAR}, 0px)`);
  // The title bar itself is drawn at that same height.
  expect(screen.getByRole("banner").style.height).toBe(TITLE_BAR_HEIGHT);

  unmount();
  // No title bar, no offset: the fallback covers the whole viewport.
  expect(document.documentElement.style.getPropertyValue(TITLE_BAR_VAR)).toBe("");
});

// Nested Tab: every focus trap listens on the document, so the outer one
// used to act on Tabs pressed inside the inner dialog and drag focus to its
// own first control. Only the topmost trap may handle Tab.
function TabNest({ innerOpen }: { innerOpen: boolean }) {
  return (
    <Modal onClose={() => {}} className="outer">
      <button>Outer one</button>
      <button>Outer two</button>
      {innerOpen && (
        <Modal onClose={() => {}} className="inner">
          <button>Inner one</button>
          <button>Inner two</button>
          <button>Inner three</button>
        </Modal>
      )}
    </Modal>
  );
}

const tab = (shift = false) => fireEvent.keyDown(document.activeElement ?? document.body, { key: "Tab", shiftKey: shift });

function withLayout() {
  // jsdom lays nothing out, so no element has an offset parent and the trap
  // would see no stops at all.
  const spy = vi.spyOn(HTMLElement.prototype, "offsetParent", "get").mockImplementation(function (this: HTMLElement) {
    return this.parentElement;
  });
  return () => spy.mockRestore();
}

test("Tab and Shift+Tab inside a nested modal stay with the inner modal's own stops", () => {
  const restore = withLayout();
  try {
    // The inner dialog opens after the outer one, as it does in the app.
    const { rerender } = render(<TabNest innerOpen={false} />);
    rerender(<TabNest innerOpen />);
    const one = screen.getByRole("button", { name: "Inner one" });
    const two = screen.getByRole("button", { name: "Inner two" });
    const three = screen.getByRole("button", { name: "Inner three" });

    // A middle stop: nothing may intervene, and focus stays where it is.
    two.focus();
    expect(tab()).toBe(true);
    expect(tab(true)).toBe(true);
    expect(two).toHaveFocus();

    // The inner trap wraps at its own ends, never to the outer dialog.
    three.focus();
    expect(tab()).toBe(false);
    expect(one).toHaveFocus();
    expect(tab(true)).toBe(false);
    expect(three).toHaveFocus();
    expect(screen.getByRole("button", { name: "Outer one" })).not.toHaveFocus();
  } finally {
    restore();
  }
});

test("once the inner modal closes, the outer trap works again", () => {
  const restore = withLayout();
  try {
    const { rerender } = render(<TabNest innerOpen={false} />);
    rerender(<TabNest innerOpen />);
    rerender(<TabNest innerOpen={false} />);
    expect(screen.queryByRole("button", { name: "Inner one" })).toBeNull();
    const last = screen.getByRole("button", { name: "Outer two" });
    last.focus();
    expect(tab()).toBe(false);
    expect(screen.getByRole("button", { name: "Outer one" })).toHaveFocus();
  } finally {
    restore();
  }
});
