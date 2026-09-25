// The Work Manager pill's number: PRs of yours (or awaiting you) that
// carry conflicts or unresolved comments - and nothing else.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { prMentions } from "../lib/mentions";
import { resetForTests } from "../lib/notifications";
import { usePrAttention } from "./usePrAttention";

vi.mock("../lib/toast", () => ({ toast: { info: vi.fn() } }));

// The real scan, counted: the tests below need to know HOW MANY PRs were
// scanned, not just what the scan raised.
vi.mock("../lib/mentions", async (importOriginal) => {
  const real = await importOriginal<typeof import("../lib/mentions")>();
  return { ...real, prMentions: vi.fn(real.prMentions) };
});

// announce() (the shared toast/OS-notification funnel, in the real,
// unmocked assignedAlerts module) checks document.hasFocus() itself to
// decide toast vs OS notification - force that the same way a real
// focused window would, rather than mocking exports announce does not
// call through this module's own boundary.
let hasFocusSpy: ReturnType<typeof vi.spyOn>;
beforeEach(() => {
  localStorage.clear();
  resetForTests();
  hasFocusSpy = vi.spyOn(document, "hasFocus").mockReturnValue(true);
});
afterEach(() => {
  clearMocks();
  hasFocusSpy.mockRestore();
});

function pr(id: number, opts: { conflicts?: boolean; repo?: string } = {}) {
  return {
    id,
    title: `PR ${id}`,
    repo: opts.repo ?? "web",
    repo_id: "r1",
    status: "active",
    is_draft: false,
    has_conflicts: opts.conflicts ?? false,
    source_branch: "f",
    target_branch: "main",
    created: "",
    closed: null,
    merge_commit: null,
    author: "a",
    reviewers: [],
    web_url: "",
  };
}

function Probe({ org, project }: { org: string; project: string }) {
  const n = usePrAttention(org, project);
  return <output>{n}</output>;
}

function mount(overview: unknown, threadsById: Record<number, { status: string }[]>) {
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return overview;
    if (cmd === "pr_threads") {
      const a = args as { prId: number };
      return (threadsById[a.prId] ?? []).map((t, i) => ({
        id: i,
        status: t.status,
        comments: [],
        file: null,
        line: null,
      }));
    }
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Probe org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

test("counts conflicts and unresolved comments once per PR, ignoring settled ones", async () => {
  mount(
    {
      // #1 has conflicts AND an unresolved thread - one PR, not two
      // problems; #2 is clean; #3 (awaiting) has an open thread; #1
      // appears in both slices and must not be double-counted.
      mine: [pr(1, { conflicts: true }), pr(2)],
      awaiting: [pr(1, { conflicts: true }), pr(3)],
    },
    {
      1: [{ status: "active" }],
      2: [{ status: "fixed" }, { status: "closed" }],
      3: [{ status: "" }], // ADO sends no status for never-resolved threads
    },
  );
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("2"));
});

test("an empty overview needs no attention", async () => {
  mount({ mine: [], awaiting: [] }, {});
  await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("0"));
});

