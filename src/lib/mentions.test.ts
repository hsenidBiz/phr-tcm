import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Mention, PrThread } from "../bindings";
import { setTourRunning } from "../tour/tourState";
import { announce } from "./assignedAlerts";
import {
  announceMentions,
  excerpt,
  forgetMentionBaselines,
  noteMentions,
  prMentionId,
  prMentions,
  prNotification,
  workItemMentionId,
  workItemNotification,
  type FoundMention,
} from "./mentions";
import { resetForTests, type AppNotification } from "./notifications";

// The toast/OS-notification rule itself is `announce`'s job and is tested
// against the real thing in assignedAlerts.test.ts; here it is a plain
// spy, so these tests only have to show announceMentions calls it with
// the right title and body. `summarizeLines` is the real, already-tested
// "3 lines then …and N more" rule - no reason to fake it too.
vi.mock("./assignedAlerts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./assignedAlerts")>()),
  announce: vi.fn(),
}));

beforeEach(() => {
  localStorage.clear();
  resetForTests();
  vi.mocked(announce).mockClear();
});
afterEach(() => {
  localStorage.clear();
  resetForTests();
  // A test that forgets to flip this back off must not leak a running
  // tour into whatever runs next in this file.
  setTourRunning(false);
});

const listed = (org = "acme") =>
  (JSON.parse(localStorage.getItem(`tcm-v2-notifications:${org}`) ?? "[]") as AppNotification[]).map((n) => n.id);

const wiMention: Mention = {
  source: "work-item",
  item_id: 41,
  item_type: "Product Backlog Item",
  item_title: "Leave requests",
  comment_id: 7,
  author: "Sam",
  excerpt: "@Avin can you check this?",
  created_date: "2026-09-25T08:00:00Z",
};

function thread(id: number, comments: Array<{ id: number; author: string; author_id: string; content: string }>): PrThread {
  return {
    id,
    status: "active",
    file_path: "",
    line: 0,
    last_updated: "2026-09-25T08:00:00Z",
    comments: comments.map((c) => ({ ...c, avatar: "", published: "2026-09-25T08:00:00Z", edited: false })),
  };
}

test("mention ids follow the spec's two shapes", () => {
  expect(workItemMentionId({ item_id: 41, comment_id: 7 })).toBe("mention:wi:41:7");
  expect(prMentionId({ repo: "web", prId: 12, threadId: 3, commentId: 9 })).toBe("mention:pr:web:12:3:9");
});

test("a work-item mention reads who, where, and opens the item", () => {
  expect(workItemNotification("acme", "Web", wiMention)).toEqual({
    id: "mention:wi:41:7",
    kind: "mention",
    title: "Sam mentioned you on Product Backlog Item #41",
    body: "@Avin can you check this?",
    href: "https://dev.azure.com/acme/Web/_workitems/edit/41",
    target: { kind: "work-item", id: 41, project: "Web" },
  });
});

test("a PR mention reads who, which PR, and opens the PR", () => {
  const n = prNotification("acme", "Web", {
    repo: "web", prId: 12, threadId: 3, commentId: 9, author: "Sam", excerpt: "@you please look", createdDate: "",
  });
  expect(n).toEqual({
    id: "mention:pr:web:12:3:9",
    kind: "mention",
    title: "Sam mentioned you on PR #12",
    body: "@you please look",
    href: "https://dev.azure.com/acme/Web/_git/web/pullrequest/12",
    target: { kind: "pr", repo: "web", id: 12, project: "Web" },
  });
});

test("the PR scan finds @<your id> in any case and skips your own comments", () => {
  const threads = [
    thread(3, [
      { id: 9, author: "Sam", author_id: "u-sam", content: "@<ME-GUID>   can you\nlook at @<KIM-GUID>?" },
      { id: 10, author: "Avin", author_id: "ME-GUID", content: "@<me-guid> note to self" },
      { id: 11, author: "Sam", author_id: "u-sam", content: "@<kim-guid> over to you" },
    ]),
  ];
  expect(prMentions({ repo: "web", id: 12 }, threads, "me-guid")).toEqual([
    {
      repo: "web", prId: 12, threadId: 3, commentId: 9, author: "Sam",
      excerpt: "@you can you look at @someone?", createdDate: "2026-09-25T08:00:00Z",
    },
  ]);
});

/// Review focus 3, webview side.
test("an empty identity id finds nothing", () => {
  const threads = [thread(3, [{ id: 9, author: "Sam", author_id: "u-sam", content: "@<> hi" }])];
  expect(prMentions({ repo: "web", id: 12 }, threads, "")).toEqual([]);
});

test("an excerpt collapses whitespace and stops at 140 characters", () => {
  expect(excerpt("  a \n\n b  ")).toBe("a b");
  const cut = excerpt("x".repeat(200));
  expect(cut).toHaveLength(140);
  expect(cut.endsWith("…")).toBe(true);
});

/// A surrogate-pair character (an emoji) straddling the old UTF-16-unit cut
/// point used to leave a lone surrogate - a broken character - in the
/// toast or OS notification. Cutting by code points keeps it whole.
test("an excerpt never splits an emoji at the cut", () => {
  const text = `${"a".repeat(138)}😀${"b".repeat(10)}`;
  expect(excerpt(text)).toBe(`${"a".repeat(138)}😀…`);
});

const NOW = Date.parse("2026-09-25T12:00:00Z");
const found = (id: string, created: string): FoundMention => ({
  notification: { id, kind: "mention", title: id, body: "" },
  created,
});

