import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import GeneralComments from "./GeneralComments";
import type { WatchedFile } from "../lib/fileSync";

afterEach(() => {
  clearMocks();
  vi.useRealTimers();
});

const watch = (path: string, comment = ""): WatchedFile => ({
  path,
  stamp: "s",
  snapshot: [],
  comment,
});

test("nothing is rendered when no file is being watched", () => {
  const { container } = render(<GeneralComments watches={[]} onSaved={() => {}} />);
  expect(container).toBeEmptyDOMElement();
});

test("collapsed by default, one box per file once opened", () => {
  render(
    <GeneralComments
      watches={[watch("C:/w/login.json", "Spec 3.2 is ambiguous"), watch("C:/w/pay.json")]}
      onSaved={() => {}}
    />,
  );
  const toggle = screen.getByRole("button", { name: /General comments/ });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByLabelText("General comments for login.json")).not.toBeInTheDocument();

  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute("aria-expanded", "true");
  // Each file keeps its own text - they are separate notes, not one shared.
  expect(screen.getByLabelText("General comments for login.json")).toHaveValue(
    "Spec 3.2 is ambiguous",
  );
  expect(screen.getByLabelText("General comments for pay.json")).toHaveValue("");
});

test("typing autosaves into that file and reports the new fingerprint", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  const calls: unknown[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "save_general_comment") {
      calls.push(args);
      return "stamp-2";
    }
  });
  const onSaved = vi.fn();
  render(<GeneralComments watches={[watch("C:/w/login.json")]} onSaved={onSaved} />);
  fireEvent.click(screen.getByRole("button", { name: /General comments/ }));

  const box = screen.getByLabelText("General comments for login.json");
  fireEvent.change(box, { target: { value: "Waiting on Dev" } });
  // Debounced: the disk is not written a character at a time.
  expect(calls).toHaveLength(0);
  expect(screen.getByText("Saving…")).toBeInTheDocument();

  await vi.advanceTimersByTimeAsync(700);
  await waitFor(() => expect(screen.getByText("Saved ✓")).toBeInTheDocument());
  expect(calls).toEqual([{ path: "C:/w/login.json", text: "Waiting on Dev" }]);
  expect(onSaved).toHaveBeenCalledWith("C:/w/login.json", "Waiting on Dev", "stamp-2");
});

/// A file can move, be locked, or stop being valid JSON while it is open.
/// Saying "Saved" then would be worse than saying nothing.
test("a failed save says so and does not claim a fingerprint", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  // A Rust `Err(String)` crosses the bridge as a bare string, not an Error.
  mockIPC((cmd) => {
    if (cmd === "save_general_comment") throw "could not read the file: not found";
  });
  const onSaved = vi.fn();
  render(<GeneralComments watches={[watch("C:/w/gone.json")]} onSaved={onSaved} />);
  fireEvent.click(screen.getByRole("button", { name: /General comments/ }));
  fireEvent.change(screen.getByLabelText("General comments for gone.json"), {
    target: { value: "x" },
  });

  await vi.advanceTimersByTimeAsync(700);
  await waitFor(() => expect(screen.getByText(/Not saved/)).toBeInTheDocument());
  expect(screen.getByText(/not found/)).toBeInTheDocument();
  expect(onSaved).not.toHaveBeenCalled();
  // The text stays on screen - a failed write must not eat what was typed.
  expect(screen.getByLabelText("General comments for gone.json")).toHaveValue("x");
});
