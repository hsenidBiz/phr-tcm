import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { AssignedItem } from "../../src/bindings";
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { alertListText, announce, appIsInView, summarize } from "../../src/lib/assignedAlerts";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { toast } from "../../src/lib/toast";

vi.mock("../../src/lib/toast", () => ({ toast: { info: vi.fn() } }));
vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(),
  requestPermission: vi.fn(),
  sendNotification: vi.fn(),
}));

const item = (id: number, title: string, type = "Task"): AssignedItem => ({
  id,
  title,
  work_item_type: type,
  state: "Active",
  project: "Web",
});

beforeEach(() => {
  vi.mocked(toast.info).mockClear();
  vi.mocked(isPermissionGranted).mockReset().mockResolvedValue(true);
  vi.mocked(requestPermission).mockReset().mockResolvedValue("granted");
  vi.mocked(sendNotification).mockClear();
});
afterEach(() => vi.restoreAllMocks());

test("a single assignment names the item", () => {
  const { title, body } = summarize([item(143783, "Participants - inconsistencies", "Bug")]);
  expect(title).toBe("Bug #143783 assigned to you");
  expect(body).toEqual({ items: [{ text: "Participants - inconsistencies" }], more: 0 });
  expect(alertListText(body)).toBe("Participants - inconsistencies");
});

test("several are counted, with the first few listed", () => {
  const { title, body } = summarize([item(1, "One"), item(2, "Two"), item(3, "Three")]);
  expect(title).toBe("3 work items assigned to you");
  expect(body.items.map((i) => i.id)).toEqual([1, 2, 3]);
  expect(body.more).toBe(0);
});

test("a long list is truncated rather than dumped", () => {
  const many = Array.from({ length: 4 }, (_, i) => item(i + 1, `Item ${i + 1}`));
  const { body } = summarize(many);
  expect(body.items).toHaveLength(3);
  expect(body.more).toBe(1);
});

test("the toast lists each item as its own block: id, type, clamped title, then 'and N more'", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(document, "hidden", "get").mockReturnValue(false);
  const long = "A very long work item title ".repeat(12);
  const { title, body } = summarize([
    item(170597, long, "Bug"),
    item(170598, "Second"),
    item(170599, "Third"),
    item(170600, "Fourth"),
  ]);
  announce(title, body);
  const opts = vi.mocked(toast.info).mock.calls[0][1] as { description: ReactNode };
  render(<div>{opts.description}</div>);
  const blocks = screen.getAllByTestId("alert-item");
  expect(blocks).toHaveLength(3);
  expect(blocks[0]).toHaveTextContent("#170597");
  expect(blocks[0]).toHaveTextContent("Bug");
  expect(blocks[0]).toHaveTextContent("A very long work item title");
  expect(blocks[1]).toHaveTextContent("#170598");
  expect(blocks[2]).toHaveTextContent("Third");
  expect(blocks[1].className).toContain("border-border");
  expect(blocks[0].querySelector(".line-clamp-2")).not.toBeNull();
  expect(screen.getByText("#170597").className).toContain("font-mono");
  expect(screen.getByText("and 1 more")).toBeInTheDocument();
});

test("a single item's toast keeps its title, clamped to two lines", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(document, "hidden", "get").mockReturnValue(false);
  const { title, body } = summarize([item(5, "Only one")]);
  announce(title, body);
  const opts = vi.mocked(toast.info).mock.calls[0][1] as { description: ReactNode };
  render(<div>{opts.description}</div>);
  expect(screen.getAllByTestId("alert-item")).toHaveLength(1);
  expect(screen.getByText("Only one").className).toContain("line-clamp-2");
  expect(screen.queryByText(/more/)).toBeNull();
});

test("the OS text is one short line per item, then '…and N more'", async () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
  const long = "x".repeat(200);
  const { title, body } = summarize([item(1, long), item(2, "Two"), item(3, "Three"), item(4, "Four"), item(5, "Five")]);
  const text = alertListText(body);
  const lines = text.split("\n");
  expect(lines).toHaveLength(4);
  expect(lines.slice(0, 3).every((l) => l.length <= 70)).toBe(true);
  expect(lines[0].endsWith("…")).toBe(true);
  expect(lines[1]).toBe("#2 Two");
  expect(lines[3]).toBe("…and 2 more");
  announce(title, body);
  await vi.waitFor(() => expect(sendNotification).toHaveBeenCalledWith({ title, body: text }));
  expect(toast.info).not.toHaveBeenCalled();
});

/** A toast behind another window is the same as no notification, so the
 * check has to be focus, not just visibility. */
test("the app counts as in view only when focused AND not hidden", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(document, "hidden", "get").mockReturnValue(false);
  expect(appIsInView()).toBe(true);

  vi.spyOn(document, "hasFocus").mockReturnValue(false);
  expect(appIsInView()).toBe(false);

  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  expect(appIsInView()).toBe(false);
});

/// `announce` is the one place a new assignment and a mention both reach
/// the user through, so both are covered by testing it directly, once,
/// here.
test("announce is a toast when the app is in view", () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(document, "hidden", "get").mockReturnValue(false);
  announce("Bug #1 assigned to you", "Fix the thing");
  expect(toast.info).toHaveBeenCalledWith("Bug #1 assigned to you", { description: "Fix the thing", duration: 10_000 });
  expect(sendNotification).not.toHaveBeenCalled();
});

test("announce reaches the OS, not a toast, when the app is not in view", async () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
  announce("Bug #1 assigned to you", "Fix the thing");
  await vi.waitFor(() =>
    expect(sendNotification).toHaveBeenCalledWith({ title: "Bug #1 assigned to you", body: "Fix the thing" }),
  );
  expect(toast.info).not.toHaveBeenCalled();
});

test("announce falls back to a toast when the OS notification is refused", async () => {
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
  vi.mocked(isPermissionGranted).mockResolvedValue(false);
  vi.mocked(requestPermission).mockResolvedValue("denied");
  announce("Bug #1 assigned to you", "Fix the thing");
  await vi.waitFor(() =>
    expect(toast.info).toHaveBeenCalledWith("Bug #1 assigned to you", { description: "Fix the thing", duration: 10_000 }),
  );
  expect(sendNotification).not.toHaveBeenCalled();
});

test("a lone item's OS text keeps its whole title; only a list clips each line", () => {
  const long = "A".repeat(120);
  expect(alertListText({ items: [{ text: long }], more: 0 })).toBe(long);
  const several = alertListText({ items: [{ id: 1, text: long }, { id: 2, text: "short" }], more: 0 });
  expect(several.split("\n")[0].length).toBeLessThanOrEqual(70);
});
