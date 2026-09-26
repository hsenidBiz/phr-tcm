// The dev panel's Toasts row: one button per kind the app raises, so every
// look can be checked in each theme without hunting for the action that
// raises it.

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, expect, test, vi } from "vitest";

const toast = vi.hoisted(() => {
  const fn = Object.assign(vi.fn(), {
    success: vi.fn(),
    error: vi.fn(),
    info: vi.fn(),
    warning: vi.fn(),
    dismiss: vi.fn(),
  });
  return fn;
});
vi.mock("../lib/toast", () => ({ toast }));

import DevPanel from "./DevPanel";

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
});

function openPanel() {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DevPanel org="acme" project="Web" pbi={null} section="manual" workMode={false} onShowSignIn={() => {}} />
    </QueryClientProvider>,
  );
  fireEvent.click(screen.getByRole("button", { name: "Expand dev panel" }));
}

test("each toast button raises its own kind", () => {
  openPanel();
  for (const [label, fn] of [
    ["Success", toast.success],
    ["Error", toast.error],
    ["Info", toast.info],
    ["Warning", toast.warning],
    ["Plain", toast],
  ] as const) {
    fireEvent.click(screen.getByRole("button", { name: `Toast: ${label}` }));
    expect(fn, label).toHaveBeenCalledTimes(1);
  }
});

/// Picking a failure kind only chooses what "Fail next/every call" will
/// arm. In the red "on" fill a picked kind read as a failure that could not
/// be switched off, so red is kept for what is actually armed.
test("a picked failure kind is not shown as on; only arming is", () => {
  openPanel();
  const timeout = screen.getByRole("button", { name: "Timeout" });
  expect(timeout).toHaveAttribute("aria-pressed", "true");
  expect(timeout.className).not.toMatch(/bg-danger/);
  expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();

  const every = screen.getByRole("button", { name: "Fail every call" });
  fireEvent.click(every);
  expect(every.className).toMatch(/bg-danger/);
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  expect(screen.getByRole("button", { name: "Fail every call" }).className).not.toMatch(/bg-danger/);
});

test("the open panel is wide and two columns; the collapsed strip stays narrow", () => {
  render(
    <QueryClientProvider client={new QueryClient()}>
      <DevPanel org="acme" project="Web" pbi={null} section="manual" workMode={false} onShowSignIn={() => {}} />
    </QueryClientProvider>,
  );
  const strip = screen.getByLabelText("Drag the dev panel").parentElement!;
  expect(strip.className).toMatch(/\bw-72\b/);
  fireEvent.click(screen.getByRole("button", { name: "Expand dev panel" }));
  expect(strip.className).not.toMatch(/\bw-72\b/);
  expect(screen.getByText("Demo data").closest(".columns-2")).not.toBeNull();
});

test("the action toast carries an Undo, and the sticky one stays until dismissed", () => {
  openPanel();
  fireEvent.click(screen.getByRole("button", { name: "Toast: With action" }));
  expect(toast.success.mock.calls[0][1]?.action?.label).toBe("Undo");
  fireEvent.click(screen.getByRole("button", { name: "Toast: Sticky" }));
  expect(toast.info.mock.calls[0][1]?.duration).toBe(Infinity);
});
