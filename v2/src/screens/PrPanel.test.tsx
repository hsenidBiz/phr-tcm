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
  description: "Why this change exists",
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

test("your own PR in the selected repo is not duplicated under Active on X", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [pr(20, { repo: "web" })] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    // The repo's active list contains your PR (20) plus a stranger's (21).
    if (cmd === "repo_pull_requests")
      return [pr(20, { repo: "web" }), pr(21, { repo: "web", author: "Kim" })];
  });
  renderPanel();
  await screen.findByText("!20"); // your PR loads first (overview)
  fireEvent.change(screen.getByLabelText("Repository"), { target: { value: "r1" } });
  // Wait for the repo's active list to load (the stranger's PR appears).
  await screen.findByText("!21");

  // !20 appears exactly once (under Your pull requests), !21 once under Active.
  expect(screen.getAllByText("!20")).toHaveLength(1);
  // The single !20 sits above the Active heading (i.e. in the mine group).
  const mine = screen.getByText("!20");
  const activeHeading = screen.getByText("Active on web");
  expect(
    mine.compareDocumentPosition(activeHeading) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
});

test("a row expands to description and named reviewer votes", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(4)], mine: [] };
    if (cmd === "list_repos") return [];
  });
  renderPanel();
  const row = (await screen.findByText("!4")).closest("[aria-expanded]")!;
  expect(row).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("Why this change exists")).not.toBeInTheDocument();

  fireEvent.click(row);
  expect(await screen.findByText("Why this change exists")).toBeInTheDocument();
  expect(screen.getByText("Kim")).toBeInTheDocument();
  expect(screen.getByText(/— approved/)).toBeInTheDocument();
  // The external-link control sits on the row for the browser hop.
  expect(screen.getByRole("button", { name: "Open !4 in Azure DevOps" })).toBeInTheDocument();
});

test("expanding a PR shows its linked work items as DevOps-style chips", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(8)], mine: [] };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_work_items")
      return [
        {
          id: 143783,
          work_item_type: "Bug",
          title: "Participants - department inconsistencies",
          state: "In Progress",
          state_color: "007acc",
          url: "https://example.invalid/wi/143783",
        },
      ];
  });
  renderPanel();
  // Work items load lazily - not requested until the row is expanded.
  expect(screen.queryByText(/143783/)).not.toBeInTheDocument();
  fireEvent.click((await screen.findByText("!8")).closest("[aria-expanded]")!);

  expect(await screen.findByText("Related work items")).toBeInTheDocument();
  const chip = screen.getByText("143783").closest("button")!;
  expect(chip).toHaveTextContent("Participants - department inconsistencies");
  expect(chip).toHaveTextContent("In Progress");
});

test("the description renders markdown like Azure DevOps", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview")
      return {
        awaiting: [
          pr(6, { description: "**Issue Summary**\nEmployee Repository was wrong\n\n- one\n- two" }),
        ],
        mine: [],
      };
    if (cmd === "list_repos") return [];
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!6")).closest("[aria-expanded]")!);

  // Bold survives as <strong>, and the list becomes real bullets.
  const summary = await screen.findByText("Issue Summary");
  expect(summary.tagName).toBe("STRONG");
  expect(screen.getByText("one").closest("li")).toBeInTheDocument();
});

test("the completed filter runs a separate query and titles the group", async () => {
  const asked: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "repo_pull_requests") {
      const status = (args as { status: string }).status;
      asked.push(status);
      return status === "completed"
        ? [pr(20620, { title: "Merged thing", status: "completed", closed: "2026-07-24T08:00:00Z" })]
        : [pr(9, { title: "Still open" })];
    }
  });
  renderPanel();
  // The repo list loads async - selecting before its <option> exists is a
  // no-op, so wait for it.
  await screen.findByRole("option", { name: "web" });
  fireEvent.change(screen.getByLabelText("Repository"), { target: { value: "r1" } });
  // Titles render as "!<id> <title>" across sibling nodes, so match the id.
  expect(await screen.findByText("!9")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "completed" }));
  expect(await screen.findByText("!20620")).toBeInTheDocument();
  expect(screen.getByText("Completed on web")).toBeInTheDocument();
  expect(screen.queryByText("!9")).not.toBeInTheDocument();
  // The status is a server-side query, not a client-side filter.
  expect(asked).toContain("completed");
});

test("an expanded PR shows its builds, stages and deployed environments", async () => {
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return { awaiting: [pr(42, { merge_commit: "abc123" })], mine: [] };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_work_items") return [];
    if (cmd === "pr_pipeline") {
      expect((args as { mergeCommit: string }).mergeCommit).toBe("abc123");
      return [
        {
          id: 901, name: "HRM-PMS-NET", number: "2026.7.24-12", status: "completed",
          result: "succeeded", is_validation: false,
          started: "2026-07-24T09:00:00Z", finished: "", web_url: "https://x/901",
          stages: [{ name: "Stage", state: "completed", result: "succeeded" }],
          deployments: [
            { release: "Release-482", environment: "QA", status: "succeeded", on: "", web_url: "" },
            { release: "Release-482", environment: "Production", status: "notStarted", on: "", web_url: "" },
          ],
        },
      ];
    }
  });
  renderPanel();
  // Pipeline data is lazy: nothing requested until the row opens.
  expect(screen.queryByText("Pipeline")).not.toBeInTheDocument();
  fireEvent.click((await screen.findByText("!42")).closest("[aria-expanded]")!);

  expect(await screen.findByText("Pipeline")).toBeInTheDocument();
  // The heading renders immediately; the builds arrive with the query.
  expect(await screen.findByText("HRM-PMS-NET")).toBeInTheDocument();
  expect(screen.getByText("CI")).toBeInTheDocument();
  expect(screen.getByText("Stage")).toBeInTheDocument();
  // The environments are the point: which ones it reached, and how far.
  expect(screen.getByText("QA")).toBeInTheDocument();
  expect(screen.getByText("Production")).toBeInTheDocument();
  expect(screen.getByTitle(/Release-482 . Production: not started/)).toBeInTheDocument();
});

test("a PR with no pipeline runs says so instead of looking broken", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(5)], mine: [] };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_work_items") return [];
    if (cmd === "pr_pipeline") return [];
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!5")).closest("[aria-expanded]")!);
  expect(await screen.findByText("No builds found for this pull request.")).toBeInTheDocument();
});
