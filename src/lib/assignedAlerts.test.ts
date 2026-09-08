import { afterEach, expect, test, vi } from "vitest";
import type { AssignedItem } from "../bindings";
import { appIsInView, summarize } from "./assignedAlerts";

const item = (id: number, title: string, type = "Task"): AssignedItem => ({
  id,
  title,
  work_item_type: type,
  state: "Active",
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
