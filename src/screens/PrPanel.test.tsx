import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ComponentProps } from "react";
import { toast } from "../lib/toast";
import { setTourRunning } from "../tour/tourState";
import PrPanel from "./PrPanel";

// The panel's "that pull request is not here" notices are toasts; assert on
// the mock the way every other screen's tests do.
vi.mock("../lib/toast", () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() },
}));

afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

function renderPanel(props: Partial<ComponentProps<typeof PrPanel>> = {}) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <PrPanel org="acme" project="Web" allProjects={false} {...props} />
    </QueryClientProvider>,
  );
}

const pr = (id: number, over: Partial<Record<string, unknown>> = {}) => ({
  id,
  title: `PR ${id}`,
  repo: "web",
  repo_id: "repo-guid-1",
  status: "active",
  closed: "",
  merge_commit: "",
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

/** Tick a repo row inside the (already open) Repositories dropdown. The
 * repo name also appears on PR-row pills, so target the dropdown's
 * checkbox label the way the Work Board tests do. */
const tickRepo = async (name: string) =>
  fireEvent.click((await screen.findAllByText(name)).find((el) => el.closest("label"))!);

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

  fireEvent.click(screen.getByLabelText("Repositories"));
  await tickRepo("api");
  expect(await screen.findByText(/Active on api/)).toBeInTheDocument();
  expect(await screen.findByText("!9")).toBeInTheDocument();
  expect(asked).toBe("r2");
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/Web")).toBe(JSON.stringify(["r2"]));
});

test("several repos each get their own section, and unticking removes one", async () => {
  const asked: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos")
      return [
        { id: "r1", name: "web" },
        { id: "r2", name: "api" },
      ];
    if (cmd === "repo_pull_requests") {
      const repoId = (args as { repoId: string }).repoId;
      asked.push(repoId);
      return repoId === "r1" ? [pr(11, { repo: "web" })] : [pr(12, { repo: "api" })];
    }
  });
  renderPanel();
  await screen.findByText("Awaiting your review");

  fireEvent.click(screen.getByLabelText("Repositories"));
  await tickRepo("web");
  await tickRepo("api");
  // One titled section per repo, each with its own PRs, in repo-list order.
  expect(await screen.findByText("Active on web")).toBeInTheDocument();
  expect(await screen.findByText("Active on api")).toBeInTheDocument();
  expect(await screen.findByText("!11")).toBeInTheDocument();
  expect(await screen.findByText("!12")).toBeInTheDocument();
  expect(
    screen
      .getByText("Active on web")
      .compareDocumentPosition(screen.getByText("Active on api")) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(new Set(asked)).toEqual(new Set(["r1", "r2"]));
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/Web")).toBe(JSON.stringify(["r1", "r2"]));

  // Unticking web drops its section but leaves api's alone.
  await tickRepo("web");
  expect(screen.queryByText("Active on web")).not.toBeInTheDocument();
  expect(screen.getByText("Active on api")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/Web")).toBe(JSON.stringify(["r2"]));
});

/// The picker orders checked options first (Your Pull Requests, then
/// tracked repos, in repo-list order) so the ones already on screen are
/// the ones easiest to find - snapshotted on open so ticking one mid-click
/// does not shuffle the list under the pointer.
test("the Repositories picker lists checked options first when it opens", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/Web", JSON.stringify(["r2"]));
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos")
      return [
        { id: "r1", name: "web" },
        { id: "r2", name: "api" },
      ];
    if (cmd === "repo_pull_requests") return [];
  });
  renderPanel();
  await screen.findByText("Awaiting your review");

  const trigger = screen.getByLabelText("Repositories");
  fireEvent.click(trigger);
  const dropdown = trigger.parentElement!.querySelector(".t-dropdown")!;
  const labels = [...dropdown.querySelectorAll("li")].map((li) => li.textContent);
  // Your Pull Requests defaults on, and the tracked repo (api) is checked -
  // both come before the untracked one (web).
  expect(labels).toEqual(["Your Pull Requests", "api", "web"]);
});

test("a single-repo choice from before multi-select migrates silently", async () => {
  localStorage.setItem("tcm-v2-pr-repo:acme/Web", "r2");
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos")
      return [
        { id: "r1", name: "web" },
        { id: "r2", name: "api" },
      ];
    if (cmd === "repo_pull_requests")
      return (args as { repoId: string }).repoId === "r2" ? [pr(9, { repo: "api" })] : [];
  });
  renderPanel();
  // The old choice keeps working without the user re-picking anything.
  expect(await screen.findByText("Active on api")).toBeInTheDocument();
  expect(await screen.findByText("!9")).toBeInTheDocument();
});

