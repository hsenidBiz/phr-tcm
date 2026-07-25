/**
 * DEMO DATA MODE - dev builds only (loaded via a DEV-gated dynamic import
 * in main.tsx, so `tauri build` ships none of it). When enabled, every
 * Azure DevOps command is patched at the bindings layer to serve a fake
 * in-memory org: reads return dummy data, and creates/updates/imports land
 * in the in-memory store - nothing ever reaches a real organization. The
 * store keeps state for the session (submit -> the new cases show up in
 * Edit/View/Run), and resets on reload.
 */
import {
  commands,
  type AuthStatus,
  type BoardData,
  type CaseHistory,
  type EnsuredSuite,
  type NewWorkItem,
  type PbiHit,
  type RunOutcome,
  type SubmitItemResult,
  type TestCase,
  type TestCaseFull,
  type TestPoint,
  type WorkComment,
  type WorkItemDetail,
} from "../bindings";

const DEMO_KEY = "tcm-v2-dev-demo";
const PREFS_KEY = "tcm-v2-prefs";
const PREFS_BACKUP_KEY = "tcm-v2-dev-demo-prefs-backup";

export function isDemoMode(): boolean {
  try {
    return localStorage.getItem(DEMO_KEY) === "on";
  } catch {
    return false;
  }
}

/** Flip the flag and reload - patches only apply at boot. Entering demo
 * swaps the persisted context to the fake org with the demo PBI already
 * selected (the real prefs are backed up); leaving restores them, so the
 * user lands exactly where they were. */
export function toggleDemoMode() {
  try {
    const entering = !isDemoMode();
    if (entering) {
      localStorage.setItem(PREFS_BACKUP_KEY, localStorage.getItem(PREFS_KEY) ?? "");
      localStorage.setItem(
        PREFS_KEY,
        JSON.stringify({
          org: "DemoOrg",
          project: "Demo Project",
          section: "manual",
          pbi: PBIS[0],
          workMode: false,
        }),
      );
      // Recents for the demo project so the pick-a-PBI empty states and
      // the picker have something to offer.
      localStorage.setItem("tcm-v2-recent-pbis:DemoOrg/Demo Project", JSON.stringify(PBIS));
    } else {
      const backup = localStorage.getItem(PREFS_BACKUP_KEY);
      if (backup) localStorage.setItem(PREFS_KEY, backup);
      else localStorage.removeItem(PREFS_KEY);
      localStorage.removeItem(PREFS_BACKUP_KEY);
      // Drop demo-scoped caches so nothing fake lingers in real use.
      for (const k of Object.keys(localStorage)) {
        if (k.includes(":DemoOrg")) localStorage.removeItem(k);
      }
    }
    localStorage.setItem(DEMO_KEY, entering ? "on" : "off");
  } catch {
    // storage unavailable
  }
  window.location.reload();
}

const ok = <T,>(data: T) => Promise.resolve({ status: "ok" as const, data });
const err = <E,>(error: E) => Promise.resolve({ status: "error" as const, error });
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
/** Timestamp N seconds ago - keeps the "in progress" demo run looking
 * genuinely live (elapsed times grow) instead of frozen in the past. */
const nowMinus = (seconds: number) => new Date(Date.now() - seconds * 1000).toISOString();

// ---------------------------------------------------------------- dataset

const PBIS: PbiHit[] = [
  { id: 1001, title: "Demo - Login & session flow", work_item_type: "Product Backlog Item" },
  { id: 1002, title: "Demo - Checkout redesign", work_item_type: "Product Backlog Item" },
];

const full = (
  id: number,
  title: string,
  tags: string,
  steps: [string, string][],
  preconditions = "",
): TestCaseFull => ({
  id,
  title,
  tags,
  automation_status: "Not Automated",
  steps: steps.map(([action, expected]) => ({ action, expected })),
  step_ids: steps.map((_, i) => String(i + 2)),
  module_value: "",
  preconditions,
});

