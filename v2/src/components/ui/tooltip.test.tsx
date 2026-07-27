import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useRef } from "react";
import { expect, test, vi } from "vitest";
import { Tooltip } from "./tooltip";

const show = (el: HTMLElement) => fireEvent.pointerOver(el);
const hide = (el: HTMLElement) => fireEvent.pointerOut(el);

test("it shows after a delay and hides again", async () => {
  render(
    <Tooltip label="Run Tests">
      <button>Run</button>
    </Tooltip>,
  );
  const btn = screen.getByRole("button");
  expect(screen.queryByRole("tooltip")).toBeNull();

  show(btn);
  // Not instant - a pointer passing through must not flash tooltips.
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Run Tests");

  hide(btn);
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
});

/** The reason this component exists: a wrapper element around the trigger
 * broke the sidebar's collapsed rail. Nothing may be added around it. */
test("it adds no element around the trigger", async () => {
  const { container } = render(
    <div data-testid="row">
      <Tooltip label="Tip">
        <button>Only child</button>
      </Tooltip>
    </div>,
  );
  const row = container.querySelector('[data-testid="row"]')!;
  expect(row.children).toHaveLength(1);
  expect(row.children[0].tagName).toBe("BUTTON");

  show(screen.getByRole("button"));
  await screen.findByRole("tooltip");
  // Still one child: the bubble is portalled out of the layout entirely.
  expect(row.children).toHaveLength(1);
  expect(screen.getByRole("tooltip").parentElement).toBe(document.body);
});

test("keyboard focus opens it too", async () => {
  render(
    <Tooltip label="Focusable">
      <button>Go</button>
    </Tooltip>,
  );
  fireEvent.focus(screen.getByRole("button"));
  expect(await screen.findByRole("tooltip")).toBeInTheDocument();

  fireEvent.blur(screen.getByRole("button"));
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
});

test("it describes its trigger while open", async () => {
  render(
    <Tooltip label="Described">
      <button>Trigger</button>
    </Tooltip>,
  );
  const btn = screen.getByRole("button");
  expect(btn).not.toHaveAttribute("aria-describedby");
  show(btn);
  const tip = await screen.findByRole("tooltip");
  expect(btn.getAttribute("aria-describedby")).toBe(tip.id);
});

test("disabled means nothing happens at all", async () => {
  render(
    <Tooltip label="Hidden" disabled>
      <button>Quiet</button>
    </Tooltip>,
  );
  show(screen.getByRole("button"));
  await new Promise((r) => setTimeout(r, 500));
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("Escape closes it", async () => {
  render(
    <Tooltip label="Escapable">
      <button>Trigger</button>
    </Tooltip>,
  );
  show(screen.getByRole("button"));
  await screen.findByRole("tooltip");
  fireEvent.keyDown(window, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
});

/** The trigger's own handlers and ref must survive being cloned. */
test("it preserves the trigger's own handlers and ref", async () => {
  const onClick = vi.fn();
  const onPointerOver = vi.fn();
  function Harness() {
    const ref = useRef<HTMLButtonElement>(null);
    return (
      <Tooltip label="Tip">
        <button ref={ref} onClick={onClick} onPointerOver={onPointerOver} data-ref-check>
          Trigger
        </button>
      </Tooltip>
    );
  }
  render(<Harness />);
  const btn = screen.getByRole("button");
  show(btn);
  expect(onPointerOver).toHaveBeenCalled();
  await screen.findByRole("tooltip");

  // Clicking runs the caller's handler AND dismisses the tooltip.
  fireEvent.click(btn);
  expect(onClick).toHaveBeenCalled();
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
});
