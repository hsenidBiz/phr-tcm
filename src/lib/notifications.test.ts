import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test } from "vitest";
import type { PullRequest } from "../bindings";
import {
  KNOWN_CAP,
  LIST_CAP,
  clearAll,
  dismiss,
  forgetAllNotifications,
  markAllRead,
  markSeen,
  noteAssigned,
  notePrComments,
  notePrOverview,
  raise,
  resetForTests,
  unreadCount,
  useNotifications,
} from "./notifications";

// The store caches per org in memory; every test starts from nothing.
beforeEach(() => {
  localStorage.clear();
  resetForTests();
});
afterEach(() => {
  localStorage.clear();
  resetForTests();
});

const ORG = "acme";

function read() {
  return JSON.parse(localStorage.getItem(`tcm-v2-notifications:${ORG}`) ?? "[]") as Array<{
    id: string;
    read: boolean;
  }>;
}

test("raise adds unread items newest first and persists them", () => {
  raise(ORG, [{ id: "a", kind: "assigned", title: "A", body: "" }]);
  raise(ORG, [{ id: "b", kind: "assigned", title: "B", body: "" }]);
  const list = read();
  expect(list.map((n) => n.id)).toEqual(["b", "a"]);
  expect(list.every((n) => n.read === false)).toBe(true);
});

/// The dedupe is what turns "the current state" into "an event": a
/// source may report the same conflict on every poll, and a dismissed
/// one must stay dismissed.
test("an id already raised - listed or dismissed - is never raised again", () => {
  expect(raise(ORG, [{ id: "a", kind: "assigned", title: "A", body: "" }])).toHaveLength(1);
  expect(raise(ORG, [{ id: "a", kind: "assigned", title: "A", body: "" }])).toHaveLength(0);
  dismiss(ORG, "a");
  expect(read()).toHaveLength(0);
  expect(raise(ORG, [{ id: "a", kind: "assigned", title: "A", body: "" }])).toHaveLength(0);
  expect(read()).toHaveLength(0);
});

test("markAllRead clears the badge and keeps the items; clearAll empties", () => {
  raise(ORG, [
    { id: "a", kind: "assigned", title: "A", body: "" },
    { id: "b", kind: "pr-review", title: "B", body: "" },
  ]);
  expect(unreadCount(read().map((n) => ({ ...n, kind: "assigned", title: "", body: "", at: "" })))).toBe(2);
  markAllRead(ORG);
  expect(read().every((n) => n.read)).toBe(true);
  expect(read()).toHaveLength(2);
  clearAll(ORG);
  expect(read()).toHaveLength(0);
});

test("the list keeps only the newest LIST_CAP", () => {
  for (let i = 0; i < LIST_CAP + 5; i++) {
    raise(ORG, [{ id: `n${i}`, kind: "assigned", title: "", body: "" }]);
  }
  const list = read();
  expect(list).toHaveLength(LIST_CAP);
  expect(list[0].id).toBe(`n${LIST_CAP + 4}`);
});

const pr = (over: Partial<PullRequest>): PullRequest =>
  ({
    id: 1,
    title: "Fix login",
    repo: "Web",
    repo_id: "r",
    author: "Bob",
    source_branch: "f",
    target_branch: "main",
    created: "",
    description: "",
    is_draft: false,
    has_conflicts: false,
    status: "active",
    closed: "",
    merge_commit: "",
    my_vote: 0,
    reviewers: [],
    ...over,
  }) as PullRequest;

