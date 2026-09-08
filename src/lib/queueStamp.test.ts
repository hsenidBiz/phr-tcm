import { describe, expect, it } from "vitest";
import type { TestCase } from "../bindings";
import { noteSyncPairs, stampFileSlices, unstampedCreated } from "./queueStamp";

const tc = (title: string, over: Partial<TestCase> = {}): TestCase => ({
  title,
  steps: [{ action: "Do", expected: "Done" }],
  tags: "",
  automation_status: "Not Automated",
  module_value: "",
  preconditions: "",
  update_id: null,
  spec_order: null,
  tester_order: null,
  comment: "",
  reviewer_notes: "",
  ...over,
});

describe("stampFileSlices", () => {
  it("stamps a created case's new id into its file, so re-import no-ops", () => {
    const a = tc("Created one", { comment: "review me" });
    const b = tc("Failed one");
    const hand = tc("Typed by hand");
    const prev = [a, b, hand];
    const owners = ["C:/d/a.json", "C:/d/a.json", ""];
    const sent = [a, b, hand];
    const files = stampFileSlices(prev, owners, sent, [
      { index: 0, action: "created", id: 501 },
      { index: 1, action: "failed" },
      { index: 2, action: "created", id: 502 },
    ]);

    const f = files.get("C:/d/a.json")!;
    expect(f.changed).toBe(true);
    expect(f.slice).toHaveLength(2);
    // The created case now says UPDATE #501 - importing it again is an
    // update that matches the server, which the review gate skips.
    expect(f.slice[0].update_id).toBe(501);
    // Its comment rides along untouched - stamping must never cost a note.
    expect(f.slice[0].comment).toBe("review me");
    // The failed case stays exactly as it was: still a draft, still ready
    // to retry.
    expect(f.slice[1].update_id).toBeNull();
    // Hand-typed cases belong to no file and appear in none.
    expect([...files.keys()]).toEqual(["C:/d/a.json"]);
  });

  it("a file whose cases all failed is not rewritten", () => {
    const a = tc("Failed");
    const files = stampFileSlices([a], ["C:/d/a.json"], [a], [{ index: 0, action: "failed" }]);
    expect(files.get("C:/d/a.json")!.changed).toBe(false);
  });

  it("skipped no-op rows keep their place without forcing a write", () => {
    const done = tc("Already up to date", { update_id: 9 });
    const fresh = tc("New");
    const prev = [done, fresh];
    const owners = ["C:/d/a.json", "C:/d/a.json"];
    // `sent` excludes the no-op; its result index points into `sent`.
    const files = stampFileSlices(prev, owners, [fresh], [
      { index: 0, action: "created", id: 700 },
    ]);
    const f = files.get("C:/d/a.json")!;
    expect(f.slice.map((c) => c.update_id)).toEqual([9, 700]);
    expect(f.changed).toBe(true);
  });
});

describe("noteSyncPairs", () => {
  it("carries a succeeded case's comment to its work item id", () => {
    const created = tc("A", { comment: "check step 3" });
    const updated = tc("B", { update_id: 42, comment: "flaky?" });
    const silent = tc("C"); // no comment - nothing to sync
    const failed = tc("D", { comment: "never landed" });
    const pairs = noteSyncPairs([created, updated, silent, failed], [
      { index: 0, action: "created", id: 501 },
      { index: 1, action: "updated" },
      { index: 2, action: "created", id: 502 },
      { index: 3, action: "failed" },
    ]);
    expect(pairs).toEqual([
      { id: 501, comment: "check step 3" },
      { id: 42, comment: "flaky?" },
    ]);
  });
});

describe("unstampedCreated", () => {
  it("names created cases with no owning file", () => {
    const a = tc("Renamed in app");
    const b = tc("Still owned");
    const prev = [a, b];
    const owners = ["", "C:/drafts/a.json"]; // a lost its owner (renamed)
    const out = unstampedCreated(prev, owners, prev, [
      { index: 0, action: "created", id: 900 },
      { index: 1, action: "created", id: 901 },
    ]);
    expect(out).toEqual(["Renamed in app"]);
  });

  it("flags every create when there is no file at all (shared draft)", () => {
    const a = tc("From a share link");
    const out = unstampedCreated([a], [""], [a], [{ index: 0, action: "created", id: 900 }]);
    expect(out).toEqual(["From a share link"]);
  });

  it("ignores updates and failures", () => {
    const a = tc("Updated", { update_id: 55 });
    const b = tc("Failed");
    const out = unstampedCreated([a, b], ["", ""], [a, b], [
      { index: 0, action: "updated", id: 55 },
      { index: 1, action: "failed", id: null },
    ]);
    expect(out).toEqual([]);
  });
});