const casesByPbi = new Map<number, TestCaseFull[]>([
  [
    1001,
    [
      full(5001, "Login - valid credentials", "smoke; demo", [
        ["Open the login page", "The form is shown"],
        ["Enter valid credentials and submit", "The dashboard opens"],
      ], "A demo user exists"),
      full(5002, "Login - wrong password shows an error", "regression; demo", [
        ["Open the login page", "The form is shown"],
        ["Enter a wrong password", "An inline error appears"],
        ["Submit again with the right password", "The dashboard opens"],
      ]),
      full(5003, "Login - locked account is refused", "regression", [
        ["Lock the demo account", "Account flagged"],
        ["Attempt to sign in", "A lockout message appears"],
      ]),
      full(5004, "Session - expires after timeout", "demo", [
        ["Sign in and idle past the timeout", "The session expires"],
        ["Interact with the page", "The user lands back on login"],
      ]),
      full(5005, "Profile - update display name", "smoke", [
        ["Open profile settings", "The current name is shown"],
        ["Change the name and save", "A confirmation toast appears"],
      ]),
    ],
  ],
  [1002, [full(5101, "Checkout - guest can pay", "demo", [["Add an item and check out as guest", "Payment succeeds"]])]],
]);

let nextId = 6000;

/** point/run state per case id: last outcome + a little history. */
const pointState = new Map<number, { outcome: string; runId: number | null }>([
  [5001, { outcome: "passed", runId: 700 }],
  [5002, { outcome: "failed", runId: 700 }],
  [5003, { outcome: "blocked", runId: 699 }],
  [5004, { outcome: "", runId: null }],
  [5005, { outcome: "passed", runId: 698 }],
  [5101, { outcome: "", runId: null }],
]);
const historyByCase = new Map<number, RunOutcome[]>([
  [5001, [
    { outcome: "Passed", completed_date: "2026-07-13T09:00:00Z", run_id: 700 },
    { outcome: "Failed", completed_date: "2026-07-12T09:00:00Z", run_id: 695 },
  ]],
  [5002, [{ outcome: "Failed", completed_date: "2026-07-13T09:00:00Z", run_id: 700 }]],
]);
let nextRunId = 701;

const SUITE_BY_PBI = new Map<number, EnsuredSuite>([
  [1001, { plan_id: 90, plan_name: "Demo Test Plan", suite_id: 91, created_plan: false }],
  [1002, { plan_id: 90, plan_name: "Demo Test Plan", suite_id: 93, created_plan: false }],
]);
const PBI_BY_SUITE = new Map([...SUITE_BY_PBI].map(([pbi, s]) => [s.suite_id, pbi]));

const boardItems: BoardData["items"] = [
  { id: 2001, title: "Demo Task - wire the login flow", work_item_type: "Task", state: "In Progress", state_color: "007acc", column: "In Progress", assigned_to: "Demo User", tags: "demo", priority: 2, changed_date: "2026-07-13T08:00:00Z" },
  { id: 2002, title: "Demo Task - write test cases", work_item_type: "Task", state: "To Do", state_color: "b2b2b2", column: "To Do", assigned_to: "Demo User", tags: "", priority: 2, changed_date: "2026-07-13T07:00:00Z" },
  { id: 2003, title: "Demo Bug - session timeout not enforced", work_item_type: "Bug", state: "To Do", state_color: "cc293d", column: "To Do", assigned_to: "Demo User", tags: "demo", priority: 1, changed_date: "2026-07-13T06:00:00Z" },
  { id: 2004, title: "Demo Task - done example", work_item_type: "Task", state: "Done", state_color: "339933", column: "Done", assigned_to: "Demo User", tags: "", priority: 3, changed_date: "2026-07-12T06:00:00Z" },
];

const comments = new Map<number, WorkComment[]>([
  [2003, [{ id: 1, text: "Repro'd on the demo build.", created_by: "Demo User", created_date: "2026-07-13T06:30:00Z", avatar_url: "" }]],
]);

