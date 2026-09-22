import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Suites, { pointsIndent } from "./Suites";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderSuites(
  onEditCases?: (label: string, ids: number[]) => void,
  // Accepts a caller-supplied QueryClient so a test can pre-seed the cache
  // (qc.setQueryData) before the component ever mounts.
  qc: QueryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
  onManageSuite?: (planId: number, suiteId: number) => void,
  onSetCurrentPbi?: (p: { id: number; title: string }) => void,
) {
  return render(
    <QueryClientProvider client={qc}>
      <Suites
        org="acme"
        project="Web"
        onEditCases={onEditCases}
        onManageSuite={onManageSuite}
        onSetCurrentPbi={onSetCurrentPbi}
      />
    </QueryClientProvider>,
  );
}

const PLAN = { id: 9, name: "Auth - Test Plan", area_path: "Proj\\Auth", root_suite_id: 90 };

function baseMock(handler: (cmd: string, args: unknown) => unknown) {
  mockIPC((cmd, args) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    return handler(cmd, args);
  });
}

test("plans render with suites; a suite click shows its points", async () => {
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            {
              id: 91,
              name: "PBI 42 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 42,
              parent_id: null,
            },
          ],
        },
      ];
    if (cmd === "list_test_points" && (args as { suiteId: number }).suiteId === 91)
      return [
        {
          point_id: 7,
          test_case_id: 201,
          test_case_name: "Valid login",
          config_name: "Windows 10",
          tester: "",
          last_outcome: "passed",
          last_run_id: null,
          last_result_id: null,
        },
      ];
  });
  renderSuites();

  expect(await screen.findByText("Auth - Test Plan")).toBeInTheDocument();
  expect(screen.getByText("PBI 42 suite").closest("li")!.className).toContain("cv-row");
  fireEvent.click(screen.getByText("PBI 42 suite"));
  expect(await screen.findByText("Valid login")).toBeInTheDocument();
  expect(screen.getByText("Passed")).toBeInTheDocument(); // capitalized display
});

