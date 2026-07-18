import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import PrPanel from "./PrPanel";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderPanel() {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <PrPanel org="acme" project="Web" />
    </QueryClientProvider>,
  );
}

const pr = (id: number, over: Partial<Record<string, unknown>> = {}) => ({
  id,
  title: `PR ${id}`,
  repo: "web",
  author: "Sam",
  source_branch: "feature/x",
  target_branch: "main",
  created: "2026-07-18T01:00:00Z",
  is_draft: false,
  has_conflicts: false,
  my_vote: 0,
  reviewers: [
    { display_name: "Avin", vote: 0 },
    { display_name: "Kim", vote: 10 },
  ],
  web_url: "https://example.invalid/pr",
  ...over,
});

test("groups render in actionability order with counts and badges", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview")
      return {
        awaiting: [pr(1)],
        mine: [pr(2, { is_draft: true, has_conflicts: true })],
      };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
  });
  renderPanel();

  const awaiting = await screen.findByText("Awaiting your review");
  const mine = screen.getByText("Your pull requests");
  // Awaiting sits above Mine in the document.
  expect(
    awaiting.compareDocumentPosition(mine) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();

  expect(screen.getByText("!1")).toBeInTheDocument();
  const mineRow = screen.getByText("!2").closest("button")!;
  expect(within(mineRow).getByText("Draft")).toBeInTheDocument();
  expect(within(mineRow).getByText("Conflicts")).toBeInTheDocument();
  // Reviewer pips carry the vote in their tooltip.
  expect(within(mineRow).getByTitle("Kim: approved")).toBeInTheDocument();
});

test("picking a repo fetches its active PRs and persists the choice", async () => {
  let asked = "";
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos")
      return [
        { id: "r1", name: "web" },
        { id: "r2", name: "api" },
      ];
    if (cmd === "repo_pull_requests") {
      asked = (args as { repoId: string }).repoId;
      return [pr(9, { repo: "api" })];
    }
  });
  renderPanel();
  await screen.findByText("Awaiting your review");

  fireEvent.change(screen.getByLabelText("Repository"), { target: { value: "r2" } });
  expect(await screen.findByText(/Active on api/)).toBeInTheDocument();
  expect(await screen.findByText("!9")).toBeInTheDocument();
  expect(asked).toBe("r2");
  expect(localStorage.getItem("tcm-v2-pr-repo:acme/Web")).toBe("r2");
});
