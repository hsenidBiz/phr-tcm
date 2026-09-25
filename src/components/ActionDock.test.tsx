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

test("in place renders children with floating=false", () => {
  render(
    <ActionDock label="Test actions">
      {(floating) => <button>{floating ? "Floating label" : "In place label"}</button>}
    </ActionDock>,
  );
  expect(screen.getByRole("button", { name: "In place label" })).toBeInTheDocument();
});

test("off screen shows a named region in document.body without aria-hidden", () => {
  onScreen = false;
  render(
    <ActionDock label="Test actions">
      {(floating) => <button>{floating ? "Floating label" : "In place label"}</button>}
    </ActionDock>,
  );
  const region = screen.getByRole("region", { name: "Test actions" });
  expect(region.parentElement).toBe(document.body);
  expect(within(region).getByText("Floating label")).toBeInTheDocument();
  expect(region).not.toHaveAttribute("aria-hidden");
});

test("on screen the region is aria-hidden", () => {
  onScreen = true;
  render(
    <ActionDock label="Test actions">
      {(floating) => <button>{floating ? "Floating label" : "In place label"}</button>}
    </ActionDock>,
  );
  const region = document.querySelector("[data-sticky-action]") as HTMLElement;
  expect(region).toHaveAttribute("aria-hidden", "true");
});

test("active=false hides the region even off screen", () => {
  onScreen = false;
  render(
    <ActionDock label="Test actions" active={false}>
      {(floating) => <button>{floating ? "Floating label" : "In place label"}</button>}
    </ActionDock>,
  );
  const region = document.querySelector("[data-sticky-action]") as HTMLElement;
  expect(region).toHaveAttribute("aria-hidden", "true");
});

test("stack offsets the floating region upward", () => {
  onScreen = false;
  render(
    <ActionDock label="Test actions" stack={2}>
      {() => <button>Action</button>}
    </ActionDock>,
  );
  const region = document.querySelector("[data-sticky-action]") as HTMLElement;
  expect(region.style.bottom).toBe("8.5rem");
});

test("the in-place row is right-aligned by default", () => {
  const { container } = render(
    <ActionDock label="Test actions">{() => <button>Action</button>}</ActionDock>,
  );
  // The in-place row is the only thing ActionDock renders into `container`;
  // the floating copy is portalled straight to document.body.
  expect(container.firstElementChild?.className).toContain("justify-end");
});