test("Manage hands the suite to Suite Management", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            {
              id: 91,
              name: "PBI 42 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 42,
              parent_id: null,
            },
          ],
        },
      ];
  });
  const managed: Array<[number, number]> = [];
  renderSuites(undefined, undefined, (planId, suiteId) => managed.push([planId, suiteId]));

  await screen.findByText("PBI 42 suite");
  // Manage is one of the extra options behind the row's More chip.
  expect(screen.queryByRole("button", { name: "Manage" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "More actions for PBI 42 suite" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Manage" }));
  expect(managed).toEqual([[9, 91]]);
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

const REQUIREMENT_SUITE_PLANS = [
  {
    plan: PLAN,
    suites: [{ id: 91, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null }],
  },
];

test("Use as current PBI resolves the PBI's own title and hands it up without leaving the screen", async () => {
  const queries: string[] = [];
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites") return REQUIREMENT_SUITE_PLANS;
    if (cmd === "search_pbis") {
      queries.push((args as { query: string }).query);
      return [{ id: 42, title: "Real PBI title", work_item_type: "Product Backlog Item" }];
    }
  });
  const picked: unknown[] = [];
  renderSuites(undefined, undefined, undefined, (p) => picked.push(p));
  await screen.findByText("PBI 42 suite");
  fireEvent.click(screen.getByRole("button", { name: "More actions for PBI 42 suite" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Use as current PBI" }));
  await waitFor(() => expect(picked).toEqual([{ id: 42, title: "Real PBI title" }]));
  expect(queries).toEqual(["42"]);
  expect(screen.getByRole("button", { name: "More actions for PBI 42 suite" })).toBeInTheDocument(); // still here
});

test("when the lookup finds nothing the suite's own name stands in", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites") return REQUIREMENT_SUITE_PLANS;
    if (cmd === "search_pbis") return [];
  });
  const picked: unknown[] = [];
  renderSuites(undefined, undefined, undefined, (p) => picked.push(p));
  await screen.findByText("PBI 42 suite");
  fireEvent.click(screen.getByRole("button", { name: "More actions for PBI 42 suite" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Use as current PBI" }));
  await waitFor(() => expect(picked).toEqual([{ id: 42, title: "PBI 42 suite" }]));
});

test("a static suite offers no Use as current PBI", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [{ plan: PLAN, suites: [{ id: 93, name: "Sprint stories", suite_type: "staticTestSuite", requirement_id: null, parent_id: null }] }];
  });
  renderSuites(() => {}, undefined, () => {}, () => {});
  const row = (await screen.findByText("Sprint stories")).closest("button")!;
  fireEvent.mouseEnter(within(row).getByRole("button", { name: "More actions for Sprint stories" }));
  const menu = screen.getByRole("menu", { name: "More actions for Sprint stories" });
  expect(within(menu).getAllByRole("menuitem").map((m) => m.textContent)).toEqual(["Manage", "Run Tests", "Report"]);
});

test("a folder row carries Manage too - a static suite can hold cases as well as child suites", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 95, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            {
              id: 96,
              name: "PBI 50 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 50,
              parent_id: 95,
            },
          ],
        },
      ];
  });
  const managed: Array<[number, number]> = [];
  renderSuites(undefined, undefined, (planId, suiteId) => managed.push([planId, suiteId]));

  const folderRow = await screen.findByText("Regression");
  const row = folderRow.closest("button")!;
  fireEvent.click(within(row).getByRole("button", { name: "More actions for Regression" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Manage" }));
  expect(managed).toEqual([[9, 95]]);
  // Picking from the menu must not also fold or unfold the row under it:
  // the menu's clicks bubble through the row in React's tree.
  expect(screen.queryByText("PBI 50 suite")).not.toBeInTheDocument();
});

test("folders build a collapsible tree from parent links", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 95, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            {
              id: 96,
              name: "PBI 50 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 50,
              parent_id: 95,
            },
          ],
        },
      ];
  });
  renderSuites();

  expect(await screen.findByText("Regression")).toBeInTheDocument();
  // Folders start collapsed; clicking expands, clicking again re-collapses.
  expect(screen.queryByText("PBI 50 suite")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText("Regression"));
  expect(screen.getByText("PBI 50 suite")).toBeInTheDocument();
  // The children list carries the guide lines, hung off the parent's
  // chevron centre: 8px pad + 0 levels + half a 14px chevron.
  const children = screen.getByText("PBI 50 suite").closest("ul")!;
  expect(children).toHaveClass("suite-tree");
  expect(children.style.getPropertyValue("--tree-x")).toBe("15px");
  fireEvent.click(screen.getByText("Regression"));
  expect(screen.queryByText("PBI 50 suite")).not.toBeInTheDocument();
});

test("folder Edit cases collects descendant case ids and hands off", async () => {
  const onEdit = vi.fn();
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 95, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            {
              id: 96,
              name: "PBI 50 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 50,
              parent_id: 95,
            },
          ],
        },
      ];
    if (cmd === "list_test_points") {
      const sid = (args as { suiteId: number }).suiteId;
      if (sid === 95) return [];
      if (sid === 96)
        return [
          {
            point_id: 7,
            test_case_id: 201,
            test_case_name: "Valid login",
            config_name: "W10",
            tester: "",
            last_outcome: "",
            last_run_id: null,
            last_result_id: null,
          },
        ];
      return [];
    }
  });
  renderSuites(onEdit);

  await screen.findByText("Regression");
  const editChips = screen.getAllByText("Edit cases");
  fireEvent.click(editChips[0]); // the folder's chip (folder row renders first)
  await vi.waitFor(() => expect(onEdit).toHaveBeenCalledWith("Regression", [201]));
});

