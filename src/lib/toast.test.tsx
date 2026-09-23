// The app's toasts: one local module every screen calls (lib/toast.ts),
// drawn by XiodUI's toast host (components/ui/toaster.tsx). The options the
// call sites were written against - sonner's - still mean what they meant.

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Toaster } from "../components/ui/toaster";
import { toast } from "./toast";

afterEach(() => {
  vi.useRealTimers();
});

test.each([
  ["success", toast.success],
  ["error", toast.error],
  ["info", toast.info],
  ["warning", toast.warning],
] as const)("toast.%s shows its message and description, marked with its kind", async (kind, show) => {
  render(<Toaster />);
  act(() => {
    show(`A ${kind} message`, { description: "More detail" });
  });
  const title = await screen.findByText(`A ${kind} message`);
  expect(screen.getByText("More detail")).toBeInTheDocument();
  expect(title.closest("[data-type]")).toHaveAttribute("data-type", kind);
  // Dragging a toast away must not highlight its text.
  expect(title.closest(".select-none")).not.toBeNull();
});

test("a plain toast() carries no kind", async () => {
  render(<Toaster />);
  act(() => {
    toast("Just so you know");
  });
  const title = await screen.findByText("Just so you know");
  expect(title.closest("[data-type]")).toBeNull();
});

test("an action runs once, and takes only its own toast away", async () => {
  const undo = vi.fn();
  render(<Toaster />);
  act(() => {
    toast.success("Saved elsewhere");
    toast.info("Changes discarded.", { action: { label: "Undo", onClick: undo } });
  });
  fireEvent.click(await screen.findByRole("button", { name: "Undo" }));
  expect(undo).toHaveBeenCalledTimes(1);
  await waitFor(() => expect(screen.queryByText("Changes discarded.")).not.toBeInTheDocument());
  expect(screen.getByText("Saved elsewhere")).toBeInTheDocument();
});

test("duration is how long a toast stays", async () => {
  render(<Toaster />);
  act(() => {
    toast.warning("Gone soon", { duration: 50 });
  });
  expect(await screen.findByText("Gone soon")).toBeInTheDocument();
  await waitFor(() => expect(screen.queryByText("Gone soon")).not.toBeInTheDocument(), { timeout: 2000 });
});

test("without a duration, a toast stays four seconds - sonner's default, which every message was written for", () => {
  vi.useFakeTimers();
  render(<Toaster />);
  act(() => {
    toast.success("Four seconds");
  });
  act(() => {
    vi.advanceTimersByTime(3_900);
  });
  expect(screen.getByText("Four seconds").closest("[data-ending-style]")).toBeNull();
  act(() => {
    vi.advanceTimersByTime(1_000);
  });
  const left = screen.queryByText("Four seconds");
  expect(left === null || left.closest("[data-ending-style]") !== null).toBe(true);
});

test("dismiss() with no id clears every toast", async () => {
  render(<Toaster />);
  act(() => {
    toast.info("One");
    toast.info("Two");
  });
  await screen.findByText("Two");
  act(() => {
    toast.dismiss();
  });
  await waitFor(() => {
    expect(screen.queryByText("One")).not.toBeInTheDocument();
    expect(screen.queryByText("Two")).not.toBeInTheDocument();
  });
});
