import { afterEach, beforeEach, expect, test } from "vitest";
import type { PullRequest } from "../bindings";
import {
  LIST_CAP,
  clearAll,
  dismiss,
  markAllRead,
  noteAssigned,
  noteFinding,
  notePrComments,
  notePrOverview,
  raise,
  resetForTests,
  unreadCount,
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

/// A finding an assistant records is something that happened while you
/// were not looking, so it goes on the bell like an assignment does.
test("a recorded finding raises one notification, keyed by its id", () => {
  noteFinding("acme", { id: "1-0", kind: "spec", title: "AC-3 contradicts the table" });
  noteFinding("acme", { id: "1-0", kind: "spec", title: "AC-3 contradicts the table" });
  expect(read()).toHaveLength(1);
  const full = JSON.parse(localStorage.getItem(`tcm-v2-notifications:${ORG}`) ?? "[]") as Array<{
    kind: string;
    title: string;
    body: string;
  }>;
  expect(full[0].kind).toBe("ai-finding");
  expect(full[0].title).toBe("AI finding: spec");
  expect(full[0].body).toBe("AC-3 contradicts the table");
});
