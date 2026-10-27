import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { afterEach, expect, test, vi } from "vitest";
import Suites, { pointsIndent } from "./Suites";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderSuites(
  onEditCases?: (label: string, ids: number[], project: string) => void,
  // Accepts a caller-supplied QueryClient so a test can pre-seed the cache
  // (qc.setQueryData) before the component ever mounts.
  qc: QueryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
  onManageSuite?: (planId: number, suiteId: number, project: string) => void,
  onSetCurrentPbi?: (p: { id: number; title: string; project: string }) => void,
) {
  return render(
    <QueryClientProvider client={qc}>
      <Suites
        org="acme"
        project="Web"
        allProjects={false}
        onEditCases={onEditCases}
        onManageSuite={onManageSuite}
        onSetCurrentPbi={onSetCurrentPbi}
      />
    </QueryClientProvider>,
  );
}

/** The projects-together render: the screen lists every project of the
 * organization, and `project` is only the home project it falls back on. */
function renderCombined(
  props: Partial<ComponentProps<typeof Suites>> = {},
  qc: QueryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  return render(
    <QueryClientProvider client={qc}>
      <Suites org="acme" project="Web" allProjects {...props} />
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
  await waitFor(() => expect(picked).toEqual([{ id: 42, title: "Real PBI title", project: "Web" }]));
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
  await waitFor(() => expect(picked).toEqual([{ id: 42, title: "PBI 42 suite", project: "Web" }]));
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
  await vi.waitFor(() => expect(onEdit).toHaveBeenCalledWith("Regression", [201], "Web"));
});

test("the header holds the title and refresh; the search sits alone in the wide toolbar below", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 91, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null },
          ],
        },
      ];
  });
  renderSuites();
  await screen.findByText("PBI 42 suite");
  expect(screen.getByRole("heading", { name: "Test plans" })).toBeInTheDocument();
  const refresh = screen.getByRole("button", { name: "Refresh test plans" });
  const search = screen.getByLabelText("Search suites");
  const toolbar = search.parentElement!.parentElement!.parentElement as HTMLElement;
  expect(toolbar).toHaveClass("flex-wrap");
  expect(toolbar).not.toContainElement(refresh);
  expect(search.parentElement!.parentElement).toHaveClass("flex-1", "min-w-64");
});

test("search finds a suite by its id or PBI id, and a plan word plus a suite word narrow together", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 96, name: "Alpha suite", suite_type: "requirementTestSuite", requirement_id: 50, parent_id: null },
            { id: 97, name: "Beta suite", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
          ],
        },
      ];
  });
  renderSuites();
  await screen.findByText("Alpha suite");
  const box = screen.getByLabelText("Search suites");
  expect(box).toHaveAttribute("placeholder", "Search by plan, suite, suite id, PBI id");

  fireEvent.change(box, { target: { value: "#97" } });
  expect(screen.getByText("Beta suite")).toBeInTheDocument();
  expect(screen.queryByText("Alpha suite")).not.toBeInTheDocument();

  fireEvent.change(box, { target: { value: "#50" } });
  expect(screen.getByText("Alpha suite")).toBeInTheDocument();
  expect(screen.queryByText("Beta suite")).not.toBeInTheDocument();

  // The plan's name is "Auth - Test Plan": its word and a suite word together.
  fireEvent.change(box, { target: { value: "auth beta" } });
  expect(screen.getByText("Beta suite")).toBeInTheDocument();
  expect(screen.queryByText("Alpha suite")).not.toBeInTheDocument();
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
  expect(session.pbi).toEqual({ id: 0, title: "Sprint stories", work_item_type: "", project: "Web" });
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
  // Every row carries the same chips, so each is named for its suite - and
  // the name starts with the words on the chip, for voice control.
  expect(within(row).getByRole("button", { name: "View Sprint stories" })).toBeInTheDocument();
  expect(within(row).getByRole("button", { name: "Edit cases in Sprint stories" })).toBeInTheDocument();
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

/// Paused and Not Applicable are different verdicts and must not read as
/// the same grey: Paused is the muted tone, Not Applicable the theme's ink
/// (the colours the runner gives each).
test("a Paused point and a Not Applicable point are not the same colour", async () => {
  const point = (id: number, last_outcome: string) => ({
    point_id: id,
    test_case_id: 300 + id,
    test_case_name: `Case ${id}`,
    config_name: "Windows 10",
    tester: "",
    last_outcome,
    last_run_id: null,
    last_result_id: null,
  });
  baseMock((cmd, args) => {
    if (cmd === "list_plans_with_suites")
      return [
        {
          plan: PLAN,
          suites: [
            { id: 91, name: "PBI 42 suite", suite_type: "requirementTestSuite", requirement_id: 42, parent_id: null },
          ],
        },
      ];
    if (cmd === "list_test_points" && (args as { suiteId: number }).suiteId === 91)
      return [point(1, "Paused"), point(2, "NotApplicable")];
  });
  renderSuites();
  fireEvent.click(await screen.findByText("PBI 42 suite"));

  const paused = await screen.findByText("Paused");
  const notApplicable = screen.getByText("Not Applicable");
  expect(paused).toHaveClass("text-muted");
  expect(notApplicable).toHaveClass("text-text");
  expect(notApplicable).not.toHaveClass("text-muted");
});