test("the first check ever raises only the last 24 hours and records older ones as seen", () => {
  const added = noteMentions(
    "acme",
    [found("mention:wi:1:1", "2026-09-25T10:00:00Z"), found("mention:wi:2:1", "2026-09-22T10:00:00Z")],
    NOW,
  );
  expect(added.map((n) => n.id)).toEqual(["mention:wi:1:1"]);
  expect(localStorage.getItem("tcm-v2-mentions-baseline:acme")).toBe(String(NOW));
  // Seen, not shown: it stays out on every later check too.
  expect(noteMentions("acme", [found("mention:wi:2:1", "2026-09-22T10:00:00Z")], NOW + 3_600_000)).toEqual([]);
  expect(listed()).toEqual(["mention:wi:1:1"]);
});

test("after the first check, a new mention raises once", () => {
  noteMentions("acme", [], NOW);
  const later = NOW + 5 * 24 * 3_600_000;
  const fresh = found("mention:pr:web:12:3:9", new Date(later - 3_600_000).toISOString());
  expect(noteMentions("acme", [fresh], later).map((n) => n.id)).toEqual(["mention:pr:web:12:3:9"]);
  expect(noteMentions("acme", [fresh], later + 300_000)).toEqual([]);
});

test("each organisation has its own first check", () => {
  noteMentions("acme", [], NOW - 10 * 24 * 3_600_000);
  const added = noteMentions("globex", [found("mention:wi:5:1", "2026-09-22T10:00:00Z")], NOW);
  expect(added).toEqual([]);
  expect(localStorage.getItem("tcm-v2-mentions-baseline:globex")).toBe(String(NOW));
});

/// Review focus 5: a malformed date cannot defeat the flood guard.
test("a mention with no readable date counts as old", () => {
  expect(noteMentions("acme", [found("mention:wi:3:1", "")], NOW)).toEqual([]);
  expect(noteMentions("acme", [found("mention:wi:3:1", "")], NOW + 60_000)).toEqual([]);
  expect(listed()).toEqual([]);
});

/// A corrupted stored baseline (never written by this code) must reset to
/// "no baseline yet", never to epoch - which would admit every mention
/// ever, the opposite of the flood guard.
test("a corrupted baseline resets rather than admitting everything", () => {
  localStorage.setItem("tcm-v2-mentions-baseline:acme", "");
  const raised = noteMentions("acme", [found("mention:wi:9:1", "2000-01-01T00:00:00Z")], NOW);
  expect(raised).toEqual([]);
  expect(localStorage.getItem("tcm-v2-mentions-baseline:acme")).toBe(String(NOW));
});

/// The guard sits in noteMentions itself (the write chokepoint), matching
/// fieldPrefs.ts / suiteSeed.ts, rather than in each hook that calls it.
test("a running tour raises nothing and writes neither the baseline nor the seen list", () => {
  setTourRunning(true);
  expect(noteMentions("acme", [found("mention:wi:1:1", "2026-09-25T10:00:00Z")], NOW)).toEqual([]);
  expect(localStorage.getItem("tcm-v2-mentions-baseline:acme")).toBeNull();

  setTourRunning(false);
  // The same mention, for real, still counts fully fresh - the tour never
  // touched this organisation's baseline or seen list.
  expect(noteMentions("acme", [found("mention:wi:1:1", "2026-09-25T10:00:00Z")], NOW).map((n) => n.id)).toEqual([
    "mention:wi:1:1",
  ]);
});

test("forgetMentionBaselines clears every organisation's baseline, so the next check is a first check", () => {
  noteMentions("acme", [], NOW);
  noteMentions("globex", [], NOW);
  expect(localStorage.getItem("tcm-v2-mentions-baseline:acme")).toBe(String(NOW));
  expect(localStorage.getItem("tcm-v2-mentions-baseline:globex")).toBe(String(NOW));

  forgetMentionBaselines();
  expect(localStorage.getItem("tcm-v2-mentions-baseline:acme")).toBeNull();
  expect(localStorage.getItem("tcm-v2-mentions-baseline:globex")).toBeNull();

  // A mention older than 24h before NOW, checked right after the wipe,
  // is treated as this organisation's own history again - not shown, but
  // it also re-establishes the baseline rather than reusing a stale one.
  const later = NOW + 3_600_000;
  expect(noteMentions("acme", [found("mention:wi:9:1", "2000-01-01T00:00:00Z")], later)).toEqual([]);
  expect(localStorage.getItem("tcm-v2-mentions-baseline:acme")).toBe(String(later));
});

const added = (n: number): AppNotification[] =>
  Array.from({ length: n }, (_, i) => ({
    id: `mention:wi:${i}:1`, kind: "mention" as const, title: `Sam mentioned you on Task #${i}`,
    body: `excerpt ${i}`, at: "", read: false,
  }));

test("one new mention announces itself, in its own words", () => {
  announceMentions(added(1));
  expect(announce).toHaveBeenCalledTimes(1);
  expect(announce).toHaveBeenCalledWith("Sam mentioned you on Task #0", "excerpt 0");
});

/// Review focus 4.
test("several new mentions make one announcement, not one each", () => {
  announceMentions(added(5));
  expect(announce).toHaveBeenCalledTimes(1);
  expect(announce).toHaveBeenCalledWith(
    "5 new mentions",
    "Sam mentioned you on Task #0\nSam mentioned you on Task #1\nSam mentioned you on Task #2\n…and 2 more",
  );
});

test("nothing new, nothing announced", () => {
  announceMentions([]);
  expect(announce).not.toHaveBeenCalled();
});
