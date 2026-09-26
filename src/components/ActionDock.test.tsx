/**
 * ActionDock is the one place that builds "row in place, floating copy once
 * scrolled past" - useOnScreen is mocked so each test drives the on/off
 * screen answer directly instead of fighting jsdom's lack of layout.
 */
import { render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import ActionDock from "./ActionDock";

let onScreen = true;
vi.mock("../hooks/useOnScreen", () => ({ useOnScreen: () => [() => {}, onScreen] }));

afterEach(() => {
  onScreen = true;
});

/** The floating copy is always aria-hidden (fix round 1, Important 2): it
 * duplicates a control that never actually leaves the accessibility tree
 * (the in-place row is only scrolled off screen), so `getByRole` cannot
 * find it - even shown, it must stay invisible to a screen reader. Tests
 * find it by `[data-sticky-action]` instead, the way its consumers do. */
const floating = () => document.querySelector("[data-sticky-action]") as HTMLElement;

test("in place renders children with floating=false", () => {
  render(
    <ActionDock label="Test actions">
      {(isFloating) => <button>{isFloating ? "Floating label" : "In place label"}</button>}
    </ActionDock>,
  );
  expect(screen.getByRole("button", { name: "In place label" })).toBeInTheDocument();
});

test("the floating copy is always aria-hidden, on screen or off", () => {
  onScreen = true;
  const { rerender } = render(
    <ActionDock label="Test actions">{(isFloating) => <button>{isFloating ? "F" : "P"}</button>}</ActionDock>,
  );
  expect(floating()).toHaveAttribute("aria-hidden", "true");

  onScreen = false;
  rerender(
    <ActionDock label="Test actions">{(isFloating) => <button>{isFloating ? "F" : "P"}</button>}</ActionDock>,
  );
  expect(floating()).toHaveAttribute("aria-hidden", "true");
});

test("off screen: shown, portalled to document.body, holds the floating children, not inert", () => {
  onScreen = false;
  render(
    <ActionDock label="Test actions">
      {(isFloating) => <button>{isFloating ? "Floating label" : "In place label"}</button>}
    </ActionDock>,
  );
  const el = floating();
  expect(el.parentElement).toBe(document.body);
  expect(el.getAttribute("aria-label")).toBe("Test actions");
  expect(within(el).getByText("Floating label")).toBeInTheDocument();
  expect(el.className).toContain("opacity-100");
  expect(el).not.toHaveAttribute("inert");
});

test("on screen: hidden and inert", () => {
  onScreen = true;
  render(
    <ActionDock label="Test actions">
      {(isFloating) => <button>{isFloating ? "Floating label" : "In place label"}</button>}
    </ActionDock>,
  );
  const el = floating();
  expect(el.className).toContain("opacity-0");
  expect(el).toHaveAttribute("inert", "");
});

test("active=false hides and inerts the region even off screen", () => {
  onScreen = false;
  render(
    <ActionDock label="Test actions" active={false}>
      {(isFloating) => <button>{isFloating ? "Floating label" : "In place label"}</button>}
    </ActionDock>,
  );
  const el = floating();
  expect(el.className).toContain("opacity-0");
  expect(el).toHaveAttribute("inert", "");
});

test("stack offsets the floating region upward", () => {
  onScreen = false;
  render(
    <ActionDock label="Test actions" stack={2}>
      {() => <button>Action</button>}
    </ActionDock>,
  );
  expect(floating().style.bottom).toBe("8.5rem");
});

test("surface wraps the floating copy in the pill; omitted by default", () => {
  onScreen = false;
  const { rerender } = render(
    <ActionDock label="Test actions">{() => <button>Action</button>}</ActionDock>,
  );
  expect(floating().className).not.toContain("rounded-full");
  expect(floating().className).not.toContain("bg-surface");

  rerender(
    <ActionDock label="Test actions" surface>
      {() => <button>Action</button>}
    </ActionDock>,
  );
  expect(floating().className).toContain("rounded-full");
  expect(floating().className).toContain("border-border");
  expect(floating().className).toContain("bg-surface");
  expect(floating().className).toContain("shadow-2xl");
  expect(floating().className).toContain("p-2.5");
});

test("the in-place row is right-aligned by default", () => {
  const { container } = render(
    <ActionDock label="Test actions">{() => <button>Action</button>}</ActionDock>,
  );
  // The in-place row is the only thing ActionDock renders into `container`;
  // the floating copy is portalled straight to document.body.
  expect(container.firstElementChild?.className).toContain("justify-end");
});
