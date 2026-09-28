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

test("duration Infinity keeps a toast until it is dismissed, as it did under sonner", () => {
  vi.useFakeTimers();
  render(<Toaster />);
  act(() => {
    toast.error("Stays", { duration: Infinity });
  });
  act(() => {
    vi.advanceTimersByTime(60_000);
  });
  expect(screen.getByText("Stays").closest("[data-ending-style]")).toBeNull();
});

test("a toast's kind is on its own element, directly in the viewport - the hook xiod-theme.css tints it by", async () => {
  render(<Toaster />);
  act(() => {
    toast.error("Tinted");
  });
  const root = (await screen.findByText("Tinted")).closest('[data-slot="toast-viewport"] > *');
  expect(root?.getAttribute("data-type")).toBe("error");
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

const leaving = (text: string) => {
  const el = screen.queryByText(text);
  return el === null || el.closest("[data-ending-style]") !== null;
};

/// Base UI stops every toast's clock while the window is not the focused
/// one (ToastViewport's window blur handler), so a toast that arrived with
/// the app beside a browser, or out in the tray, never left. jsdom does not
/// reproduce that pause, so the tests below pin the fix instead: the clock
/// is this module's own - it caps, and it waits only for the pointer.
test("no toast stays longer than ten seconds, whatever it asked for", () => {
  vi.useFakeTimers();
  render(<Toaster />);
  act(() => {
    toast.error("Asked for thirty", { duration: 30_000 });
  });
  act(() => {
    vi.advanceTimersByTime(9_900);
  });
  expect(leaving("Asked for thirty")).toBe(false);
  act(() => {
    vi.advanceTimersByTime(300);
  });
  expect(leaving("Asked for thirty")).toBe(true);
});

test("pointing at the toasts holds them until the pointer leaves", () => {
  vi.useFakeTimers();
  render(<Toaster />);
  act(() => {
    toast.success("Being read");
  });
  const title = screen.getByText("Being read");
  act(() => {
    vi.advanceTimersByTime(2_000);
    fireEvent.pointerOver(title);
    vi.advanceTimersByTime(20_000);
  });
  expect(leaving("Being read")).toBe(false);
  act(() => {
    fireEvent.pointerOut(title, { relatedTarget: document.body });
    vi.advanceTimersByTime(1_900);
  });
  // Two seconds were left when the pointer arrived.
  expect(leaving("Being read")).toBe(false);
  act(() => {
    vi.advanceTimersByTime(200);
  });
  expect(leaving("Being read")).toBe(true);
});