test("deselecting Your Pull Requests hides the group and un-hides yours in repo sections", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [pr(20, { repo: "web" })] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "repo_pull_requests")
      return [pr(20, { repo: "web" }), pr(21, { repo: "web", author: "Kim" })];
  });
  renderPanel();
  // On by default: the group renders without anyone touching the picker.
  expect(await screen.findByText("Your pull requests")).toBeInTheDocument();

  fireEvent.click(screen.getByLabelText("Repositories"));
  await tickRepo("web");
  await screen.findByText("!21");
  // While the group is shown, your PR lives there and only there.
  expect(screen.getAllByText("!20")).toHaveLength(1);

  await tickRepo("Your Pull Requests");
  expect(screen.queryByText("Your pull requests")).not.toBeInTheDocument();
  // "Show all pull requests": yours now belongs to the repo's own list -
  // deduping against the hidden group would have made it vanish entirely.
  expect(screen.getAllByText("!20")).toHaveLength(1);
  expect(screen.getByText("Active on web")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-yours:acme/Web")).toBe("off");

  // And back on: the group returns, the repo section dedupes again.
  await tickRepo("Your Pull Requests");
  expect(await screen.findByText("Your pull requests")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-yours:acme/Web")).toBe("on");
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
  fireEvent.click(screen.getByLabelText("Repositories"));
  await tickRepo("web");
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

  expect(await screen.findByText("Work items")).toBeInTheDocument();
  const chip = screen.getByText("143783").closest("button")!;
  expect(chip).toHaveTextContent("Participants - department inconsistencies");
  expect(chip).toHaveTextContent("In Progress");
});

test("work items render inside their own right-hand panel, sibling to the description column, not nested in it", async () => {
  // Long enough to sit at the truncation cap, so "View more" renders and
  // the description's own lazy fetch (pr_description) can be exercised too.
  const longDescription = "Alpha ".repeat(70).trim();
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd as string);
    if (cmd === "pr_overview")
      return { awaiting: [pr(44, { description: longDescription })], mine: [] };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_work_items")
      return [
        {
          id: 555,
          work_item_type: "Task",
          title: "Wire up the panel",
          state: "Active",
          state_color: "b2b2b2",
          url: "https://example.invalid/wi/555",
        },
      ];
    if (cmd === "pr_description") return `${longDescription} ...and the rest of it.`;
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!44")).closest("[aria-expanded]")!);

  // Structure, not styling: the work-items panel and the description
  // column are the two children of one shared row container - siblings,
  // not one nested inside the other. A single-stack layout would also
  // satisfy "not nested inside", so what actually pins the two-column
  // change is that the shared parent has exactly these two children.
  const heading = await screen.findByText("Work items");
  const chip = screen.getByText("555").closest("button")!;
  const workItemsPanel = heading.parentElement!;
  expect(workItemsPanel).toContainElement(chip);
  const row = workItemsPanel.parentElement!;
  const siblings = Array.from(row.children) as HTMLElement[];
  expect(siblings).toContain(workItemsPanel);
  expect(siblings).toHaveLength(2);
  const descriptionColumn = siblings.find((el) => el !== workItemsPanel)!;
  expect(descriptionColumn).toContainElement(screen.getByText(longDescription, { exact: false }));
  expect(descriptionColumn).not.toContainElement(workItemsPanel);
  expect(workItemsPanel).not.toContainElement(descriptionColumn);

  // Laziness: expanding the row fetches work items, but must not also
  // reach for the full description - that only happens once its own
  // "View more" modal is opened, mirroring the work-items test above.
  expect(calls).not.toContain("pr_description");
  fireEvent.click(await screen.findByRole("button", { name: "View more" }));
  await screen.findByRole("dialog");
  expect(calls).toContain("pr_description");
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

// ---- "View more" and the full description ------------------------------

test("a short description shows no View more button", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(30, { description: "Fixes a typo." })], mine: [] };
    if (cmd === "list_repos") return [];
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!30")).closest("[aria-expanded]")!);

  await screen.findByText("Fixes a typo.");
  expect(screen.queryByRole("button", { name: "View more" })).not.toBeInTheDocument();
});

test("a description at the truncation cap shows View more, and the modal renders the full body from pr_description", async () => {
  // >= 380 chars: at the API's ~400-char cutoff, so it was almost
  // certainly cut mid-word even though jsdom can't tell us it overflows.
  const truncated = "Alpha ".repeat(70).trim();
  const full = `${truncated} ...and the rest of the story that only pr_description returns.`;
  let askedFor: unknown;
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return { awaiting: [pr(31, { description: truncated })], mine: [] };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_description") {
      askedFor = args;
      return full;
    }
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!31")).closest("[aria-expanded]")!);

  const more = await screen.findByRole("button", { name: "View more" });
  fireEvent.click(more);

  const dialog = await screen.findByRole("dialog");
  expect(await within(dialog).findByText(/rest of the story/)).toBeInTheDocument();
  expect(askedFor).toMatchObject({ organization: "acme", project: "Web", repo: "web", prId: 31 });
});

test("the modal shows the truncated text immediately, before the full description arrives", async () => {
  const truncated = "Beta ".repeat(80).trim();
  let resolveFull: (v: string) => void = () => {};
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(32, { description: truncated })], mine: [] };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_description") return new Promise<string>((res) => (resolveFull = res));
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!32")).closest("[aria-expanded]")!);
  fireEvent.click(await screen.findByRole("button", { name: "View more" }));

  // A slow network must never hand back an empty box: the text we already
  // have is on screen the instant the modal opens, before the fetch settles.
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(truncated, { exact: false })).toBeInTheDocument();

  resolveFull(`${truncated} plus everything that came after.`);
  expect(await within(dialog).findByText(/plus everything that came after/)).toBeInTheDocument();
});