test("a PR comment that mentions you raises a Mention; yours and others' do not", async () => {
  const hourAgo = new Date(Date.now() - 3_600_000).toISOString();
  const c = (id: number, author: string, author_id: string, content: string) => ({
    id, author, author_id, avatar: "", content, published: hourAgo, edited: false,
  });
  mockIPC((cmd) => {
    if (cmd === "connected_user") return { id: "me-guid", display_name: "Avin" };
    if (cmd === "pr_overview") return { mine: [pr(1)], awaiting: [] };
    if (cmd === "pr_threads")
      return [
        {
          id: 30, status: "active", file_path: "", line: 0, last_updated: hourAgo,
          comments: [
            c(5, "Sam", "sam-guid", "@<ME-GUID> can you look?"),
            c(6, "Avin", "ME-GUID", "@<me-guid> note to self"),
            c(7, "Sam", "sam-guid", "@<kim-guid> over to you"),
          ],
        },
      ];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Probe org="acme" project="Web" />
    </QueryClientProvider>,
  );
  const ids = () =>
    (JSON.parse(localStorage.getItem("tcm-v2-notifications:acme") ?? "[]") as Array<{ id: string }>).map((n) => n.id);
  await waitFor(() => expect(ids()).toContain("mention:pr:web:1:30:5"));
  const list = JSON.parse(localStorage.getItem("tcm-v2-notifications:acme")!) as Array<Record<string, unknown>>;
  expect(list.find((n) => n.id === "mention:pr:web:1:30:5")).toMatchObject({
    kind: "mention",
    title: "Sam mentioned you on PR #1",
    body: "@you can you look?",
    target: { kind: "pr", repo: "web", id: 1, project: "Web" },
  });
  expect(ids()).not.toContain("mention:pr:web:1:30:6");
  expect(ids()).not.toContain("mention:pr:web:1:30:7");
});

test("a failed identity lookup is logged and retried, and the PR scan resumes once it succeeds", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
  try {
    const hourAgo = new Date(Date.now() - 3_600_000).toISOString();
    let userCalls = 0;
    const logged: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "connected_user") {
        userCalls += 1;
        if (userCalls === 1) throw { kind: "Network", detail: "Can't reach Azure DevOps." };
        return { id: "me-guid", display_name: "Avin" };
      }
      if (cmd === "pr_overview") return { mine: [pr(1)], awaiting: [] };
      if (cmd === "pr_threads")
        return [
          {
            id: 30, status: "active", file_path: "", line: 0, last_updated: hourAgo,
            comments: [{ id: 5, author: "Sam", author_id: "sam-guid", avatar: "", content: "@<ME-GUID> can you look?", published: hourAgo, edited: false }],
          },
        ];
      if (cmd === "log_ui") logged.push((args as { message: string }).message);
    });
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const ids = () =>
      (JSON.parse(localStorage.getItem("tcm-v2-notifications:acme") ?? "[]") as Array<{ id: string }>).map((n) => n.id);
    const tick = async (ms: number) => {
      await act(async () => {
        await vi.advanceTimersByTimeAsync(ms);
      });
    };

    render(
      <QueryClientProvider client={qc}>
        <Probe org="acme" project="Web" />
      </QueryClientProvider>,
    );
    await tick(50);
    expect(userCalls).toBe(1);
    expect(logged.some((m) => m.startsWith("mentions: "))).toBe(true);
    // No URL in the failure line - just the readable sentence unwrap() built.
    expect(logged.some((m) => m.includes("http"))).toBe(false);
    expect(ids()).not.toContain("mention:pr:web:1:30:5");

    // Five minutes on: the identity query is in error, so it is due for a
    // retry - the same interval the other mention checks use.
    await tick(5 * 60_000 + 1_000);
    await tick(50);
    expect(userCalls).toBe(2);
    // waitFor polls with a real timer, which fake timers never advance -
    // the effect chain has already settled by here, so assert directly.
    expect(ids()).toContain("mention:pr:web:1:30:5");
  } finally {
    vi.useRealTimers();
  }
});