/// The overview is re-reported on every refresh; only a NEW conflict on
/// one of your PRs, or a PR newly awaiting you, becomes a notification -
/// and each exactly once.
test("notePrOverview raises conflicts on your PRs and PRs awaiting you, once each", () => {
  const overview = {
    mine: [pr({ id: 10, has_conflicts: true }), pr({ id: 11, has_conflicts: false })],
    awaiting: [pr({ id: 20, title: "Add report", author: "Cy" })],
  };
  notePrOverview(ORG, "Web", overview);
  notePrOverview(ORG, "Web", overview); // the next poll, same state
  const list = read();
  expect(list.map((n) => n.id).sort()).toEqual(["pr-conflict:Web:10", "pr-review:Web:20"]);
  const full = JSON.parse(localStorage.getItem(`tcm-v2-notifications:${ORG}`) ?? "[]") as Array<{
    id: string;
    title: string;
    href: string;
  }>;
  const conflict = full.find((n) => n.id === "pr-conflict:Web:10")!;
  expect(conflict.title).toBe("PR #10 has merge conflicts");
  expect(conflict.href).toBe("https://dev.azure.com/acme/Web/_git/Web/pullrequest/10");
  const review = full.find((n) => n.id === "pr-review:Web:20")!;
  expect(review.title).toBe("PR #20 is waiting for your review");
});

/// "Comments to resolve" used to be a number on the Work Manager pill; it
/// now lives in the bell. Keyed on the count, so the same two comments
/// never raise twice, and a third arriving is news again.
test("notePrComments raises once per PR-and-count, and again when the count grows", () => {
  const p = pr({ id: 30, title: "Timeline" });
  notePrComments(ORG, "Web", p, 2);
  notePrComments(ORG, "Web", p, 2); // next poll, same two threads
  expect(read().map((n) => n.id)).toEqual(["pr-comments:Web:30:2"]);
  notePrComments(ORG, "Web", p, 3);
  expect(read().map((n) => n.id)).toEqual(["pr-comments:Web:30:3", "pr-comments:Web:30:2"]);
  notePrComments(ORG, "Web", p, 0); // nothing to resolve raises nothing
  expect(read()).toHaveLength(2);
  const full = JSON.parse(localStorage.getItem(`tcm-v2-notifications:${ORG}`) ?? "[]") as Array<{ title: string }>;
  expect(full[0].title).toBe("PR #30 has 3 comments to resolve");
});

test("noteAssigned raises one item per work item, linked to it", () => {
  noteAssigned(ORG, "Web", [{ id: 501, title: "Wire the login flow", work_item_type: "Task", state: "New" }]);
  const full = JSON.parse(localStorage.getItem(`tcm-v2-notifications:${ORG}`) ?? "[]") as Array<{
    id: string;
    title: string;
    body: string;
    href: string;
  }>;
  expect(full).toHaveLength(1);
  expect(full[0].id).toBe("assigned:501");
  expect(full[0].title).toBe("Task #501 assigned to you");
  expect(full[0].body).toBe("Wire the login flow");
  expect(full[0].href).toBe("https://dev.azure.com/acme/Web/_workitems/edit/501");
});

/// The href is the browser's way in; the target is the app's own. Both are
/// stored, so a click can stay inside the app and the browser stays one
/// click away.
test("sources carry a structured target beside the browser href", () => {
  noteAssigned(ORG, "Web", [
    { id: 501, title: "Wire login", work_item_type: "Task", state: "New" },
  ]);
  notePrOverview(ORG, "Web", {
    mine: [pr({ id: 12, repo: "web", has_conflicts: true })],
    awaiting: [pr({ id: 13, repo: "web" })],
  });
  notePrComments(ORG, "Web", pr({ id: 14, repo: "web" }), 2);
  const full = JSON.parse(localStorage.getItem(`tcm-v2-notifications:${ORG}`) ?? "[]") as Array<{
    id: string;
    target?: unknown;
  }>;
  const byId = Object.fromEntries(full.map((n) => [n.id, n.target]));
  // Every destination is project-scoped, so the target carries the project
  // it came from, not just what it points at.
  expect(byId["assigned:501"]).toEqual({ kind: "work-item", id: 501, project: "Web" });
  expect(byId["pr-conflict:web:12"]).toEqual({ kind: "pr", repo: "web", id: 12, project: "Web" });
  expect(byId["pr-review:web:13"]).toEqual({ kind: "pr", repo: "web", id: 13, project: "Web" });
  expect(byId["pr-comments:web:14:2"]).toEqual({ kind: "pr", repo: "web", id: 14, project: "Web" });
});

