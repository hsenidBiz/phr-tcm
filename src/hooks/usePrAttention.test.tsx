// The Work Manager pill's number: PRs of yours (or awaiting you) that
// carry conflicts or unresolved comments - and nothing else.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { usePrAttention } from "./usePrAttention";

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
