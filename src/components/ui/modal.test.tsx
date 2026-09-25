// Nested modals must not fight over Escape. A modal has no idea another one
// is nested inside it - the bug this pins is Escape closing BOTH the inner
// dialog and whatever opened it, in one press.

import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { expect, test, vi } from "vitest";
import { Modal } from "./modal";

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
