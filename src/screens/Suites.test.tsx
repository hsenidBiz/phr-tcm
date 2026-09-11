import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import Suites from "./Suites";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

function renderSuites(
  onEditCases?: (label: string, ids: number[]) => void,
  // Accepts a caller-supplied QueryClient so a test can pre-seed the cache
  // (qc.setQueryData) before the component ever mounts.
  qc: QueryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } }),
) {
  return render(
    <QueryClientProvider client={qc}>
      <Suites org="acme" project="Web" onEditCases={onEditCases} />
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

  fireEvent.click(screen.getByText("Run"));
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