/// `reSignIn` invalidates connected-user, pr-overview and pr-threads
/// together. If a thread refresh settles before identity does, scanning
/// with the still-cached (about to be stale) `myId` would raise a mention
/// meant for whoever is signing OUT. The scan must wait for identity to
/// settle - `isFetching` alone is not enough, since a failed refetch keeps
/// the previous `data` too (covered by the isError branch of the previous
/// test, which resumes once the retry succeeds).
test("the mention scan holds a new comment back until an in-flight identity refetch settles", async () => {
  const hourAgo = new Date(Date.now() - 3_600_000).toISOString();
  let userCalls = 0;
  let resolveSecondUser: ((v: unknown) => void) | undefined;
  let comments = [
    { id: 5, author: "Sam", author_id: "sam-guid", avatar: "", content: "@<ME-GUID> can you look?", published: hourAgo, edited: false },
  ];
  mockIPC((cmd) => {
    if (cmd === "connected_user") {
      userCalls += 1;
      if (userCalls === 1) return { id: "me-guid", display_name: "Avin" };
      return new Promise((resolve) => {
        resolveSecondUser = resolve;
      });
    }
    if (cmd === "pr_overview") return { mine: [pr(1)], awaiting: [] };
    if (cmd === "pr_threads")
      return [{ id: 30, status: "active", file_path: "", line: 0, last_updated: hourAgo, comments }];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Probe org="acme" project="Web" />
    </QueryClientProvider>,
  );
  const ids = () =>
    (JSON.parse(localStorage.getItem("tcm-v2-notifications:acme") ?? "[]") as Array<{ id: string }>).map((n) => n.id);
  await waitFor(() => expect(ids()).toContain("mention:pr:web:1:30:5"));

  // A second comment arrives, at the same moment identity starts a refetch
  // that this test holds open.
  comments = [
    ...comments,
    { id: 6, author: "Sam", author_id: "sam-guid", avatar: "", content: "@<ME-GUID> and this too", published: hourAgo, edited: false },
  ];
  await act(async () => {
    qc.invalidateQueries({ queryKey: ["connected-user", "acme"] });
  });
  await act(async () => {
    await qc.refetchQueries({ queryKey: ["pr-threads", "acme", "Web", "web", 1] });
  });
  // The new comment is in, but identity is still mid-refetch: it must not
  // be scanned yet, whatever the stale id would have matched.
  expect(ids()).not.toContain("mention:pr:web:1:30:6");

  // Identity settles - the deferred scan now runs and raises what it held.
  await act(async () => {
    resolveSecondUser?.({ id: "me-guid", display_name: "Avin" });
  });
  await waitFor(() => expect(ids()).toContain("mention:pr:web:1:30:6"));
});

/// A thread query settling re-renders the hook with EVERY PR's threads.
/// Scanning them all each time made one poll cycle PRs x threads; a refresh
/// of one PR's threads scans that PR only.
test("a thread refresh rescans that PR's threads only, not every PR's", async () => {
  vi.mocked(prMentions).mockClear();
  mockIPC((cmd) => {
    if (cmd === "connected_user") return { id: "me-guid", display_name: "Avin" };
    if (cmd === "pr_overview") return { mine: [pr(1), pr(2), pr(3)], awaiting: [] };
    if (cmd === "pr_threads") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Probe org="acme" project="Web" />
    </QueryClientProvider>,
  );
  const scanned = () => vi.mocked(prMentions).mock.calls.map((c) => c[0].id);
  await waitFor(() => expect([...scanned()].sort()).toEqual([1, 2, 3]));

  vi.mocked(prMentions).mockClear();
  await act(async () => {
    await qc.refetchQueries({ queryKey: ["pr-threads", "acme", "Web", "web", 2] });
  });
  // react-query's notify of the refetched observer lands on the next
  // macrotask, after this act() block's own microtasks have already run -
  // waitFor's real-timer polling is what catches it.
  await waitFor(() => expect(scanned()).toEqual([2]));
});

/// What was scanned is kept only for the PRs in the current list, so it
/// cannot grow with every PR seen in a session. A PR that leaves the list
/// and comes back is simply scanned again; the store dedupes what it finds.
test("a PR that leaves the list and comes back is scanned again", async () => {
  vi.mocked(prMentions).mockClear();
  let listed = [pr(1), pr(2)];
  mockIPC((cmd) => {
    if (cmd === "connected_user") return { id: "me-guid", display_name: "Avin" };
    if (cmd === "pr_overview") return { mine: listed, awaiting: [] };
    if (cmd === "pr_threads") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <Probe org="acme" project="Web" />
    </QueryClientProvider>,
  );
  const scanned = () => vi.mocked(prMentions).mock.calls.map((c) => c[0].id);
  await waitFor(() => expect([...scanned()].sort()).toEqual([1, 2]));

  // #2 leaves the list... (the refetched observer notifies on a later
  // macrotask, so give it a few before moving on)
  listed = [pr(1)];
  await act(async () => {
    await qc.refetchQueries({ queryKey: ["pr-overview", "acme", "Web"] });
    await new Promise((r) => setTimeout(r, 20));
  });

  // ...and comes back, its threads still cached from before.
  vi.mocked(prMentions).mockClear();
  listed = [pr(1), pr(2)];
  await act(async () => {
    await qc.refetchQueries({ queryKey: ["pr-overview", "acme", "Web"] });
  });
  await waitFor(() => expect(scanned()).toEqual([2]));
});