test("markSeen records ids without listing them, and raise skips them after", () => {
  markSeen(ORG, ["mention:wi:1:1", "mention:wi:1:1"]);
  expect(read()).toEqual([]);
  expect(raise(ORG, [{ id: "mention:wi:1:1", kind: "mention", title: "M", body: "" }])).toEqual([]);
  expect(raise(ORG, [{ id: "mention:wi:2:1", kind: "mention", title: "N", body: "" }]).map((n) => n.id)).toEqual([
    "mention:wi:2:1",
  ]);
});

/// Two accounts on one Windows profile (or a mid-session re-sign-in as
/// someone else) must not see each other's mentions and PR notices, bodies
/// included - forgetAllNotifications is what claimCacheFor's account
/// switch reaches for. It runs from App during render, so its notify is a
/// queued microtask rather than synchronous - useNotifications repaints on
/// the next flush, same as any other externally-raised change.
test("forgetAllNotifications empties every organisation's bell and un-forgets dismissed ids", async () => {
  raise(ORG, [
    { id: "a", kind: "assigned", title: "A", body: "" },
    { id: "b", kind: "assigned", title: "B", body: "" },
  ]);
  raise("globex", [{ id: "g", kind: "assigned", title: "G", body: "" }]);
  dismiss(ORG, "a"); // dismissed, not forgotten - known() still remembers it

  const { result } = renderHook(() => useNotifications(ORG));
  expect(result.current.map((n) => n.id)).toEqual(["b"]);

  forgetAllNotifications();
  // The notify is a queued microtask (this runs from App during render, so
  // it cannot fire synchronously) - repaints on the next flush.
  await waitFor(() => expect(result.current).toHaveLength(0));

  expect(localStorage.getItem(`tcm-v2-notifications:${ORG}`)).toBeNull();
  expect(localStorage.getItem(`tcm-v2-notifications-known:${ORG}`)).toBeNull();
  expect(localStorage.getItem("tcm-v2-notifications:globex")).toBeNull();

  // The dismissed id is no longer "known" - it can raise again, for the
  // next account, exactly like a first-ever sighting.
  expect(raise(ORG, [{ id: "a", kind: "assigned", title: "A", body: "" }]).map((n) => n.id)).toEqual(["a"]);
});

/// A source re-reports its whole state on every check. An id it still
/// reports must stay "seen", however many other ids arrive over time - or a
/// mention dismissed weeks ago comes back once 500 newer ids push it out.
test("an id every check still reports never falls out of the seen set", () => {
  const mention = { id: "mention:wi:41:7", kind: "mention" as const, title: "Sam mentioned you", body: "" };
  expect(raise(ORG, [mention])).toHaveLength(1);
  dismiss(ORG, mention.id);
  for (let check = 0; check < 5; check++) {
    // Between two checks: fewer new ids than the cap, but many in all.
    raise(
      ORG,
      Array.from({ length: 300 }, (_, i) => ({
        id: `pr-comments:web:${check}:${i}`,
        kind: "pr-comments" as const,
        title: "t",
        body: "",
      })),
    );
    // The next check reports the mention again: still seen, not raised.
    expect(raise(ORG, [mention])).toEqual([]);
  }
});

test("one report larger than the cap keeps every id it reported", () => {
  const many = Array.from({ length: KNOWN_CAP + 100 }, (_, i) => ({
    id: `pr-review:web:${i}`,
    kind: "pr-review" as const,
    title: "t",
    body: "",
  }));
  raise(ORG, many);
  expect(raise(ORG, [many[KNOWN_CAP + 99]])).toEqual([]);
});