test("when pr_description fails, the modal keeps the truncated text and says the full body could not load", async () => {
  const truncated = "Gamma ".repeat(70).trim();
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(33, { description: truncated })], mine: [] };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_description") throw new Error("network down");
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!33")).closest("[aria-expanded]")!);
  fireEvent.click(await screen.findByRole("button", { name: "View more" }));

  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText(truncated, { exact: false })).toBeInTheDocument();
  expect(await within(dialog).findByText(/could not be loaded/i)).toBeInTheDocument();
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
  fireEvent.click(screen.getByLabelText("Repositories"));
  await tickRepo("web");
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
          stages: [
            {
              name: "Stage", state: "completed", result: "succeeded",
              started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:05:00Z",
              jobs: [
                {
                  name: "Build_solution", state: "completed", result: "succeeded",
                  started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:05:00Z",
                  tasks: [
                    { name: "Run Unit Test", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:04:00Z", finished: "2026-07-24T09:05:00Z", issues: [] },
                  ],
                },
              ],
            },
          ],
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
  expect(screen.queryByText("Last Run Pipeline")).not.toBeInTheDocument();
  fireEvent.click((await screen.findByText("!42")).closest("[aria-expanded]")!);

  expect(await screen.findByText("Last Run Pipeline")).toBeInTheDocument();
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

test("View history opens a readable pipeline dialog with environments", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(42, { merge_commit: "abc123" })], mine: [] };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_work_items") return [];
    if (cmd === "pr_pipeline")
      return [
        {
          id: 901, name: "HRM-PMS-NET", number: "2026.7.24-12", status: "completed",
          result: "succeeded", is_validation: false,
          started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:05:30Z",
          web_url: "https://x/901",
          stages: [
            {
              name: "Stage", state: "completed", result: "succeeded",
              started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:05:00Z",
              jobs: [
                {
                  name: "Build_solution", state: "completed", result: "succeeded",
                  started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:05:00Z",
                  tasks: [
                    { name: "Run Unit Test", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:04:00Z", finished: "2026-07-24T09:05:00Z", issues: [] },
                  ],
                },
              ],
            },
          ],
          deployments: [
            { release: "Release-482", environment: "QA", status: "succeeded", on: "2026-07-24T10:00:00Z", web_url: "" },
            { release: "Release-482", environment: "Production", status: "notStarted", on: "", web_url: "" },
          ],
        },
      ];
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!42")).closest("[aria-expanded]")!);
  fireEvent.click(await screen.findByRole("button", { name: "View history" }));

  const dialog = await screen.findByRole("dialog");
  // The headline answers "how far did this get?" without reading the detail.
  expect(within(dialog).getByText(/Reached QA/)).toBeInTheDocument();
  expect(within(dialog).getByText("CI after merge")).toBeInTheDocument();
  // Duration is derived from start/finish.
  expect(within(dialog).getByText(/5m 30s/)).toBeInTheDocument();
  expect(within(dialog).getByText("Environments")).toBeInTheDocument();
  expect(within(dialog).getByText("not started")).toBeInTheDocument();

  fireEvent.click(within(dialog).getByRole("button", { name: "Close pipeline history" }));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});

test("a full page offers Load more, which fetches the next skip", async () => {
  const skips: number[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "repo_pull_requests") {
      const skip = (args as { skip: number }).skip;
      skips.push(skip);
      // Page 1 is full (25) -> a next page may exist; page 2 is short.
      return skip === 0
        ? Array.from({ length: 25 }, (_, i) => pr(100 + i, { author: "Kim" }))
        : [pr(200, { author: "Kim" })];
    }
  });
  renderPanel();
  fireEvent.click(screen.getByLabelText("Repositories"));
  await tickRepo("web");

  expect(await screen.findByText("!100")).toBeInTheDocument();
  const more = await screen.findByRole("button", { name: "Load more" });
  fireEvent.click(more);
  expect(await screen.findByText("!200")).toBeInTheDocument();
  expect(skips).toEqual([0, 25]);
  // The short second page means no further pages are offered.
  expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
});