const detailFor = (b: BoardData["items"][number]): WorkItemDetail => ({
  id: b.id,
  title: b.title,
  work_item_type: b.work_item_type,
  state: b.state,
  assigned_to: b.assigned_to,
  assigned_to_unique: "demo@local",
  activity: "Development",
  tags: b.tags,
  area_path: "Demo Project\\Demo Team",
  iteration_path: "Demo Project\\Sprint 1",
  remaining_work: 2,
  completed_work: 1,
  original_estimate: 3,
  start_date: "2026-07-10T00:00:00Z",
  finish_date: "2026-07-15T00:00:00Z",
  description_text: "Demo description. Nothing here is real.",
  description_html: "<div><b>Demo description.</b> Nothing here is real.<ul><li>bullet one</li><li>bullet two</li></ul></div>",
  description_field: b.work_item_type === "Bug" ? "Microsoft.VSTS.TCM.ReproSteps" : "System.Description",
  extra_pages:
    b.work_item_type === "Bug"
      ? [
          {
            name: "RCA",
            fields: [
              { label: "Initial Findings", reference_name: "Demo.InitialFindings", section: 0, kind: "html", allowed: [], value: "<div>### What is the issue?\n**Session timeout** never fires in demo mode.</div>" },
              { label: "Root Cause Category", reference_name: "Demo.RootCauseCategory", section: 1, kind: "pick", allowed: ["Code Defect", "Design/Requirement", "Environment"], value: "Code Defect" },
            ],
          },
          {
            name: "Preventive Measures",
            fields: [
              { label: "Lessons Learned", reference_name: "Demo.LessonsLearned", section: 0, kind: "html", allowed: [], value: "" },
            ],
          },
        ]
      : [],
  extra_pages_error: null,
  inline_images: [],
});

// ---------------------------------------------------------------- patches

