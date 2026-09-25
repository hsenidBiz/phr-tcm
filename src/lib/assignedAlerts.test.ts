import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { AssignedItem } from "../bindings";
import { announce, appIsInView, summarize } from "./assignedAlerts";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { toast } from "./toast";

vi.mock("./toast", () => ({ toast: { info: vi.fn() } }));
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
  expect(body).toBe("Participants - inconsistencies");
});

test("several are counted, with the first few listed", () => {
  const { title, body } = summarize([item(1, "One"), item(2, "Two"), item(3, "Three")]);
  expect(title).toBe("3 work items assigned to you");
  expect(body).toContain("#1 One");
  expect(body).toContain("#3 Three");
  expect(body).not.toContain("more");
});

test("a long list is truncated rather than dumped", () => {
  const many = Array.from({ length: 9 }, (_, i) => item(i + 1, `Item ${i + 1}`));
  const { body } = summarize(many);
  expect(body).toContain("…and 6 more");
  expect(body.split("\n")).toHaveLength(4);
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

/// Fix round 1, Important #1: `announce` is the one place a new
/// assignment and a mention both reach the user through, so both are
/// covered by testing it directly, once, here.
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
