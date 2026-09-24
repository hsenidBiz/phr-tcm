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

test("the action toast carries an Undo, and the sticky one stays until dismissed", () => {
  openPanel();
  fireEvent.click(screen.getByRole("button", { name: "Toast: With action" }));
  expect(toast.success.mock.calls[0][1]?.action?.label).toBe("Undo");
  fireEvent.click(screen.getByRole("button", { name: "Toast: Sticky" }));
  expect(toast.info.mock.calls[0][1]?.duration).toBe(Infinity);
});
