import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { raise, resetForTests } from "../lib/notifications";
import { openUrl } from "@tauri-apps/plugin-opener";
import NotificationBell from "./NotificationBell";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(() => Promise.resolve()) }));

beforeEach(() => {
  localStorage.clear();
  resetForTests();
  vi.mocked(openUrl).mockClear();
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

/// The title is the way IN - the app opens the thing itself - and the
/// browser is still there, one small button along.
test("a notification with a target opens in the app and closes the panel; the browser stays one click away", () => {
  raise("acme", [
    {
      id: "assigned:501",
      kind: "assigned",
      title: "Task #501 assigned to you",
      body: "Wire the login flow",
      href: "https://x/501",
      target: { kind: "work-item", id: 501, project: "Web" },
    },
  ]);
  const opened: unknown[] = [];
  render(<NotificationBell org="acme" onOpen={(t) => opened.push(t)} />);
  fireEvent.click(screen.getByRole("button", { name: "Notifications, 1 unread" }));
  fireEvent.click(screen.getByRole("button", { name: "Task #501 assigned to you" }));
  expect(opened).toEqual([{ kind: "work-item", id: 501, project: "Web" }]);
  expect(openUrl).not.toHaveBeenCalled();
  expect(screen.queryByRole("dialog", { name: "Notifications" })).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Notifications" }));
  const toBrowser = screen.getByRole("button", { name: "Open in Azure DevOps: Task #501 assigned to you" });
  // It sits in the kind-and-time line, beside the time - not on the title's
  // line, where it read as part of the title.
  expect(toBrowser.parentElement).toHaveTextContent("Assigned");
  expect(toBrowser.parentElement).not.toHaveTextContent("Task #501 assigned to you");
  fireEvent.click(toBrowser);
  expect(openUrl).toHaveBeenCalledWith("https://x/501");
});

/// Entries saved before targets existed keep working exactly as they did.
test("a stored notification without a target still opens the browser", () => {
  seed(); // pr-conflict:Web:10 has href only
  render(
    <NotificationBell
      org="acme"
      onOpen={() => {
        throw new Error("must not be called");
      }}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Notifications, 2 unread" }));
  fireEvent.click(screen.getByRole("button", { name: "PR #10 has merge conflicts" }));
  expect(openUrl).toHaveBeenCalledWith("https://x/10");
});

/// A target saved by a build between the field's addition and the project
/// guard has no project on it - it must not be treated as an in-app
/// target (there is nowhere safe to navigate), only as the browser link.
test("a stored target with no project falls back to the browser, not the app", () => {
  raise("acme", [
    {
      id: "assigned:501",
      kind: "assigned",
      title: "Task #501 assigned to you",
      body: "Wire the login flow",
      href: "https://x/501",
      target: { kind: "work-item", id: 501 } as unknown as { kind: "work-item"; id: number; project: string },
    },
  ]);
  render(
    <NotificationBell
      org="acme"
      onOpen={() => {
        throw new Error("must not be called");
      }}
    />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Notifications, 1 unread" }));
  fireEvent.click(screen.getByRole("button", { name: "Task #501 assigned to you" }));
  expect(openUrl).toHaveBeenCalledWith("https://x/501");
});

test("renders nothing without an organisation", () => {
  const { container } = render(<NotificationBell org="" />);
  expect(container).toBeEmptyDOMElement();
});

test("a mention wears the Mention label and opens its work item in the app", () => {
  raise("acme", [
    {
      id: "mention:wi:41:7",
      kind: "mention",
      title: "Sam mentioned you on Product Backlog Item #41",
      body: "@Avin can you check this?",
      href: "https://x/41",
      target: { kind: "work-item", id: 41, project: "Web" },
    },
  ]);
  const opened: unknown[] = [];
  render(<NotificationBell org="acme" onOpen={(t) => opened.push(t)} />);
  fireEvent.click(screen.getByRole("button", { name: "Notifications, 1 unread" }));
  expect(screen.getByText("Mention")).toHaveClass("text-danger");
  expect(screen.getByText("@Avin can you check this?")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Sam mentioned you on Product Backlog Item #41" }));
  expect(opened).toEqual([{ kind: "work-item", id: 41, project: "Web" }]);
});

test("a PR mention opens its pull request in the app", () => {
  raise("acme", [
    {
      id: "mention:pr:web:12:3:9",
      kind: "mention",
      title: "Sam mentioned you on PR #12",
      body: "@you please look",
      href: "https://x/pr/12",
      target: { kind: "pr", repo: "web", id: 12, project: "Web" },
    },
  ]);
  const opened: unknown[] = [];
  render(<NotificationBell org="acme" onOpen={(t) => opened.push(t)} />);
  fireEvent.click(screen.getByRole("button", { name: "Notifications, 1 unread" }));
  fireEvent.click(screen.getByRole("button", { name: "Sam mentioned you on PR #12" }));
  expect(opened).toEqual([{ kind: "pr", repo: "web", id: 12, project: "Web" }]);
});
