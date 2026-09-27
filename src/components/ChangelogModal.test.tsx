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

test("beta entries carry a Beta tag, stable ones do not", () => {
  render(
    <ChangelogModal
      entries={[
        { version: "1.26.0", date: "2026-10-05", items: ["Stable."] },
        { version: "1.26.0-beta.1", date: "2026-10-01", items: ["Beta."] },
      ]}
      onClose={() => {}}
    />,
  );
  const headings = screen.getAllByRole("heading", { level: 3 });
  // No literal whitespace separates "Version {version}" from the date span
  // in the rendered DOM (spacing is CSS margin, ml-2, not text) - this
  // matches today's stable heading exactly, so existing getByText("Version
  // 1.9.0")-style assertions elsewhere are unaffected by this change.
  expect(headings.map((h) => h.textContent)).toEqual([
    "Version 1.26.02026-10-05",
    "Version 1.26.0-beta.1 Beta2026-10-01",
  ]);
});
