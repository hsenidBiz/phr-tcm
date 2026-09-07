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

test("work items render inside their own right-hand panel, not the description column", async () => {
  mockIPC((cmd) => {
    if (cmd === "pr_overview") return { awaiting: [pr(44)], mine: [] };
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
  });
  renderPanel();
  fireEvent.click((await screen.findByText("!44")).closest("[aria-expanded]")!);

  // Structure, not styling: the heading and the chip sit in the same
  // container, and that container is not the one the description lives in.
  const heading = await screen.findByText("Work items");
  const panel = heading.parentElement!;
  const chip = screen.getByText("555").closest("button")!;
  expect(panel).toContainElement(chip);
  expect(panel).not.toContainElement(screen.getByText("Why this change exists"));
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