function applyPatches() {
  const auth: AuthStatus = { signed_in: true, account: "demo@local (DEMO DATA)" };
  const allCases = () => [...casesByPbi.values()].flat();
  const findCase = (id: number) => allCases().find((c) => c.id === id);

  Object.assign(commands, {
    authStatus: () => Promise.resolve(auth),
    signIn: () => ok(auth),
    checkUpdate: () => Promise.resolve(null),
    applyUpdate: () => err("Demo mode: updates are disabled"),

    listOrgs: () => ok([{ name: "DemoOrg", url: "https://example.invalid/demo" }]),
    listProjects: () => ok([{ id: "demo-project", name: "Demo Project" }]),
    searchPbis: (_o: string, _p: string, query: string) =>
      ok(PBIS.filter((b) => `${b.id} ${b.title}`.toLowerCase().includes(query.toLowerCase()))),

    listTestCaseFields: () => ok([]),
    testCaseFieldValues: () => ok([]),
    listProjectTags: () => ok(["demo", "smoke", "regression"]),
    classificationPaths: () => ok(["Demo Project", "Demo Project\\Demo Team"]),
    listIterations: () =>
      ok([
        { path: "Demo Project", start_date: null, finish_date: null },
        { path: "Demo Project\\Sprint 1", start_date: "2026-07-06T00:00:00Z", finish_date: "2026-07-17T00:00:00Z" },
        { path: "Demo Project\\Sprint 2", start_date: "2026-07-20T00:00:00Z", finish_date: "2026-07-31T00:00:00Z" },
      ]),
    activityValues: () => ok(["Development", "Testing"]),

    pbiTestCases: (_o: string, pbiId: number) =>
      ok((casesByPbi.get(pbiId) ?? []).map((c) => ({ id: c.id, title: c.title, tags: c.tags, automation_status: c.automation_status }))),
    pbiTestCasesFull: (_o: string, pbiId: number) => ok(casesByPbi.get(pbiId) ?? []),
    testCasesByIds: (_o: string, ids: number[]) =>
      ok(ids.map(findCase).filter((c): c is TestCaseFull => Boolean(c))),

    submitQueue: async (_o: string, _p: string, pbiId: number, queue: TestCase[]) => {
      const list = casesByPbi.get(pbiId) ?? [];
      casesByPbi.set(pbiId, list);
      const results: SubmitItemResult[] = [];
      for (const [i, tc] of queue.entries()) {
        await sleep(120); // let the progress UI breathe like a real run
        const existing = tc.update_id != null ? list.find((c) => c.id === tc.update_id) : undefined;
        if (existing) {
          existing.title = tc.title;
          existing.steps = tc.steps;
          existing.automation_status = tc.automation_status;
          if (tc.tags.trim()) existing.tags = tc.tags;
          if (tc.preconditions.trim()) existing.preconditions = tc.preconditions;
          results.push({ index: i, title: tc.title, action: "updated", id: existing.id, error: null });
        } else {
          const id = nextId++;
          list.push({ ...full(id, tc.title, tc.tags, []), steps: tc.steps, step_ids: tc.steps.map((_, s) => String(s + 2)), preconditions: tc.preconditions, automation_status: tc.automation_status });
          pointState.set(id, { outcome: "", runId: null });
          results.push({ index: i, title: tc.title, action: "created", id, error: null });
        }
      }
      return { status: "ok" as const, data: results };
    },
    updateTestCase: (_o: string, _p: string, tc: TestCase) => {
      const existing = tc.update_id != null ? findCase(tc.update_id) : undefined;
      if (existing) {
        existing.title = tc.title;
        existing.steps = tc.steps;
        existing.automation_status = tc.automation_status;
        if (tc.tags.trim()) existing.tags = tc.tags;
        if (tc.preconditions.trim()) existing.preconditions = tc.preconditions;
      }
      return ok(null);
    },

    ensurePbiSuite: (_o: string, _p: string, pbiId: number) =>
      ok(SUITE_BY_PBI.get(pbiId) ?? { plan_id: 90, plan_name: "Demo Test Plan", suite_id: 91, created_plan: false }),
    findPbiSuite: (_o: string, _p: string, pbiId: number) => ok(SUITE_BY_PBI.get(pbiId) ?? null),
    listPlansWithSuites: () =>
      ok([
        {
          plan: { id: 90, name: "Demo Test Plan", area_path: "Demo Project", root_suite_id: 1 },
          suites: [
            { id: 95, name: "Demo Folder", suite_type: "staticTestSuite", requirement_id: null, parent_id: null },
            { id: 91, name: "Demo - Login & session flow", suite_type: "requirementTestSuite", requirement_id: 1001, parent_id: 95 },
            { id: 93, name: "Demo - Checkout redesign", suite_type: "requirementTestSuite", requirement_id: 1002, parent_id: 95 },
          ],
        },
      ]),
    listTestPoints: (_o: string, _p: string, _plan: number, suiteId: number) => {
      const pbiId = PBI_BY_SUITE.get(suiteId);
      const cases = pbiId != null ? (casesByPbi.get(pbiId) ?? []) : [];
      const points: TestPoint[] = cases.map((c) => {
        const st = pointState.get(c.id) ?? { outcome: "", runId: null };
        return {
          point_id: c.id + 40000,
          test_case_id: c.id,
          test_case_name: c.title,
          config_name: "Demo Config",
          tester: "Demo User",
          last_outcome: st.outcome,
          last_run_id: st.runId,
          last_result_id: st.runId != null ? st.runId + 100 : null,
        };
      });
      return ok(points);
    },
    runHistory: () =>
      ok([...historyByCase.entries()].map(([test_case_id, outcomes]): CaseHistory => ({ test_case_id, outcomes }))),
    submitTestRun: (_o: string, _p: string, _plan: number, _name: string, outcomes: { point_id: number; outcome: string }[]) => {
      const runId = nextRunId++;
      for (const oc of outcomes) {
        const caseId = oc.point_id - 40000;
        pointState.set(caseId, { outcome: oc.outcome.toLowerCase(), runId });
        const hist = historyByCase.get(caseId) ?? [];
        hist.unshift({ outcome: oc.outcome, completed_date: new Date().toISOString(), run_id: runId });
        historyByCase.set(caseId, hist.slice(0, 5));
      }
      return ok({ run_id: runId, web_url: "https://example.invalid/demo-run" });
    },
    getResultDetail: () => ok({ outcome: "Failed", comment: "Demo failure comment" }),
    resultFailureDetail: () => ok({ comment: "Demo: step 3 timed out waiting for the redirect.", bug_ids: [2003] }),
    resultScreenshots: () => ok([]),
    fileBug: (_o: string, _p: string, title: string) => {
      const id = nextId++;
      boardItems.push({ id, title, work_item_type: "Bug", state: "To Do", state_color: "cc293d", column: "To Do", assigned_to: "Demo User", tags: "demo", priority: 2, changed_date: new Date().toISOString() });
      return ok({ id, url: "https://example.invalid/demo-bug" });
    },
    viewExecutionReport: () => err("Demo mode: execution reports are disabled"),

    listRepos: () =>
      ok([
        { id: "demo-repo-1", name: "demo-web" },
        { id: "demo-repo-2", name: "demo-api" },
      ]),
    prOverview: () =>
      ok({
        awaiting: [
          {
            id: 501, title: "Add PBI scope to the board", repo: "demo-web", repo_id: "demo-repo-1",
            author: "Sam Rivera", source_branch: "feature/pbi-scope", target_branch: "main",
            created: "2026-07-17T09:00:00Z",
            description: "Demo description: what changed and why.", is_draft: false, has_conflicts: false,
            my_vote: 0,
            reviewers: [
              { display_name: "Demo User", vote: 0 },
              { display_name: "Alex Kim", vote: 10 },
            ],
            web_url: "https://example.invalid/demo-pr/501",
          },
        ],
        mine: [
          {
            id: 502, title: "Electric border for the PBI chip", repo: "demo-web", repo_id: "demo-repo-1",
            author: "Demo User", source_branch: "feature/electric-border", target_branch: "main",
            created: "2026-07-16T14:00:00Z",
            description: "Demo description: what changed and why.", is_draft: true, has_conflicts: true,
            my_vote: 0,
            reviewers: [{ display_name: "Sam Rivera", vote: -5 }],
            web_url: "https://example.invalid/demo-pr/502",
          },
        ],
      }),
    boardPrLinks: () =>
      ok([
        {
          work_item_id: 2001, pr_id: 501, status: "active",
          title: "Add PBI scope to the board", repo: "demo-web", repo_id: "demo-repo-1",
          web_url: "https://example.invalid/demo-pr/501",
        },
        {
          work_item_id: 2001, pr_id: 499, status: "completed",
          title: "Earlier slice", repo: "demo-db", web_url: "https://example.invalid/demo-pr/499",
        },
      ]),
    prWorkItems: () =>
      ok([
        {
          id: 143783, work_item_type: "Bug",
          title: "Participants - Selected employees and department inconsistencies",
          state: "In Progress", state_color: "007acc",
          url: "https://example.invalid/demo-wi/143783",
        },
      ]),
    repoPullRequests: () =>
      ok([
        {
          id: 503, title: "Bump dependencies", repo: "demo-web", repo_id: "demo-repo-1",
          author: "Alex Kim", source_branch: "chore/deps", target_branch: "main",
          created: "2026-07-15T08:00:00Z",
            description: "Demo description: what changed and why.", is_draft: false, has_conflicts: false,
          my_vote: 10,
          reviewers: [{ display_name: "Demo User", vote: 10 }],
          web_url: "https://example.invalid/demo-pr/503",
          status: "active", closed: "", merge_commit: "",
        },
      ]),
    // Three runs so the pipeline views have one of each kind to show: one
    // still building (newest), one green that deployed, and the failed
    // PR-validation run - including the failing step message.
    prPipeline: () =>
      ok([
        {
          id: 902, name: "demo-web", number: "2026.7.25-03", status: "inProgress",
          result: "", is_validation: false,
          started: nowMinus(6 * 60), finished: "",
          web_url: "https://example.invalid/demo-build/902",
          stages: [
            {
              name: "Build", state: "completed", result: "succeeded",
              started: nowMinus(6 * 60), finished: nowMinus(2 * 60),
              jobs: [
                {
                  name: "Build_solution", state: "completed", result: "succeeded",
                  started: nowMinus(6 * 60), finished: nowMinus(2 * 60),
                  tasks: [
                    { name: "Restore The Solution", state: "completed", result: "succeeded",
                      started: nowMinus(6 * 60), finished: nowMinus(330), issues: [], log_id: 1 },
                    { name: "Build The Solution", state: "completed", result: "succeeded",
                      started: nowMinus(330), finished: nowMinus(180), issues: [], log_id: 2 },
                    { name: "Run Unit Test", state: "completed", result: "succeeded",
                      started: nowMinus(180), finished: nowMinus(120), issues: [], log_id: 3 },
                  ],
                },
              ],
            },
            {
              name: "Package", state: "inProgress", result: "",
              started: nowMinus(120), finished: "",
              jobs: [
                {
                  name: "Build Docker App", state: "inProgress", result: "",
                  started: nowMinus(120), finished: "",
                  tasks: [
                    { name: "Initialize job", state: "completed", result: "succeeded",
                      started: nowMinus(120), finished: nowMinus(110), issues: [], log_id: 4 },
                    { name: "Build The Image", state: "inProgress", result: "",
                      started: nowMinus(110), finished: "", issues: [], log_id: 5 },
                    { name: "Push The Image", state: "pending", result: "",
                      started: "", finished: "", issues: [], log_id: 6 },
                  ],
                },
              ],
            },
            { name: "Deploy", state: "pending", result: "", started: "", finished: "", jobs: [] },
          ],
          deployments: [],
        },
        {
          id: 901, name: "demo-web", number: "2026.7.24-12", status: "completed",
          result: "succeeded", is_validation: false,
          started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:06:00Z",
          web_url: "https://example.invalid/demo-build/901",
          stages: [
            {
              name: "Build", state: "completed", result: "succeeded",
              started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:04:00Z",
              jobs: [
                {
                  name: "Build_solution", state: "completed", result: "succeeded",
                  started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:04:00Z",
                  tasks: [
                    { name: "Restore The Solution", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:00:00Z", finished: "2026-07-24T09:00:20Z", issues: [], log_id: 7 },
                    { name: "Build The Solution", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:00:20Z", finished: "2026-07-24T09:03:00Z", issues: [], log_id: 8 },
                    { name: "Run Unit Test", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:03:00Z", finished: "2026-07-24T09:04:00Z", issues: [], log_id: 9 },
                  ],
                },
              ],
            },
            {
              name: "Package", state: "completed", result: "succeeded",
              started: "2026-07-24T09:04:00Z", finished: "2026-07-24T09:06:00Z",
              jobs: [
                {
                  name: "Build Docker App", state: "completed", result: "succeeded",
                  started: "2026-07-24T09:04:00Z", finished: "2026-07-24T09:06:00Z",
                  tasks: [
                    { name: "Build The Image", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:04:00Z", finished: "2026-07-24T09:05:40Z", issues: [], log_id: 10 },
                    { name: "Push The Image", state: "completed", result: "succeeded",
                      started: "2026-07-24T09:05:40Z", finished: "2026-07-24T09:06:00Z", issues: [], log_id: 11 },
                  ],
                },
              ],
            },
          ],
          deployments: [
            {
              release: "Release-482", environment: "QA", status: "succeeded",
              on: "2026-07-24T10:00:00Z", web_url: "https://example.invalid/demo-release/482",
            },
            {
              release: "Release-482", environment: "Production", status: "notStarted",
              on: "", web_url: "https://example.invalid/demo-release/482",
            },
          ],
        },
        {
          id: 900, name: "demo-web (Build)", number: "20260724.1", status: "completed",
          result: "failed", is_validation: true,
          started: "2026-07-24T08:00:00Z", finished: "2026-07-24T08:04:00Z",
          web_url: "https://example.invalid/demo-build/900",
          stages: [
            {
              name: "Build", state: "completed", result: "failed",
              started: "2026-07-24T08:00:00Z", finished: "2026-07-24T08:04:00Z",
              jobs: [
                {
                  name: "Build_solution", state: "completed", result: "failed",
                  started: "2026-07-24T08:00:00Z", finished: "2026-07-24T08:04:00Z",
                  tasks: [
                    { name: "Restore The Solution", state: "completed", result: "succeeded",
                      started: "2026-07-24T08:00:00Z", finished: "2026-07-24T08:00:30Z", issues: [], log_id: 12 },
                    { name: "Run Unit Test", state: "completed", result: "failed",
                      started: "2026-07-24T08:00:30Z", finished: "2026-07-24T08:04:00Z",
                      issues: ["3 tests failed in PeoplesHR.PMS.Infrastructure.Tests"], log_id: 99 },
                    { name: "Publish Test Results", state: "completed", result: "skipped",
                      started: "", finished: "", issues: [], log_id: 13 },
                  ],
                },
              ],
            },
          ],
          deployments: [],
        },
      ]),

    fetchBoard: () =>
      ok({
        items: boardItems,
        states_by_type: {
          Task: [
            { name: "To Do", color: "b2b2b2", category: "Proposed" },
            { name: "In Progress", color: "007acc", category: "InProgress" },
            { name: "Done", color: "339933", category: "Completed" },
          ],
          Bug: [
            { name: "To Do", color: "cc293d", category: "Proposed" },
            { name: "In Progress", color: "007acc", category: "InProgress" },
            { name: "Done", color: "339933", category: "Completed" },
          ],
        },
      }),
    moveBoardItem: (_o: string, _p: string, itemId: number, _t: string, column: string) => {
      const item = boardItems.find((b) => b.id === itemId);
      const state = column === "Done" ? "Done" : column;
      if (item) {
        item.column = column;
        item.state = state;
        item.changed_date = new Date().toISOString();
      }
      return ok(state);
    },
    workItemDetail: (_o: string, _p: string, id: number) => {
      const item = boardItems.find((b) => b.id === id);
      return item ? ok(detailFor(item)) : err({ kind: "notFound" });
    },
    updateWorkItem: (_o: string, _p: string, id: number, patches: { reference_name: string; value: string }[]) => {
      const item = boardItems.find((b) => b.id === id);
      for (const p of patches) {
        if (!item) break;
        if (p.reference_name === "System.Title") item.title = p.value;
        if (p.reference_name === "System.State") item.state = p.value;
      }
      return ok(null);
    },
    workItemComments: (_o: string, _p: string, id: number) => ok(comments.get(id) ?? []),
    addComment: (_o: string, _p: string, id: number, text: string) => {
      const list = comments.get(id) ?? [];
      list.unshift({ id: Date.now(), text, created_by: "Demo User", created_date: new Date().toISOString(), avatar_url: "" });
      comments.set(id, list);
      return ok(null);
    },
    listTeamMembers: () => ok([{ display_name: "Demo User", unique_name: "demo@local" }]),
    listTeams: () => ok(["Demo Team"]),
    createWorkItem: (_o: string, _p: string, item: NewWorkItem) => {
      const id = nextId++;
      boardItems.push({ id, title: item.title, work_item_type: item.wi_type, state: "To Do", state_color: "b2b2b2", column: "To Do", assigned_to: item.assigned_to ?? "", tags: item.tags ?? "", priority: item.priority ?? 2, changed_date: new Date().toISOString() });
      return ok({ id, url: "https://example.invalid/demo-wi" });
    },
    bridgeStatus: () => ok({ port: 51999, mcp_exe: "C:\\demo\\v2.exe" }),
    setBridgeContext: () => ok(null),
    detectAiTools: () =>
      Promise.resolve([
        { id: "claude-code", name: "Claude Code", installed: true, registered: true },
        { id: "vscode", name: "VS Code", installed: true, registered: false },
      ]),
    registerAiTool: () => ok(null),
    unregisterAiTool: () => ok(null),
    setAdoRateLevel: () => Promise.resolve(200),
    prDeployments: () => ok([]),
    buildLog: () =>
      ok(
        [
          "Starting: Run Unit Test",
          "==============================================================================",
          "Task         : .NET Core",
          "Determining projects to restore...",
          "Restored /home/vsts/work/1/s/src/Demo.Application.csproj (in 344 ms)",
          "##[warning]Package 'SQLitePCLRaw' 2.1.11 has a known vulnerability",
          "Test run for /home/vsts/work/1/s/Demo.Tests.dll (.NETCoreApp,Version=v10.0)",
          "Passed!  - Failed:     0, Passed:   203, Skipped:     0, Total:   203",
          "##[error]3 tests failed in PeoplesHR.PMS.Infrastructure.Tests",
          "Finishing: Run Unit Test",
        ].join(String.fromCharCode(10)),
      ),
    appLogDir: () => Promise.resolve("C:\demo\logs"),
    appLogs: () =>
      Promise.resolve([
        { at: "2026-07-26 09:00:01", level: "info", message: "Test Case Manager 1.11.2 started" },
        { at: "2026-07-26 09:00:04", level: "info", message: "Signed in to Azure DevOps" },
        { at: "2026-07-26 09:02:10", level: "info", message: "Submitting 3 test case(s) to DemoOrg/Demo Project PBI #1001" },
        { at: "2026-07-26 09:02:14", level: "warn", message: "Retrying after rate limit (5s)" },
        { at: "2026-07-26 09:02:20", level: "error", message: "Submit failed for 'Checkout - guest can pay': field 'Module' is required" },
        { at: "2026-07-26 09:02:21", level: "info", message: "Submit finished: 3 of 3 processed, 1 failed" },
      ]),
  });
}

/** Called from main.tsx (DEV only) before the app renders. */
export function maybeEnableDemoMode() {
  if (isDemoMode()) applyPatches();
}
