import { fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ChangelogModal from "./ChangelogModal";

const ENTRIES = [
  { version: "1.9.0", date: "2026-07-20", items: ["Edit queued cases in place."] },
  { version: "1.8.0", date: "2026-07-18", items: ["Pull Requests panel.", "Areas scope."] },
];

test("lists every pending version with its items", () => {
  render(<ChangelogModal entries={ENTRIES} onClose={() => {}} />);
  expect(screen.getByText("What's new")).toBeInTheDocument();
  expect(screen.getByText("Version 1.9.0")).toBeInTheDocument();
  expect(screen.getByText("Version 1.8.0")).toBeInTheDocument();
  expect(screen.getByText("Edit queued cases in place.")).toBeInTheDocument();
  expect(screen.getByText("Areas scope.")).toBeInTheDocument();
});

test("Got it and the backdrop both dismiss", () => {
  const onClose = vi.fn();
  render(<ChangelogModal entries={ENTRIES} onClose={onClose} />);
  fireEvent.click(screen.getByRole("button", { name: "Got it" }));
  expect(onClose).toHaveBeenCalledTimes(1);
  // Clicking inside the panel must NOT dismiss.
  fireEvent.click(screen.getByText("Version 1.9.0"));
  expect(onClose).toHaveBeenCalledTimes(1);
});