test("a step's log opens in its own wide dialog with coloured lines", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(42, { merge_commit: "abc" })], mine: [] };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_work_items") return [];
    if (cmd === "build_log")
      return ["Starting: Run Unit Test", "##[error]3 tests failed", "Passed! - Failed: 0, Passed: 203"].join("\n");
    if (cmd === "pr_pipeline")
      return [
        {
          id: 901, name: "HRM-PMS-NET", number: "1", status: "completed",
          result: "failed", is_validation: false,
          started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:05:00Z", web_url: "",
          stages: [
            {
              name: "Build", state: "completed", result: "failed",
              started: "", finished: "",
              jobs: [
                {
                  name: "Build_solution", state: "completed", result: "failed",
                  started: "", finished: "",
                  tasks: [
                    { name: "Run Unit Test", state: "completed", result: "failed",
                      started: "", finished: "", issues: [], log_id: 42 },
                  ],
                },
              ],
            },
          ],
          deployments: [],
        },
      ];
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!42")).closest("[aria-expanded]")!);
  fireEvent.click(await screen.findByRole("button", { name: "View history" }));

  // Click the failing step inside the history dialog to open its log.
  const dialogs = await screen.findByRole("dialog");
  fireEvent.click(within(dialogs).getByTitle("Show this step's log"));

  expect(await screen.findByText(/3 tests failed/)).toBeInTheDocument();
  // Colour heuristics: the ##[error] line is red, the summary line green.
  expect(screen.getByText(/3 tests failed/).className).toContain("text-danger");
  expect(screen.getByText(/Failed: 0, Passed: 203/).className).toContain("text-success");
  expect(screen.getByText("Starting: Run Unit Test").className).toContain("text-accent");
});

test("a cached closed PR re-asks only the deployments and folds them in", async () => {
  // Seed the local cache: a completed PR whose CI build finished with no
  // deployments (nothing had been released when it was cached).
  const cachedBuild = {
    id: 901, name: "HRM-PMS-NET", number: "1", status: "completed",
    result: "succeeded", is_validation: false,
    started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:05:00Z", web_url: "",
    stages: [], deployments: [],
  };
  localStorage.setItem(
    "tcm-v2-cache:pipe:acme/Web:42:abc",
    JSON.stringify({ at: Date.now(), data: [cachedBuild] }),
  );

  const calls: string[] = [];
  mockIPC((cmd, args) => {
    calls.push(cmd as string);
    if (cmd === "pr_overview")
      return {
        awaiting: [pr(42, { status: "completed", merge_commit: "abc" })],
        mine: [],
      };
    if (cmd === "list_repos") return [];
    if (cmd === "pr_work_items") return [];
    if (cmd === "pr_deployments") {
      expect((args as { buildIds: number[] }).buildIds).toEqual([901]);
      // A release created AFTER the history was cached.
      return [
        {
          build_id: 901,
          deployments: [
            { release: "Release-500", environment: "Production", status: "succeeded",
              on: "2026-07-26T10:00:00Z", web_url: "" },
          ],
        },
      ];
    }
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!42")).closest("[aria-expanded]")!);

  // The late release shows up even though the pipeline came from cache...
  expect(await screen.findByText("Production")).toBeInTheDocument();
  // ...the expensive full-chain fetch never ran...
  expect(calls).not.toContain("pr_pipeline");
  expect(calls).toContain("pr_deployments");
  // ...and the cache itself was rebuilt with the new deployment.
  const stored = JSON.parse(localStorage.getItem("tcm-v2-cache:pipe:acme/Web:42:abc")!);
  expect(stored.data[0].deployments[0].release).toBe("Release-500");
});

// ---- The comments-to-resolve pill --------------------------------------

const thread = (id: number, status: string) => ({
  id,
  status,
  file_path: "src/app.ts",
  line: 3,
  comments: [{ id: 1, author: "Kim", content: "look here", published: "2026-08-01T10:00:00Z", is_system: false }],
  last_updated: "2026-08-01T10:00:00Z",
});

test("a row with unresolved review threads says so without being opened", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(1)], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "pr_threads")
      return [thread(1, "active"), thread(2, ""), thread(3, "fixed")];
  });
  renderPanel();

  // Two need someone: "active" and the empty status ADO sends for a
  // thread never resolved either way. "fixed" is settled and not counted.
  const row = (await screen.findByText("!1")).closest("button")!;
  expect(await within(row).findByText("2 comments to resolve")).toBeInTheDocument();
});

test("one unresolved thread reads in the singular", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(1)], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "pr_threads") return [thread(1, "active")];
  });
  renderPanel();
  const row = (await screen.findByText("!1")).closest("button")!;
  expect(await within(row).findByText("1 comment to resolve")).toBeInTheDocument();
});

test("a fully resolved conversation shows no pill at all", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(1)], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "pr_threads") return [thread(1, "fixed"), thread(2, "wontFix"), thread(3, "closed")];
  });
  renderPanel();
  await screen.findByText("!1");
  expect(screen.queryByText(/to resolve/)).not.toBeInTheDocument();
});

test("threads are not fetched eagerly for a completed PR", async () => {
  const asked: number[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview")
      return {
        awaiting: [pr(1)],
        mine: [pr(2, { status: "completed", closed: "2026-08-01T12:00:00Z" })],
      };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "pr_threads") {
      asked.push((args as { prId: number }).prId);
      return [];
    }
  });
  renderPanel();
  await screen.findByText("!1");
  // The active row may ask; the completed one must not - eager thread
  // calls across pages of closed PRs would multiply the panel's ADO
  // traffic for rows nobody still needs to act on.
  await new Promise((r) => setTimeout(r, 50));
  expect(asked).not.toContain(2);
});