test("search filters the tree; results start collapsed and open on demand", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 95, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            {
              id: 96,
              name: "PBI 50 suite",
              suite_type: "requirementTestSuite",
              requirement_id: 50,
              parent_id: 95,
            },
            {
              id: 97,
              name: "Smoke pack",
              suite_type: "staticTestSuite",
              requirement_id: null,
              parent_id: null,
            },
          ],
        },
      ];
  });
  renderSuites();
  await screen.findByText("Regression");

  // A nested match keeps its pruned ancestor folder and hides the rest -
  // COLLAPSED. A query for one suite should show the matching branches,
  // not unfold every plan's tree.
  fireEvent.change(screen.getByLabelText("Search suites"), { target: { value: "PBI 50" } });
  expect(screen.getByText("Regression")).toBeInTheDocument();
  expect(screen.queryByText("PBI 50 suite")).not.toBeInTheDocument();
  expect(screen.queryByText("Smoke pack")).not.toBeInTheDocument();

  // Opening the folder reveals the match; closing folds it again.
  fireEvent.click(screen.getByText("Regression"));
  expect(screen.getByText("PBI 50 suite")).toBeInTheDocument();
  fireEvent.click(screen.getByText("Regression"));
  expect(screen.queryByText("PBI 50 suite")).not.toBeInTheDocument();

  fireEvent.change(screen.getByLabelText("Search suites"), { target: { value: "zzz" } });
  expect(await screen.findByText(/Nothing matches "zzz"/)).toBeInTheDocument();
});

/// Every suite row can hand out ADO's own deep link - opened or copied -
/// so pointing a teammate at a suite no longer means describing the path.
/// A static suite has no PBI, so it never had a Run path - its cases were
/// viewable but not runnable. The Run chip hands the runner a suite-scoped
/// session: pbi id 0 (nothing to link), the suite's name as the title, and
/// the suite's cases as ordered caseIds.
test("a static suite's Run chip opens a suite-scoped runner session", async () => {
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            {
              id: 93,
              name: "Sprint stories",
              suite_type: "staticTestSuite",
              requirement_id: null,
              parent_id: null,
            },
          ],
        },
      ];
    if (cmd === "list_test_points" && (args as { suiteId: number }).suiteId === 93)
      return [301, 302].map((id, i) => ({
        point_id: i + 1,
        test_case_id: id,
        test_case_name: `Case ${id}`,
        config_name: "Windows 10",
        tester: "",
        last_outcome: "",
        last_run_id: null,
        last_result_id: null,
      }));
  });
  renderSuites();
  await screen.findByText("Sprint stories");

  fireEvent.click(screen.getByRole("button", { name: "More actions for Sprint stories" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Run Tests" }));
  await vi.waitFor(() => {
    const raw = localStorage.getItem("tcm-v2-runner-session");
    expect(raw).toBeTruthy();
  });
  const session = JSON.parse(localStorage.getItem("tcm-v2-runner-session") as string);
  expect(session.pbi).toEqual({ id: 0, title: "Sprint stories", work_item_type: "" });
  expect(session.caseIds).toEqual([301, 302]);
  expect(session.planId).toBe(9);
  expect(session.suiteId).toBe(93);
});

test("a suite row copies its Azure DevOps link", async () => {
  // copyText goes through the Tauri clipboard plugin first - capture that
  // invoke rather than the navigator fallback (same as AiBridge's test).
  let copied = "";
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 95, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
          ],
        },
      ];
    if (String(cmd).startsWith("plugin:clipboard-manager|")) {
      copied = JSON.stringify(args);
      return null;
    }
  });
  renderSuites();
  await screen.findByText("Regression");

  fireEvent.click(screen.getByLabelText("Copy link to Regression"));
  await vi.waitFor(() =>
    expect(copied).toContain(
      `https://dev.azure.com/acme/Web/_testPlans/define?planId=${PLAN.id}&suiteId=95`,
    ),
  );
  expect(screen.getByLabelText("Open Regression in Azure DevOps")).toBeInTheDocument();
});

test("empty project shows the friendly message", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites") return [];
  });
  renderSuites();
  expect(
    await screen.findByText(/No test plans with test suites in this project yet/),
  ).toBeInTheDocument();
});

