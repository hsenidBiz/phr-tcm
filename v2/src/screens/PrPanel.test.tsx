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
