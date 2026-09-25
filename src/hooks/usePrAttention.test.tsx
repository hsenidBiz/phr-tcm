// The Work Manager pill's number: PRs of yours (or awaiting you) that
// carry conflicts or unresolved comments - and nothing else.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { resetForTests } from "../lib/notifications";
import { usePrAttention } from "./usePrAttention";

vi.mock("../lib/toast", () => ({ toast: { info: vi.fn() } }));
// announce() (the shared toast/OS-notification funnel) must stay real -
// only the in-view check is forced, so announce's own toast branch runs.
vi.mock("../lib/assignedAlerts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/assignedAlerts")>()),
  appIsInView: () => true,
  osNotify: () => Promise.resolve(true),
}));

beforeEach(() => {
  localStorage.clear();
  resetForTests();
});
afterEach(() => clearMocks());

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
