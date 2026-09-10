import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { raise, resetForTests } from "../lib/notifications";
import NotificationBell from "./NotificationBell";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(() => Promise.resolve()) }));

beforeEach(() => {
  localStorage.clear();
  resetForTests();
});
afterEach(() => {
  localStorage.clear();
  resetForTests();
});

function seed() {
  raise("acme", [
    { id: "pr-conflict:Web:10", kind: "pr-conflict", title: "PR #10 has merge conflicts", body: "Fix login (Web)", href: "https://x/10" },
    { id: "assigned:501", kind: "assigned", title: "Task #501 assigned to you", body: "Wire the login flow" },
  ]);
}

/// The badge counts what you have not seen; opening the panel is seeing
/// it, so the badge goes while the items stay.
test("the badge shows the unread count and opening the panel clears it", () => {
  seed();
  render(<NotificationBell org="acme" />);

  const bell = screen.getByRole("button", { name: "Notifications, 2 unread" });
  expect(bell).toHaveTextContent("2");
  fireEvent.click(bell);

  expect(screen.getByRole("dialog", { name: "Notifications" })).toBeInTheDocument();
  expect(screen.getByText("PR #10 has merge conflicts")).toBeInTheDocument();
  expect(screen.getByText("Task #501 assigned to you")).toBeInTheDocument();
  // Read now: the badge is gone, the items are not.
  expect(screen.getByRole("button", { name: "Notifications" })).not.toHaveTextContent("2");
});

test("X dismisses one item; Clear all empties the panel", () => {
  seed();
  render(<NotificationBell org="acme" />);
  fireEvent.click(screen.getByRole("button", { name: /Notifications/ }));

  fireEvent.click(screen.getByRole("button", { name: "Dismiss: PR #10 has merge conflicts" }));
  expect(screen.queryByText("PR #10 has merge conflicts")).not.toBeInTheDocument();
  expect(screen.getByText("Task #501 assigned to you")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Clear all" }));
  expect(screen.getByText(/all caught up/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Clear all" })).not.toBeInTheDocument();
});

/// A new notification raised while the panel is closed lights the badge
/// again - the store publishes, the bell repaints.
test("a notification raised later lights the badge again", async () => {
  seed();
  render(<NotificationBell org="acme" />);
  fireEvent.click(screen.getByRole("button", { name: /Notifications/ }));
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

  // Raised from outside React (a poll, an event): the store publishes and
  // the bell repaints on the next flush.
  raise("acme", [{ id: "pr-review:Web:20", kind: "pr-review", title: "PR #20 is waiting for your review", body: "" }]);
  expect(await screen.findByRole("button", { name: "Notifications, 1 unread" })).toBeInTheDocument();
});

test("renders nothing without an organisation", () => {
  const { container } = render(<NotificationBell org="" />);
  expect(container).toBeEmptyDOMElement();
});
