import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, test } from "vitest";
import { Tooltip, TooltipLayer, anchorBox, place } from "./tooltip";

/** DOMRect-shaped literal, so placement can be tested without a layout. */
const box = (left: number, top: number, width: number, height: number) => ({
  left,
  top,
  width,
  height,
  right: left + width,
  bottom: top + height,
});

/** The layer is mounted once per window in main.tsx; tests mount it the
 * same way, alongside whatever they are hovering. */
function withLayer(ui: ReactNode) {
  return render(
    <>
      {ui}
      <TooltipLayer />
    </>,
  );
}

const hover = (el: Element) => fireEvent.pointerOver(el, { bubbles: true });
const leave = (el: Element) =>
  fireEvent.pointerOut(el, { bubbles: true, relatedTarget: document.body });

test("any element with a title gets the app's tooltip", async () => {
  withLayer(<button title="Refresh work items">R</button>);
  const btn = screen.getByRole("button");

  hover(btn);
  // Not instant - a pointer passing through must not flash tooltips.
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Refresh work items");
});

/** Taking `title` off is what stops the OS bubble drawing over ours - and
 * it has to go back afterwards, or the element is left altered. */
test("the native title is suppressed while hovered and restored after", async () => {
  withLayer(<button title="Native text">R</button>);
  const btn = screen.getByRole("button");

  hover(btn);
  await waitFor(() => expect(btn).not.toHaveAttribute("title"));
  expect(btn).toHaveAttribute("data-tip-text", "Native text");

  leave(btn);
  await waitFor(() => expect(btn).toHaveAttribute("title", "Native text"));
  expect(btn).not.toHaveAttribute("data-tip-text");
});

/** The reason this is delegated rather than a wrapper: a wrapper element
 * inside the sidebar's flex rail stopped the collapsed rail resolving its
 * width. Nothing may be added around the trigger. */
test("it adds no element around the trigger", async () => {
  const { container } = withLayer(
    <div data-testid="row">
      <button title="Tip">Only child</button>
    </div>,
  );
  const row = container.querySelector('[data-testid="row"]')!;
  expect(row.children).toHaveLength(1);

  hover(screen.getByRole("button"));
  await screen.findByRole("tooltip");
  // Still one child: the bubble is portalled out of the layout entirely.
  expect(row.children).toHaveLength(1);
  expect(screen.getByRole("tooltip").parentElement).toBe(document.body);
});

test("an element with no title is ignored", async () => {
  withLayer(<button>Plain</button>);
  hover(screen.getByRole("button"));
  await new Promise((r) => setTimeout(r, 500));
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("an empty title is ignored", async () => {
  withLayer(<button title="   ">Blank</button>);
  hover(screen.getByRole("button"));
  await new Promise((r) => setTimeout(r, 500));
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("hovering a child of the trigger still finds the title", async () => {
  withLayer(
    <button title="Outer label">
      <span data-testid="icon">icon</span>
    </button>,
  );
  hover(screen.getByTestId("icon"));
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Outer label");
});

test("it closes on leave, on Escape and on a press", async () => {
  withLayer(<button title="Closes">X</button>);
  const btn = screen.getByRole("button");

  hover(btn);
  await screen.findByRole("tooltip");
  leave(btn);
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());

  hover(btn);
  await screen.findByRole("tooltip");
  fireEvent.keyDown(window, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());

  hover(btn);
  await screen.findByRole("tooltip");
  fireEvent.pointerDown(btn);
  await waitFor(() => expect(screen.queryByRole("tooltip")).toBeNull());
});

test("the explicit wrapper just labels its child", async () => {
  withLayer(
    <Tooltip label="Manual Entry" side="right">
      <button>M</button>
    </Tooltip>,
  );
  const btn = screen.getByRole("button");
  expect(btn).toHaveAttribute("title", "Manual Entry");
  expect(btn).toHaveAttribute("data-tip-side", "right");

  hover(btn);
  expect(await screen.findByRole("tooltip")).toHaveTextContent("Manual Entry");
});

/* Placement: an ordinary control anchors to itself, a big one to the mouse. */

test("a pointer-sized trigger is anchored to its own rect", () => {
  const btn = box(100, 100, 32, 32);
  const bubble = box(0, 0, 120, 24);
  // Nowhere near the middle of the button, and it changes nothing: the
  // rect is a fine anchor at this size, so every hover of the same
  // control puts the bubble in the same place.
  const atEdge = place(btn, bubble, "top", { x: 131, y: 130 });
  expect(atEdge).toEqual(place(btn, bubble, "top"));
  // Centred over the button, clear above it.
  expect(atEdge.left).toBe(100 + 16 - 60);
  expect(atEdge.top).toBe(100 - 24 - 8);
});

test("a wide trigger follows the pointer across, but still clears the card", () => {
  // A board card: wide, short. Only the horizontal anchor moves - the
  // bubble stays above the card rather than landing on top of it.
  const card = box(0, 400, 300, 80);
  const bubble = box(0, 0, 120, 24);
  const p = place(card, bubble, "top", { x: 260, y: 450 });
  expect(p.left).toBe(260 - 60);
  expect(p.top).toBe(400 - 24 - 8);
  // The old behaviour put it 110px away, over the middle of the card.
  expect(p.left).not.toBe(place(card, bubble, "top").left);
});

test("a trigger that is big both ways anchors fully to the pointer", () => {
  // A description block. There is no edge worth clearing, so the bubble
  // goes to the mouse on both axes.
  const desc = box(0, 100, 600, 400);
  const bubble = box(0, 0, 120, 24);
  const p = place(desc, bubble, "top", { x: 300, y: 420 });
  expect(p).toEqual({ top: 420 - 24 - 8, left: 300 - 60 });
});

test("a tall narrow trigger keeps its side edge", () => {
  // The collapsed board rail asks for `right`: the bubble should clear
  // the rail's edge, at the height being pointed at.
  const rail = box(0, 0, 36, 500);
  const bubble = box(0, 0, 100, 24);
  const p = place(rail, bubble, "right", { x: 18, y: 300 });
  expect(p.left).toBe(36 + 8);
  expect(p.top).toBe(300 - 12);
});

test("a pointer-anchored bubble is still clamped on-screen", () => {
  const desc = box(0, 0, 600, 400);
  const bubble = box(0, 0, 120, 24);
  // Pointing at the very top-left corner: without clamping this would be
  // placed at a negative top.
  const p = place(desc, bubble, "top", { x: 2, y: 2 });
  expect(p.top).toBe(8);
  expect(p.left).toBe(8);
});

test("with no pointer known, placement is unchanged", () => {
  const desc = box(0, 100, 600, 400);
  const bubble = box(0, 0, 120, 24);
  expect(anchorBox(desc, null)).toBe(desc);
  expect(place(desc, bubble, "top", null)).toEqual(place(desc, bubble, "top"));
});

test("a disabled wrapper sets no title at all", async () => {
  withLayer(
    <Tooltip label="Hidden" disabled>
      <button>Q</button>
    </Tooltip>,
  );
  const btn = screen.getByRole("button");
  expect(btn).not.toHaveAttribute("title");

  hover(btn);
  await new Promise((r) => setTimeout(r, 500));
  expect(screen.queryByRole("tooltip")).toBeNull();
});