/// A background revalidation must not throw the scan bar over a tree the
/// user is already working with - the toolbar spinner is that signal.
test("the scan progress bar shows only while there is no tree yet", async () => {
  const plansFixture = [
    {
      plan: PLAN,
      suites: [
        { id: 91, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null },
      ],
    },
  ];
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  // Seed the cache as a disk-seeded session would: data present, but old
  // enough to be past the 6h staleMs, so mounting still fires a background
  // refetch (isFetching true) even though the tree is already on screen.
  qc.setQueryData(["plans-suites", "acme", "Web"], plansFixture, {
    updatedAt: Date.now() - 7 * 60 * 60_000,
  });
  let release!: () => void;
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return new Promise((res) => {
        release = () => res(plansFixture);
      });
    if (cmd === "list_test_points") return [];
  });
  renderSuites(undefined, qc);

  // The tree renders from cache while the refetch is in flight...
  expect(await screen.findByText("Auth - Test Plan")).toBeInTheDocument();
  // ...and the scan bar must NOT be over it.
  expect(screen.queryByText(/Scanning test plans|Loading test plans/)).not.toBeInTheDocument();
  release();
});

/// Manage, Run Tests and Report are extra options: one chip holds them, so
/// the row shows View and Edit cases, and the chip opens on hover as well as
/// on click.
test("a row shows View and Edit cases, with Manage, Run Tests and Report behind More", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 93, name: "Sprint stories", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
          ],
        },
      ];
  });
  renderSuites(() => {}, undefined, () => {});
  const name = await screen.findByText("Sprint stories");
  const row = name.closest("button")!;
  expect(within(row).getByText("View")).toBeInTheDocument();
  expect(within(row).getByText("Edit cases")).toBeInTheDocument();
  for (const gone of ["Manage", "Run", "Run Tests", "Report"]) {
    expect(within(row).queryByText(gone)).not.toBeInTheDocument();
  }

  const more = within(row).getByRole("button", { name: "More actions for Sprint stories" });
  fireEvent.mouseEnter(more);
  const menu = screen.getByRole("menu", { name: "More actions for Sprint stories" });
  expect(within(menu).getAllByRole("menuitem").map((m) => m.textContent)).toEqual(["Manage", "Run Tests", "Report"]);
});

/// Field report: in a deep tree the guide lines ran through an opened
/// suite's case ids. The table sat a fixed 32px in, while every level moves
/// the guides 18px further right.
test("an opened suite's cases sit past every guide line above them, however deep", async () => {
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 1, name: "Root folder", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            { id: 2, name: "Mid folder", suite_type: "staticTestSuite", requirement_id: null, parent_id: 1 },
            { id: 3, name: "Regression", suite_type: "staticTestSuite", requirement_id: null, parent_id: 2 },
            { id: 4, name: "Sibling", suite_type: "staticTestSuite", requirement_id: null, parent_id: 2 },
          ],
        },
      ];
    if (cmd === "list_test_points" && (args as { suiteId: number }).suiteId === 3)
      return [
        {
          point_id: 1,
          test_case_id: 81165,
          test_case_name: "CMS | Menu Page Validation",
          config_name: "Windows 10",
          tester: "",
          last_outcome: "Passed",
          last_run_id: null,
          last_result_id: null,
        },
      ];
  });
  renderSuites();
  fireEvent.click(await screen.findByText("Root folder"));
  fireEvent.click(await screen.findByText("Mid folder"));
  fireEvent.click(await screen.findByText("Regression"));
  const table = await screen.findByRole("table", { name: "Test points in Regression" });

  // Regression is two levels down. The rightmost guide beside it hangs off
  // its parent's chevron: 8px + 18px per level + half the 14px chevron.
  const depth = 2;
  const deepestGuide = 8 + (depth - 1) * 18 + 7;
  const inset = parseFloat(table.style.marginLeft);
  expect(inset).toBe(pointsIndent(depth));
  expect(inset).toBeGreaterThan(deepestGuide);
  // And the table still fits its row.
  expect(table.style.width).toBe(`calc(100% - ${inset}px)`);
  expect(within(table).getByText("Windows 10")).toHaveClass("whitespace-nowrap");
});
