import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { expect, test } from "vitest";
import { Tooltip, TooltipLayer } from "./tooltip";

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