// ---- Projects together -------------------------------------------------

const WEB_PLAN = { id: 9, name: "Auth - Test Plan", area_path: "Web\\Auth", root_suite_id: 90 };
const PAY_PLAN = { id: 12, name: "Payslip - Test Plan", area_path: "Payroll\\Slips", root_suite_id: 120 };
const WEB_SUITES = [
  { id: 91, name: "Login checks", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
];
const PAY_SUITES = [
  { id: 121, name: "Login checks", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
  { id: 122, name: "PBI 77 suite", suite_type: "requirementTestSuite", requirement_id: 77, parent_id: null },
];
const POINT = {
  point_id: 7,
  test_case_id: 301,
  test_case_name: "Pay a slip",
  config_name: "W10",
  tester: "",
  last_outcome: "",
  last_run_id: null,
  last_result_id: null,
};

/** Two projects, each with its own plan; `extra` answers anything else and
 * every command asked is recorded with its arguments. */
function twoProjects(extra?: (cmd: string, args: Record<string, unknown>) => unknown) {
  const asked: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  baseMock((cmd, raw) => {
    const args = (raw ?? {}) as Record<string, unknown>;
    asked.push({ cmd, args });
    if (cmd === "list_projects")
      return [
        { id: "p1", name: "Web" },
        { id: "p2", name: "Payroll" },
      ];
    if (cmd === "list_plans_with_suites")
      return args.project === "Payroll"
        ? [{ plan: PAY_PLAN, suites: PAY_SUITES }]
        : [{ plan: WEB_PLAN, suites: WEB_SUITES }];
    return extra?.(cmd, args);
  });
  return asked;
}

test("combined Search Suites lists plans from every project under project headers", async () => {
  twoProjects();
  renderCombined();

  expect(await screen.findByRole("heading", { name: "Payroll" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Web" })).toBeInTheDocument();
  expect(screen.getByText("Auth - Test Plan")).toBeInTheDocument();
  expect(screen.getByText("Payslip - Test Plan")).toBeInTheDocument();
  // Name order, and every project was read: nothing to apologise for.
  const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent);
  expect(headings.indexOf("Payroll")).toBeLessThan(headings.indexOf("Web"));
  expect(screen.queryByText(/Showing plans from/)).not.toBeInTheDocument();
});

test("one project with plans shows no header", async () => {
  baseMock((cmd, args) => {
    if (cmd === "list_projects")
      return [
        { id: "p1", name: "Web" },
        { id: "p2", name: "Payroll" },
      ];
    if (cmd === "list_plans_with_suites")
      return (args as { project: string }).project === "Payroll"
        ? [{ plan: PAY_PLAN, suites: PAY_SUITES }]
        : [];
  });
  renderCombined();

  expect(await screen.findByText("Payslip - Test Plan")).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Payroll" })).not.toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Web" })).not.toBeInTheDocument();
});

test("search spans every project", async () => {
  twoProjects();
  renderCombined();
  await screen.findByRole("heading", { name: "Payroll" });

  fireEvent.change(screen.getByLabelText("Search suites"), { target: { value: "login" } });
  // Both projects have a "Login checks" suite: both stay, each under its header.
  expect(screen.getByRole("heading", { name: "Payroll" })).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Web" })).toBeInTheDocument();
  expect(screen.getByText("Auth - Test Plan")).toBeInTheDocument();
  expect(screen.getByText("Payslip - Test Plan")).toBeInTheDocument();

  // A name only one project has narrows the list to that project's plan.
  fireEvent.change(screen.getByLabelText("Search suites"), { target: { value: "PBI 77" } });
  expect(screen.getByText("Payslip - Test Plan")).toBeInTheDocument();
  expect(screen.queryByText("Auth - Test Plan")).not.toBeInTheDocument();
});

test("a suite's points are read in its plan's project", async () => {
  const asked = twoProjects((cmd) => {
    if (cmd === "list_test_points") return [POINT];
  });
  renderCombined();
  // The Payroll "Login checks" is the first row (name order puts Payroll first).
  const rows = await screen.findAllByText("Login checks");
  fireEvent.click(rows[0]);
  expect(await screen.findByText("Pay a slip")).toBeInTheDocument();
  const points = asked.filter((a) => a.cmd === "list_test_points");
  expect(points.map((a) => [a.args.project, a.args.planId, a.args.suiteId])).toEqual([["Payroll", 12, 121]]);
});

test("opening a requirement suite's PBI makes it the PBI with the plan's project", async () => {
  twoProjects();
  const opened: unknown[] = [];
  renderCombined({ onOpenPbi: (pbi, target) => opened.push([pbi, target]) });
  await screen.findByText("PBI 77 suite");

  fireEvent.click(screen.getByRole("button", { name: "Edit cases in PBI 77 suite" }));
  fireEvent.click(screen.getByRole("button", { name: "More actions for PBI 77 suite" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Run Tests" }));
  expect(opened).toEqual([
    [{ id: 77, title: "PBI 77 suite", project: "Payroll" }, "edit"],
    [{ id: 77, title: "PBI 77 suite", project: "Payroll" }, "run"],
  ]);
});

test("Use as current PBI looks the PBI up in the plan's project and hands that project up", async () => {
  const asked = twoProjects((cmd) => {
    if (cmd === "search_pbis") return []; // nothing found: the fallback names the plan's project
  });
  const picked: unknown[] = [];
  renderCombined({ onSetCurrentPbi: (p) => picked.push(p) });
  await screen.findByText("PBI 77 suite");

  fireEvent.click(screen.getByRole("button", { name: "More actions for PBI 77 suite" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Use as current PBI" }));
  await waitFor(() => expect(picked).toEqual([{ id: 77, title: "PBI 77 suite", project: "Payroll" }]));
  expect(asked.filter((a) => a.cmd === "search_pbis").map((a) => a.args.project)).toEqual(["Payroll"]);
});

test("cases handed to Update Test Cases carry their plan's project", async () => {
  const asked = twoProjects((cmd) => {
    if (cmd === "list_test_points") return [POINT];
  });
  const onEdit = vi.fn();
  renderCombined({ onEditCases: onEdit });
  await screen.findByRole("heading", { name: "Payroll" });

  // The Payroll "Login checks" row comes first; the Web one is last.
  const chips = screen.getAllByRole("button", { name: "Edit cases in Login checks" });
  fireEvent.click(chips[0]);
  await vi.waitFor(() => expect(onEdit).toHaveBeenCalledWith("Login checks", [301], "Payroll"));
  fireEvent.click(chips[1]);
  await vi.waitFor(() => expect(onEdit).toHaveBeenCalledWith("Login checks", [301], "Web"));
  expect(
    asked.filter((a) => a.cmd === "list_test_points").map((a) => [a.args.project, a.args.planId]),
  ).toEqual([
    ["Payroll", 12],
    ["Web", 9],
  ]);
});

test("a suite run, a report and its link use the plan's project", async () => {
  const asked = twoProjects((cmd) => {
    if (cmd === "list_test_points") return [POINT];
    if (cmd === "view_execution_report") return "report.html";
  });
  renderCombined();
  const more = await screen.findAllByRole("button", { name: "More actions for Login checks" });

  fireEvent.click(more[0]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Run Tests" }));
  await vi.waitFor(() => expect(localStorage.getItem("tcm-v2-runner-session")).toBeTruthy());
  const session = JSON.parse(localStorage.getItem("tcm-v2-runner-session") as string);
  expect(session.project).toBe("Payroll");
  expect(session.pbi.project).toBe("Payroll");
  expect(session.planId).toBe(12);

  fireEvent.click(more[0]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Report" }));
  await vi.waitFor(() => expect(asked.some((a) => a.cmd === "view_execution_report")).toBe(true));
  expect(asked.find((a) => a.cmd === "view_execution_report")?.args).toMatchObject({
    project: "Payroll",
    planId: 12,
  });
});

test("a project Search Suites may not read is left out and the coverage says so", async () => {
  baseMock((cmd, args) => {
    if (cmd === "list_projects")
      return [
        { id: "p1", name: "Web" },
        { id: "p2", name: "Payroll" },
      ];
    if (cmd === "list_plans_with_suites") {
      if ((args as { project: string }).project === "Payroll") throw { kind: "Forbidden" };
      return [{ plan: WEB_PLAN, suites: WEB_SUITES }];
    }
  });
  renderCombined();

  expect(await screen.findByText("Auth - Test Plan")).toBeInTheDocument();
  expect(screen.getByText("Showing plans from 1 of 2 projects.")).toBeInTheDocument();
});

test("combined with no organization says to pick one", () => {
  baseMock(() => undefined);
  renderCombined({ org: "" });
  expect(screen.getByText("Pick an organization in the bar above to browse test suites.")).toBeInTheDocument();
});

test("Refresh reads every project again", async () => {
  const asked = twoProjects();
  renderCombined();
  await screen.findByRole("heading", { name: "Payroll" });
  const reads = () => asked.filter((a) => a.cmd === "list_plans_with_suites").length;
  expect(reads()).toBe(2);

  fireEvent.click(screen.getByRole("button", { name: "Refresh test plans" }));
  await vi.waitFor(() => expect(reads()).toBe(4));
});

test("Refresh is disabled while the combined walk is reading", async () => {
  let release: (v: unknown) => void = () => {};
  baseMock((cmd, args) => {
    if (cmd === "list_projects")
      return [
        { id: "p1", name: "Web" },
        { id: "p2", name: "Payroll" },
      ];
    if (cmd === "list_plans_with_suites") {
      if ((args as { project: string }).project === "Payroll") return [{ plan: PAY_PLAN, suites: PAY_SUITES }];
      // Web is slow: the walk stays open until the test lets it go.
      return new Promise((resolve) => {
        release = resolve;
      });
    }
  });
  renderCombined();

  expect(await screen.findByText("Reading test plans in Web (2 of 2 projects)")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Refresh test plans" })).toBeDisabled();

  release([{ plan: WEB_PLAN, suites: WEB_SUITES }]);
  await screen.findByRole("heading", { name: "Web" });
  await waitFor(() => expect(screen.getByRole("button", { name: "Refresh test plans" })).toBeEnabled());
});

test("a Refresh that fails partway keeps the projects it had not reached", async () => {
  twoProjects();
  renderCombined();
  await screen.findByRole("heading", { name: "Payroll" });
  const stored = (project: string) => localStorage.getItem(`tcm-v2-cache:plans-suites:acme/${project}`);
  expect(stored("Web")).not.toBeNull();

  // Payroll still answers; Web is throttled this time.
  baseMock((cmd, args) => {
    if (cmd === "list_projects")
      return [
        { id: "p1", name: "Web" },
        { id: "p2", name: "Payroll" },
      ];
    if (cmd === "list_plans_with_suites") {
      if ((args as { project: string }).project === "Web")
        throw { kind: "RateLimited", detail: { retry_after_secs: 7 } };
      return [{ plan: PAY_PLAN, suites: PAY_SUITES }];
    }
  });
  fireEvent.click(screen.getByRole("button", { name: "Refresh test plans" }));

  expect(await screen.findByText("Rate limited - retry in 7s.")).toBeInTheDocument();
  // Web's warm tree is still on disk for the next visit.
  expect(JSON.parse(stored("Web") as string).data[0].plan.id).toBe(9);
});

test("an organization with no projects says so", async () => {
  baseMock((cmd) => {
    if (cmd === "list_projects") return [];
  });
  renderCombined();

  expect(await screen.findByText("This organization has no projects you can read.")).toBeInTheDocument();
});

test("switched off Search Suites lists the picked project as before", async () => {
  const asked: Array<{ cmd: string; args: Record<string, unknown> }> = [];
  baseMock((cmd, args) => {
    asked.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    if (cmd === "list_plans_with_suites") return [{ plan: WEB_PLAN, suites: WEB_SUITES }];
  });
  renderSuites();

  expect(await screen.findByText("Auth - Test Plan")).toBeInTheDocument();
  expect(asked.filter((a) => a.cmd === "list_plans_with_suites").map((a) => a.args.project)).toEqual(["Web"]);
  expect(asked.some((a) => a.cmd === "list_projects")).toBe(false);
  expect(screen.queryByRole("heading", { name: "Web" })).not.toBeInTheDocument();
});

test("Manage hands a suite to Suite Management with its plan's project", async () => {
  twoProjects();
  const managed: unknown[] = [];
  renderCombined({ onManageSuite: (planId, suiteId, project) => managed.push([planId, suiteId, project]) });
  await screen.findByRole("heading", { name: "Payroll" });

  // Both projects have a "Login checks"; the Payroll one is first.
  const more = screen.getAllByRole("button", { name: "More actions for Login checks" });
  fireEvent.click(more[0]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Manage" }));
  fireEvent.click(more[1]);
  fireEvent.click(screen.getByRole("menuitem", { name: "Manage" }));
  expect(managed).toEqual([
    [12, 121, "Payroll"],
    [9, 91, "Web"],
  ]);
});

test("switched off Manage names the picked project", async () => {
  baseMock((cmd) => {
    if (cmd === "list_plans_with_suites") return [{ plan: WEB_PLAN, suites: WEB_SUITES }];
  });
  const managed: unknown[] = [];
  renderSuites(undefined, undefined, (planId, suiteId, project) => managed.push([planId, suiteId, project]));
  await screen.findByText("Login checks");
  fireEvent.click(screen.getByRole("button", { name: "More actions for Login checks" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Manage" }));
  expect(managed).toEqual([[9, 91, "Web"]]);
});

/// Two screens hear one scan event; each shows only its own walk. This
/// screen's wait is the read of every plan ("plans"), never Run Tests'
/// search for one PBI's suite ("suite").
test("the scan bar follows the plans walk only, and says so when one large plan is being paged", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "list_plans_with_suites") return new Promise(() => {});
      if (cmd === "list_test_points") return [];
    },
    { shouldMockEvents: true },
  );
  renderSuites();
  expect(await screen.findByText("Loading test plans")).toBeInTheDocument();
  const { emit } = await import("@tauri-apps/api/event");

  await act(async () => {
    await emit("suite-scan-progress", { done: 3, total: 12, pages: null, kind: "suite", pbi_id: 42, project: "Web" });
  });
  expect(screen.getByText("Loading test plans")).toBeInTheDocument();
  expect(screen.queryByText("Scanning test plans")).not.toBeInTheDocument();

  await act(async () => {
    await emit("suite-scan-progress", { done: 5, total: 9, pages: null, kind: "plans", pbi_id: null, project: "Web" });
  });
  expect(screen.getByText("Scanning test plans")).toBeInTheDocument();
  expect(screen.getByText("5 / 9")).toBeInTheDocument();

  await act(async () => {
    await emit("suite-scan-progress", { done: 8, total: 9, pages: 4, kind: "plans", pbi_id: null, project: "Web" });
  });
  expect(screen.getByText("Reading a large plan (page 4)")).toBeInTheDocument();
  expect(screen.queryByText("8 / 9")).not.toBeInTheDocument();
});

const plansEvent = (project: string, done: number, total: number) => ({
  done,
  total,
  pages: null,
  kind: "plans",
  pbi_id: null,
  project,
});

/// The app also reads its current project's plans in the background, so a
/// plans walk for another project can be running while this screen waits.
test("the scan bar ignores another project's plans walk", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "list_plans_with_suites") return new Promise(() => {});
    },
    { shouldMockEvents: true },
  );
  renderSuites();
  expect(await screen.findByText("Loading test plans")).toBeInTheDocument();
  const { emit } = await import("@tauri-apps/api/event");

  await act(async () => {
    await emit("suite-scan-progress", plansEvent("Payroll", 40, 90));
  });
  expect(screen.getByText("Loading test plans")).toBeInTheDocument();
  expect(screen.queryByText("40 / 90")).not.toBeInTheDocument();

  await act(async () => {
    await emit("suite-scan-progress", plansEvent("Web", 2, 5));
  });
  expect(screen.getByText("2 / 5")).toBeInTheDocument();
  await act(async () => {
    await emit("suite-scan-progress", plansEvent("Payroll", 41, 90));
  });
  expect(screen.getByText("2 / 5")).toBeInTheDocument();
});

/// Projects together are read one after another: the bar follows the
/// project whose walk reports first until that walk is done, so two
/// projects' counts never alternate on it.
test("projects together: the bar follows one project's walk at a time", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "list_projects")
        return [
          { id: "p1", name: "Web" },
          { id: "p2", name: "Payroll" },
        ];
      if (cmd === "list_plans_with_suites") return new Promise(() => {});
    },
    { shouldMockEvents: true },
  );
  renderCombined();
  expect(await screen.findByText("Loading test plans")).toBeInTheDocument();
  const { emit } = await import("@tauri-apps/api/event");
  const send = (project: string, done: number, total: number) =>
    act(async () => {
      await emit("suite-scan-progress", plansEvent(project, done, total));
    });

  await send("Payroll", 1, 3);
  expect(screen.getByText("1 / 3")).toBeInTheDocument();
  await send("Web", 40, 90);
  expect(screen.getByText("1 / 3")).toBeInTheDocument();
  expect(screen.queryByText("40 / 90")).not.toBeInTheDocument();

  // Payroll's walk is done; the next project's walk takes over the bar.
  await send("Payroll", 3, 3);
  expect(screen.getByText("3 / 3")).toBeInTheDocument();
  await send("Web", 41, 90);
  expect(screen.getByText("41 / 90")).toBeInTheDocument();
});