/// The bell's handoff: the row the notification named expands itself,
/// scrolls into view and hands the focus back.
test("a focused pull request expands, scrolls into view and reports handled", async () => {
  const spy = vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(() => {});
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(7)], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "pr_threads") return [];
    if (cmd === "pr_work_items") return [];
    if (cmd === "pr_pipeline") return [];
  });
  const handled = vi.fn();
  renderPanel({ focus: { repo: "web", id: 7 }, onFocusHandled: handled });
  const row = await screen.findByRole("button", { name: /PR 7/, expanded: true });
  expect(row).toBeInTheDocument();
  expect(spy).toHaveBeenCalled();
  expect(handled).toHaveBeenCalledTimes(1);
  spy.mockRestore();
});

/// ...and when no group on the page could ever hold it, the panel says so
/// instead of leaving the user staring at an unchanged list.
test("a focused pull request that no listed group can hold says so", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
  });
  const handled = vi.fn();
  renderPanel({ focus: { repo: "billing", id: 99 }, onFocusHandled: handled });
  await vi.waitFor(() =>
    expect(toast.info).toHaveBeenCalledWith(
      "PR #99 is not listed here. Track the billing repository to see it.",
    ),
  );
  expect(handled).toHaveBeenCalledTimes(1);
});

/// ...and when it WOULD be held, just in the group the user turned off,
/// the toast has to say that instead - tracking a repo would not help.
test("a focused pull request that is one of yours, with the group hidden, says so", async () => {
  localStorage.setItem("tcm-v2-pr-yours:acme/Web", "off");
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [pr(99, { repo: "billing" })] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
  });
  const handled = vi.fn();
  renderPanel({ focus: { repo: "billing", id: 99 }, onFocusHandled: handled });
  await vi.waitFor(() =>
    expect(toast.info).toHaveBeenCalledWith(
      "PR #99 is one of yours. Turn on Your Pull Requests to see it.",
    ),
  );
  expect(handled).toHaveBeenCalledTimes(1);
});

test("a saved repository the fresh list no longer offers is not requested, and says why", async () => {
  // r9 was ticked once; the project's list no longer carries it (disabled,
  // or gone). The tick stays in storage for the person to remove.
  localStorage.setItem("tcm-v2-pr-repos:acme/Web", JSON.stringify(["r9"]));
  const asked: string[] = [];
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "repo_pull_requests") {
      asked.push((args as { repoId: string }).repoId);
      return [];
    }
  });
  renderPanel();

  expect(
    await screen.findByText(
      "A repository you tracked is disabled in Azure DevOps, or no longer in this project.",
    ),
  ).toBeInTheDocument();
  expect(asked).toEqual([]);
  expect(screen.queryByText(/Active on/)).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/Web")).toBe(JSON.stringify(["r9"]));
});

test("a failing repository section keeps its heading, and the others still render", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/Web", JSON.stringify(["r1", "r2"]));
  mockIPC((cmd, args) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos")
      return [
        { id: "r1", name: "web" },
        { id: "r2", name: "api" },
      ];
    if (cmd === "repo_pull_requests") {
      if ((args as { repoId: string }).repoId === "r1") throw { kind: "RepositoryUnavailable" };
      return [pr(12, { repo: "api" })];
    }
  });
  renderPanel();

  const failing = (await screen.findByText("Active on web")).closest("section")!;
  expect(
    await within(failing).findByText(
      "Azure DevOps says this repository is disabled or you cannot read it.",
    ),
  ).toBeInTheDocument();
  expect(await screen.findByText("Active on api")).toBeInTheDocument();
  expect(await screen.findByText("!12")).toBeInTheDocument();
});

test("Stop tracking removes every saved repository the list no longer offers, and the line goes", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/Web", JSON.stringify(["r1", "r8", "r9"]));
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
    if (cmd === "repo_pull_requests") return [];
  });
  renderPanel();

  const line = "A repository you tracked is disabled in Azure DevOps, or no longer in this project.";
  // Two missing, one line.
  expect(await screen.findAllByText(line)).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Stop tracking" }));
  expect(screen.queryByText(line)).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Stop tracking" })).not.toBeInTheDocument();
  // The one still on the list stays tracked.
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/Web")).toBe(JSON.stringify(["r1"]));
  expect(await screen.findByText("Active on web")).toBeInTheDocument();
});

test("when the repository list fails, the panel says so once instead of dropping the tracked sections silently", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/Web", JSON.stringify(["r1", "r2"]));
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [], mine: [] };
    if (cmd === "list_repos") throw { kind: "RepositoryUnavailable" };
  });
  renderPanel();

  expect(
    await screen.findAllByText("Azure DevOps says this repository is disabled or you cannot read it."),
  ).toHaveLength(1);
  // No "tracked but missing" line is invented from a list that never loaded.
  expect(screen.queryByText(/is disabled in Azure DevOps, or no longer/)).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/Web")).toBe(JSON.stringify(["r1", "r2"]));
});

// ---- the projects shown together --------------------------------------------

/** The home project only fills in for what names none; the lists are the
 * organization's. */
const together = { allProjects: true, project: "HRM" } as const;

const hrmPr = (id: number, over: Partial<Record<string, unknown>> = {}) =>
  pr(id, { project: "HRM", ...over });
const payrollPr = (id: number, over: Partial<Record<string, unknown>> = {}) =>
  pr(id, { project: "Payroll", repo: "payroll-api", repo_id: "repo-pay", ...over });
const repoRef = (id: string, name: string, project: string) => ({ id, name, project });

type Call = { cmd: string; args: Record<string, unknown> };

/** Answers the organization-wide overview and repository list; records every
 * command. `extra` answers the rest. */
function mockTogether(
  answer: {
    awaiting?: unknown[];
    mine?: unknown[];
    repos?: unknown[];
    coverage?: { shown: number; total: number };
  } = {},
  extra: (cmd: string, args: Record<string, unknown>) => unknown = () => undefined,
): Call[] {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    if (cmd === "pr_overview_all")
      return {
        awaiting: answer.awaiting ?? [],
        mine: answer.mine ?? [],
        coverage: answer.coverage ?? { shown: 0, total: 0 },
      };
    if (cmd === "list_repos_all") return answer.repos ?? [];
    return extra(cmd, a);
  });
  return calls;
}

test("the repositories tracked in the home project seed the combined key once", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/HRM", JSON.stringify(["r1"]));
  const calls = mockTogether({ repos: [repoRef("r1", "web", "HRM")] });
  renderPanel(together);
  await waitFor(() => expect(calls.some((c) => c.cmd === "pr_overview_all")).toBe(true));
  // Copied, never moved.
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/*")).toBe(JSON.stringify(["r1"]));
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/HRM")).toBe(JSON.stringify(["r1"]));
});

test("with the tour running, the repository seed is refused", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/HRM", JSON.stringify(["r1"]));
  setTourRunning(true);
  try {
    const calls = mockTogether({ repos: [repoRef("r1", "web", "HRM")] });
    renderPanel(together);
    await waitFor(() => expect(calls.some((c) => c.cmd === "pr_overview_all")).toBe(true));
    // The walkthrough runs on sample data and writes nothing of the person's.
    expect(localStorage.getItem("tcm-v2-pr-repos:acme/*")).toBeNull();
  } finally {
    setTourRunning(false);
  }
});

test("combined panel lists pull requests from every project", async () => {
  const calls = mockTogether({
    awaiting: [hrmPr(1)],
    mine: [payrollPr(2)],
    repos: [repoRef("r1", "web", "HRM"), repoRef("rp", "payroll-api", "Payroll")],
  });
  renderPanel(together);

  expect(await screen.findByText("!1")).toBeInTheDocument();
  expect(await screen.findByText("!2")).toBeInTheDocument();
  // One organization-wide read each, and nothing asks a single project.
  expect(calls.filter((c) => c.cmd === "pr_overview_all").map((c) => c.args)).toEqual([
    { organization: "acme" },
  ]);
  expect(calls.filter((c) => c.cmd === "list_repos_all").map((c) => c.args)).toEqual([
    { organization: "acme" },
  ]);
  expect(calls.some((c) => c.cmd === "pr_overview" || c.cmd === "list_repos")).toBe(false);
});

test("pull requests name their project only across two projects", async () => {
  mockTogether({ awaiting: [hrmPr(1)], mine: [hrmPr(2)] });
  const one = renderPanel(together);
  await screen.findByText("!2");
  expect(screen.queryByText("Project:", { exact: false })).not.toBeInTheDocument();
  one.unmount();

  mockTogether({ awaiting: [hrmPr(1)], mine: [payrollPr(2)] });
  renderPanel(together);
  const hrm = (await screen.findByText("!1")).closest("button")!;
  const pay = (await screen.findByText("!2")).closest("button")!;
  expect(within(hrm).getByText("HRM")).toBeInTheDocument();
  expect(within(pay).getByText("Payroll")).toBeInTheDocument();
});

test("repository sections name their project only across two projects", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/*", JSON.stringify(["r1", "rp"]));
  mockTogether(
    { repos: [repoRef("r1", "web", "HRM"), repoRef("rp", "payroll-api", "Payroll")] },
    (cmd, args) => {
      if (cmd === "repo_pull_requests")
        return args.repoId === "r1" ? [hrmPr(11)] : [payrollPr(12)];
    },
  );
  renderPanel(together);
  const web = (await screen.findByText("Active on web")).closest("section")!;
  const api = (await screen.findByText("Active on payroll-api")).closest("section")!;
  expect(within(web).getAllByText("HRM").length).toBeGreaterThan(0);
  expect(within(api).getAllByText("Payroll").length).toBeGreaterThan(0);
});

test("a pull request's detail is read in its own project", async () => {
  const calls = mockTogether(
    { awaiting: [hrmPr(1)], mine: [payrollPr(2)], repos: [repoRef("r1", "web", "HRM")] },
    (cmd) => {
      if (cmd === "pr_threads") return [];
      if (cmd === "pr_work_items") return [];
      if (cmd === "pr_pipeline") return [];
      if (cmd === "pr_build_states") return [];
    },
  );
  renderPanel(together);
  const pay = (await screen.findByText("!2")).closest("button")!;
  fireEvent.click(pay);

  await vi.waitFor(() => {
    expect(calls.some((c) => c.cmd === "pr_work_items" && c.args.prId === 2)).toBe(true);
    expect(calls.some((c) => c.cmd === "pr_pipeline" && c.args.prId === 2)).toBe(true);
  });
  // Every per-pull-request read carries the project of that pull request.
  const projectOf = (cmd: string, prId: number) =>
    calls.filter((c) => c.cmd === cmd && c.args.prId === prId).map((c) => c.args.project);
  for (const cmd of ["pr_threads", "pr_work_items", "pr_pipeline"]) {
    expect(new Set(projectOf(cmd, 2))).toEqual(new Set(["Payroll"]));
  }
  expect(new Set(projectOf("pr_threads", 1))).toEqual(new Set(["HRM"]));
  // Build states are asked per repository, in that repository's project.
  const states = calls.filter((c) => c.cmd === "pr_build_states");
  expect(states.map((c) => [c.args.repoId, c.args.project]).sort()).toEqual([
    ["repo-guid-1", "HRM"],
    ["repo-pay", "Payroll"],
  ]);
});

test("resolving a thread writes to the pull request's project", async () => {
  const calls = mockTogether({ mine: [payrollPr(2)] }, (cmd) => {
    if (cmd === "pr_threads") return [thread(5, "active")];
    if (cmd === "pr_work_items") return [];
    if (cmd === "pr_pipeline") return [];
    if (cmd === "set_pr_thread_status") return "fixed";
  });
  renderPanel(together);
  fireEvent.click((await screen.findByText("!2")).closest("button")!);

  fireEvent.click(await screen.findByRole("button", { name: "Resolve" }));
  await vi.waitFor(() => expect(calls.some((c) => c.cmd === "set_pr_thread_status")).toBe(true));
  expect(calls.filter((c) => c.cmd === "set_pr_thread_status").map((c) => c.args)).toEqual([
    {
      organization: "acme",
      project: "Payroll",
      repo: "payroll-api",
      prId: 2,
      threadId: 5,
      status: "fixed",
    },
  ]);
});

test("a pull request that names no project is read in the home project", async () => {
  const calls = mockTogether({ mine: [pr(3, { project: "" })] }, (cmd) => {
    if (cmd === "pr_threads") return [];
  });
  renderPanel(together);
  await screen.findByText("!3");
  await vi.waitFor(() => expect(calls.some((c) => c.cmd === "pr_threads")).toBe(true));
  expect(new Set(calls.filter((c) => c.cmd === "pr_threads").map((c) => c.args.project))).toEqual(
    new Set(["HRM"]),
  );
});

test("two repositories with one name are told apart by project", async () => {
  const calls = mockTogether(
    {
      repos: [
        repoRef("r1", "web", "HRM"),
        repoRef("r2", "web", "Payroll"),
        repoRef("r3", "api", "HRM"),
      ],
    },
    (cmd) => {
      if (cmd === "repo_pull_requests") return [];
    },
  );
  renderPanel(together);
  await vi.waitFor(() => expect(calls.some((c) => c.cmd === "list_repos_all")).toBe(true));

  const trigger = screen.getByLabelText("Repositories");
  fireEvent.click(trigger);
  await vi.waitFor(() => {
    const dropdown = trigger.parentElement!.querySelector(".t-dropdown")!;
    const labels = [...dropdown.querySelectorAll("li")].map((li) => li.textContent);
    // Only the clashing name is qualified.
    expect(labels).toEqual(["Your Pull Requests", "web (HRM)", "web (Payroll)", "api"]);
  });

  // The label maps back to the right repository: its id is stored and
  // its pull requests are asked in its own project.
  await tickRepo("web (Payroll)");
  expect(await screen.findByText("Active on web (Payroll)")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/*")).toBe(JSON.stringify(["r2"]));
  await vi.waitFor(() =>
    expect(calls.filter((c) => c.cmd === "repo_pull_requests").map((c) => [c.args.project, c.args.repoId])).toEqual([
      ["Payroll", "r2"],
    ]),
  );
});

test("combined tracked repositories seed once from the home project", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/HRM", JSON.stringify(["r1"]));
  localStorage.setItem("tcm-v2-pr-yours:acme/HRM", "off");
  mockTogether({ repos: [repoRef("r1", "web", "HRM"), repoRef("r2", "api", "Payroll")] }, (cmd) => {
    if (cmd === "repo_pull_requests") return [];
  });
  const first = renderPanel(together);

  // Copied across, and the home project's own keys are exactly as they were.
  expect(await screen.findByText("Active on web")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/*")).toBe(JSON.stringify(["r1"]));
  expect(localStorage.getItem("tcm-v2-pr-yours:acme/*")).toBe("off");
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/HRM")).toBe(JSON.stringify(["r1"]));
  expect(localStorage.getItem("tcm-v2-pr-yours:acme/HRM")).toBe("off");
  expect(screen.queryByText("Your pull requests")).not.toBeInTheDocument();

  // Changing the choice writes the combined key and leaves the home project's alone.
  fireEvent.click(screen.getByLabelText("Repositories"));
  await tickRepo("api");
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/*")).toBe(JSON.stringify(["r1", "r2"]));
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/HRM")).toBe(JSON.stringify(["r1"]));
  first.unmount();

  // A second visit never overwrites it, even when the home project's key has since moved.
  localStorage.setItem("tcm-v2-pr-repos:acme/HRM", JSON.stringify(["r9"]));
  localStorage.setItem("tcm-v2-pr-yours:acme/HRM", "on");
  renderPanel(together);
  expect(await screen.findByText("Active on api")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/*")).toBe(JSON.stringify(["r1", "r2"]));
  expect(localStorage.getItem("tcm-v2-pr-yours:acme/*")).toBe("off");
});

test("a profile with only the old single-repository key is carried over too", async () => {
  // A raw id, not JSON - the one key a plain key copy cannot carry.
  localStorage.setItem("tcm-v2-pr-repo:acme/HRM", "r2");
  mockTogether({ repos: [repoRef("r1", "web", "HRM"), repoRef("r2", "api", "HRM")] }, (cmd) => {
    if (cmd === "repo_pull_requests") return [];
  });
  renderPanel(together);

  expect(await screen.findByText("Active on api")).toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/*")).toBe(JSON.stringify(["r2"]));
  expect(localStorage.getItem("tcm-v2-pr-repo:acme/HRM")).toBe("r2");
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/HRM")).toBeNull();
});

test("nothing saved for the home project leaves the combined keys unwritten", async () => {
  mockTogether({ repos: [repoRef("r1", "web", "HRM")] });
  renderPanel(together);
  await screen.findByText("Awaiting your review");
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/*")).toBeNull();
  expect(localStorage.getItem("tcm-v2-pr-yours:acme/*")).toBeNull();
});

test("projects left out are reported", async () => {
  mockTogether({ awaiting: [hrmPr(1)], coverage: { shown: 3, total: 5 } });
  renderPanel(together);
  expect(
    await screen.findByText("Showing pull requests from 3 of 5 projects."),
  ).toBeInTheDocument();
});

test("nothing is said when every project was read", async () => {
  mockTogether({ awaiting: [hrmPr(1)], coverage: { shown: 5, total: 5 } });
  renderPanel(together);
  await screen.findByText("!1");
  expect(screen.queryByText(/Showing pull requests from/)).not.toBeInTheDocument();
});

test("a tracked repository the list no longer offers says organization, not project", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/*", JSON.stringify(["r9"]));
  mockTogether({ repos: [repoRef("r1", "web", "HRM")] });
  renderPanel(together);
  expect(
    await screen.findByText(
      "A repository you tracked is disabled in Azure DevOps, or no longer in this organization.",
    ),
  ).toBeInTheDocument();
});

test("the refresh button reloads the organization's overview", async () => {
  const calls = mockTogether({ awaiting: [hrmPr(1)] });
  renderPanel(together);
  await screen.findByText("!1");
  const before = calls.filter((c) => c.cmd === "pr_overview_all").length;
  fireEvent.click(screen.getByLabelText("Refresh pull requests"));
  await vi.waitFor(() =>
    expect(calls.filter((c) => c.cmd === "pr_overview_all").length).toBeGreaterThan(before),
  );
});

test("a notification for a same-named repository in another project is not claimed here", async () => {
  localStorage.setItem("tcm-v2-pr-repos:acme/*", JSON.stringify(["r1"]));
  mockTogether({ repos: [repoRef("r1", "web", "HRM"), repoRef("r2", "web", "Payroll")] }, (cmd) => {
    if (cmd === "repo_pull_requests") return [];
  });
  const handled = vi.fn();
  // Payroll's web is not tracked; HRM's is. The name alone must not count.
  renderPanel({ ...together, focus: { repo: "web", id: 99, project: "Payroll" }, onFocusHandled: handled });
  await vi.waitFor(() =>
    expect(toast.info).toHaveBeenCalledWith(
      "PR #99 is not listed here. Track the web repository to see it.",
    ),
  );
  expect(handled).toHaveBeenCalledTimes(1);
});

test("switched off the panel asks the picked project as before", async () => {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === "pr_overview") return { awaiting: [pr(1)], mine: [] };
    if (cmd === "list_repos") return [{ id: "r1", name: "web" }];
  });
  renderPanel({ allProjects: false, project: "Web" });
  await screen.findByText("!1");
  await vi.waitFor(() => expect(calls.some((c) => c.cmd === "list_repos")).toBe(true));

  expect(calls.find((c) => c.cmd === "pr_overview")!.args).toEqual({
    organization: "acme",
    project: "Web",
  });
  expect(calls.find((c) => c.cmd === "list_repos")!.args).toEqual({
    organization: "acme",
    project: "Web",
  });
  expect(calls.some((c) => c.cmd === "pr_overview_all" || c.cmd === "list_repos_all")).toBe(false);
  // Nothing names a project while the projects are apart, and the keys are the per-project ones.
  expect(screen.queryByText("Project:", { exact: false })).not.toBeInTheDocument();
  expect(localStorage.getItem("tcm-v2-pr-repos:acme/*")).toBeNull();
});
