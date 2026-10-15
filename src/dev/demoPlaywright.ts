/**
 * DEMO DATA for the Playwright tab (dev builds only; spread into the demo
 * patches in dev/demo.ts). A made-up clone of the team's Playwright repo: a
 * folder tree with a few features, a tree of tests with last results,
 * reports, test accounts, Sync rows in every state, and an Export preview
 * with cases with and without a script. Nothing here touches the disk: a
 * run streams canned output, a change answers with canned CLI lines.
 *
 * Every name, address and password is made up.
 */
import {
  events,
  type AccountChange,
  type AccountRefs,
  type AppSettings,
  type ExportMap,
  type ExportResult,
  type HandoffCard,
  type ListedTest,
  type Preview,
  type PreviewCase,
  type PwReport,
  type RepoAccount,
  type RepoStructure,
  type RunAction,
  type SyncRow,
  type TestTree,
} from "../bindings";

const ok = <T,>(data: T) => Promise.resolve({ status: "ok" as const, data });

/** Where the sample clone "is". A folder picked in demo mode replaces it,
 * in memory only. */
let demoClone = "C:\\Users\\demo\\source\\PHR-PLAYWRIGHT-AUTOMATION";

const FEATURES: RepoStructure["features"] = [
  { path: ["sl", "admin", "leave", "leave-types"], navigation_key: "leave/leave-types", spec_count: 2, case_count: 6, case_ids: [4101, 4102, 4103, 4104, 4105, 4106] },
  { path: ["sl", "admin", "payroll", "salary-components"], navigation_key: "payroll/salary-components", spec_count: 1, case_count: 3, case_ids: [4301, 4302, 4303] },
  { path: ["sl", "admin", "performance", "definition-wizard", "competencies"], navigation_key: "definition-wizard/competencies", spec_count: 1, case_count: 2, case_ids: [4221, 4222] },
  { path: ["sl", "admin", "performance", "goal-setting"], navigation_key: "performance/goal-setting", spec_count: 1, case_count: 4, case_ids: [4201, 4202, 4203, 4204] },
  { path: ["sl", "admin", "performance", "rating-methods"], navigation_key: "performance/rating-methods", spec_count: 2, case_count: 5, case_ids: [4211, 4212, 4213, 4214, 4215] },
  { path: ["sl", "self", "leave", "apply-leave"], navigation_key: "leave/apply-leave", spec_count: 1, case_count: 3, case_ids: [4401, 4402, 4403] },
  { path: ["sl", "self", "smoke", "framework-smoke"], navigation_key: "smoke/framework-smoke", spec_count: 1, case_count: 0, case_ids: [] },
];

const STRUCTURE: RepoStructure = {
  features: FEATURES,
  generated_ids: [4101, 4102, 4201, 4211, 4212],
  user_keys: ["DemoSL.admin.hr", "DemoSL.performance.general", "DemoSL.performance.supervisor", "DemoSL.leave.employee", "DemoSL.payroll.officer"],
  navigation_keys: ["leave/leave-types", "definition-wizard/competencies", "performance/goal-setting", "performance/rating-methods", "payroll/salary-components", "leave/apply-leave"],
  health: [],
};

function test(
  title: string,
  where: { country?: string; side: string; module: string; feature: string; spec?: string },
  ado_id: number | null,
  last_result: ListedTest["last_result"],
  line: number,
): ListedTest {
  const country = where.country ?? "sl";
  const spec = where.spec ?? where.feature;
  const file =
    country === "common-components"
      ? `common-components/${where.feature}/specs/${spec}.spec.ts`
      : `${country}/${where.side}/${where.module}/${where.feature}/specs/${spec}.spec.ts`;
  const title_path = [spec.split("-").map((w) => w[0].toUpperCase() + w.slice(1)).join(" "), title];
  return {
    id: JSON.stringify([file, ...title_path]),
    path:
      country === "common-components"
        ? [country, where.feature]
        : [country, where.side, where.module, where.feature],
    spec,
    file,
    title,
    title_path,
    line,
    project: country === "common-components" ? "Shared" : "SL",
    ado_id,
    last_result,
  };
}

const SHARED = { country: "common-components", side: "", module: "", feature: "employee-search" };
const LEAVE_TYPES = { side: "admin", module: "leave", feature: "leave-types" };
const LEAVE_RULES = { side: "admin", module: "leave", feature: "leave-types", spec: "leave-type-rules" };
const GOALS = { side: "admin", module: "performance", feature: "goal-setting" };
const RATING = { side: "admin", module: "performance", feature: "rating-methods" };
const SALARY = { side: "admin", module: "payroll", feature: "salary-components" };
const APPLY = { side: "self", module: "leave", feature: "apply-leave" };
const SMOKE = { side: "self", module: "smoke", feature: "framework-smoke" };

const TESTS: ListedTest[] = [
  test("finds an employee by name", SHARED, null, "Passed", 6),
  test("finds an employee by employee number", SHARED, null, "Passed", 18),
  test("Admin can create a leave type", LEAVE_TYPES, 4101, "Passed", 8),
  test("Admin can edit a leave type's name", LEAVE_TYPES, 4102, "Passed", 31),
  test("A leave type in use cannot be deleted", LEAVE_TYPES, 4103, "Failed", 52),
  test("Carry forward is capped at the set limit", LEAVE_RULES, 4104, "Flaky", 9),
  test("Half days follow the company calendar", LEAVE_RULES, 4105, null, 40),
  test("Admin can set a goal for a team", GOALS, 4201, "Passed", 7),
  test("A goal's weight cannot pass 100", GOALS, 4202, "Passed", 29),
  test("User can create a new rating method", RATING, 4211, "Passed", 5),
  test("User can delete a rating method", RATING, 4212, "Failed", 27),
  test("Rating scale steps keep their order", RATING, 4213, "Skipped", 44),
  test("A salary component adds to gross pay", SALARY, 4301, null, 11),
  test("An employee can apply for a full day", APPLY, 4401, "Passed", 6),
  test("An employee sees the balance after applying", APPLY, 4402, "Passed", 24),
  test("the app shell loads", SMOKE, null, "Passed", 4),
];

const TREE: TestTree = { tests: TESTS, projects: ["SL", "Shared"], last_report: "2026-10-13T09-14-00", load_errors: [] };

let reports: PwReport[] = [
  { run_id: "2026-10-13T09-14-00", stats: { passed: 11, failed: 2, flaky: 1, skipped: 1, duration_ms: 184000 } },
  { run_id: "2026-10-12T16-02-41", stats: { passed: 9, failed: 0, flaky: 0, skipped: 0, duration_ms: 97000 } },
  { run_id: "2026-10-10T11-30-05", stats: { passed: 4, failed: 1, flaky: 0, skipped: 0, duration_ms: 41000 } },
];

/** A demo account with what uses it (the CLI counts these; users.json has none). */
type DemoAccount = RepoAccount & Omit<AccountRefs, "key">;
let accounts: DemoAccount[] = [
  { key: "DemoSL.admin.hr", username: "hr.admin@demo.example", employee_no: "E1001", display_name: "Dana Admin", title: "HR Manager", supervisor: "", specs: 5, other_refs: 2 },
  { key: "DemoSL.performance.general", username: "perf.general@demo.example", employee_no: "E1042", display_name: "Pat General", title: "Analyst", supervisor: "DemoSL.performance.supervisor", specs: 3, other_refs: 0 },
  { key: "DemoSL.performance.supervisor", username: "perf.super@demo.example", employee_no: "E1007", display_name: "Sam Supervisor", title: "Team Lead", supervisor: "", specs: 1, other_refs: 1 },
  { key: "DemoSL.leave.employee", username: "leave.emp@demo.example", employee_no: "E1103", display_name: "Lee Employee", title: "Associate", supervisor: "DemoSL.performance.supervisor", specs: 2, other_refs: 0 },
  { key: "DemoSL.payroll.officer", username: "payroll@demo.example", employee_no: "", display_name: "", title: "", supervisor: "", specs: 0, other_refs: 0 },
];

const SYNC: SyncRow[] = [
  { case_id: 4101, title: "Admin can create a leave type", state: { kind: "Current" }, exported_at: "2026-10-09T08:12:00Z", has_script: true, in_repo: true, repo_note: "in leave-types.md and index.json" },
  { case_id: 4102, title: "Admin can edit a leave type's name", state: { kind: "OutOfDate", why: "changed since the export: steps" }, exported_at: "2026-10-09T08:12:00Z", has_script: true, in_repo: true, repo_note: "in leave-types.md and index.json" },
  { case_id: 4201, title: "Admin can set a goal for a team", state: { kind: "OutOfDate", why: "changed since the export: account" }, exported_at: "2026-10-07T14:40:00Z", has_script: true, in_repo: true, repo_note: "in goal-setting.md and index.json" },
  { case_id: 4211, title: "User can create a new rating method", state: { kind: "Current" }, exported_at: "2026-10-07T14:40:00Z", has_script: true, in_repo: true, repo_note: "in rating-methods.md and specs/rating-methods.spec.ts" },
  { case_id: 4212, title: "User can delete a rating method", state: { kind: "NotInRepo" }, exported_at: "2026-10-07T14:40:00Z", has_script: true, in_repo: false, repo_note: null },
  { case_id: 4302, title: "A deduction lowers net pay", state: { kind: "NeverExported" }, exported_at: null, has_script: true, in_repo: false, repo_note: null },
  { case_id: 4303, title: "A component can be switched off for one grade", state: { kind: "NeverExported" }, exported_at: null, has_script: true, in_repo: false, repo_note: null },
  { case_id: 4401, title: "An employee can apply for a full day", state: { kind: "InRepoNotFromApp" }, exported_at: null, has_script: true, in_repo: true, repo_note: "in specs/apply-leave.spec.ts" },
  { case_id: 4104, title: "Carry forward is capped at the set limit", state: { kind: "ExcludedInRepo" }, exported_at: null, has_script: true, in_repo: true, repo_note: "Excluded in the repo (in leave-type-rules.md)" },
  { case_id: 4103, title: "A leave type in use cannot be deleted", state: { kind: "OutOfDate", why: "its script is no longer in Auto Run" }, exported_at: "2026-10-02T10:00:00Z", has_script: false, in_repo: true, repo_note: "in leave-types.md and index.json" },
];

type CaseTemplate = Omit<PreviewCase, "case_id">;
const base: CaseTemplate = {
  title: "",
  exportable: true,
  reason: null,
  seg: "sl/admin/leave/leave-types",
  user_key: "DemoSL.admin.hr",
  add_user_command: null,
  in_repo: false,
  repo_note: null,
  from_app: false,
  excluded: false,
  has_script: true,
};
/** What the Export preview says of each case asked about, in turn. */
const EXPORT_CASES: CaseTemplate[] = [
  { ...base, title: "Admin can create a leave type" },
  { ...base, title: "Admin can edit a leave type's name", in_repo: true, repo_note: "in leave-types.md and index.json", from_app: true },
  { ...base, title: "A leave type with a past end date is refused" },
  { ...base, title: "No script yet", exportable: false, reason: "no script - record or write one in Auto Run", seg: null, user_key: null, has_script: false },
  { ...base, title: "Carry forward is capped at the set limit", exportable: false, in_repo: true, excluded: true, repo_note: "Excluded in the repo (in leave-type-rules.md)" },
  { ...base, title: "Leave balance shows on the dashboard", exportable: false, reason: "its newest run did not pass", seg: null },
  { ...base, title: "Admin can set a goal for a team", seg: "sl/admin/performance/goal-setting" },
  { ...base, title: "A goal's weight cannot pass 100", seg: "sl/admin/performance/goal-setting", in_repo: true, repo_note: "in goal-setting.md and specs/goal-setting.spec.ts" },
  {
    ...base,
    title: "A supervisor approves a goal",
    exportable: false,
    reason: "repo user \"DemoSL.performance.approver\" is not in the clone's users.json",
    seg: "sl/admin/performance/goal-setting",
    user_key: "DemoSL.performance.approver",
    add_user_command: "npm run users -- add DemoSL.performance.approver --username approver@demo.example --password <password> --apply",
  },
  { ...base, title: "No script yet", exportable: false, reason: "no script - record or write one in Auto Run", seg: null, user_key: null, has_script: false },
  { ...base, title: "No script yet", exportable: false, reason: "no script - record or write one in Auto Run", seg: null, user_key: null, has_script: false },
];
const EXPORT_IDS = [5101, 5102, 5103, 5104, 5105, 5106, 5201, 5202, 5203, 5204, 5205];

let exportMap: ExportMap = {
  areas: { "Leave Types": { side: "admin", module: "leave", feature: "leave-types" } },
  accounts: { dev: { "hr.admin": "DemoSL.admin.hr", "perf.supervisor": "DemoSL.performance.supervisor" } },
};

function exportPreview(caseIds: number[]): Preview {
  const ids = caseIds.length ? caseIds : EXPORT_IDS;
  return {
    environment: "dev",
    clone_ok: true,
    clone_problem: null,
    user_keys: STRUCTURE.user_keys,
    areas: ["Leave Types", "Goal Setting"],
    accounts: ["hr.admin", "perf.supervisor"],
    map: exportMap,
    suggested: { "Goal Setting": { side: "admin", module: "performance", feature: "goal-setting" } },
    cases: ids.map((case_id, i) => ({ ...EXPORT_CASES[i % EXPORT_CASES.length], case_id })),
  };
}

function written(ids: number[]): ExportResult {
  return {
    files: [...new Set(ids.map((id) => `suites/_generated/case-${id}.spec.ts`)), "suites/sl/admin/leave/leave-types/test-cases/leave-types.md"],
    cases: ids.map((id) => [id, `suites/_generated/case-${id}.spec.ts`]),
    user_keys: ids.map((id) => [id, "DemoSL.admin.hr"]),
    missing_navigation: [],
    sections: ids.map((id) => [id, "suites/sl/admin/leave/leave-types/test-cases/leave-types.md"]),
    record_error: null,
    skipped: [],
  };
}

function handoff(ids: number[]): HandoffCard[] {
  return [
    {
      seg: "sl/admin/leave/leave-types",
      stem: "leave-types",
      case_ids: ids,
      prompt: [
        `Run /gen-test steps 4, 5, 6 and 7 for ${ids.map((id) => `#${id}`).join(", ")}.`,
        "Target spec: suites/sl/admin/leave/leave-types/specs/leave-types.spec.ts",
        "Run each raw spec, heal it until it passes, then refactor it into the target spec.",
        "Run npm run lint:tests -- --require-specs and the live check, then summarise the run.",
      ].join("\n"),
      problem: null,
      already: [],
    },
  ];
}

/** A run: canned output a line at a time, then its end. */
let nextRun = 900;
function streamRun(run: number, count: number) {
  const lines = [
    `Running ${count} tests using 2 workers`,
    "",
    "  ok 1 [SL] > leave-types.spec.ts:8:3 > Leave Types > Admin can create a leave type (6.2s)",
    "  ok 2 [SL] > leave-types.spec.ts:31:3 > Leave Types > Admin can edit a leave type's name (4.8s)",
    "  x  3 [SL] > leave-types.spec.ts:52:3 > Leave Types > A leave type in use cannot be deleted (12.1s)",
    "",
    "    Error: expect(locator).toBeVisible() failed",
    "    Locator: getByRole('alert')",
    "    Expected: visible",
    "    Timeout: 10000ms",
    "",
    "  ok 4 [Shared] > employee-search.spec.ts:6:3 > Employee Search > finds an employee by name (2.4s)",
    "",
    "  1 failed",
    "  3 passed (31.6s)",
  ];
  lines.forEach((line, i) => setTimeout(() => void events.pwOutput.emit({ run, line }), 250 * (i + 1)));
  setTimeout(() => {
    // The report first, so the run's end finds it and opens it.
    reports = [{ run_id: new Date().toISOString().slice(0, 19).replace(/:/g, "-"), stats: { passed: 3, failed: 1, flaky: 0, skipped: 0, duration_ms: 31600 } }, ...reports];
    void events.pwDone.emit({ run, code: 1 });
  }, 250 * (lines.length + 2));
}

/** The accounts as users.json gives them: no counts. */
const listed = (): RepoAccount[] => accounts.map(({ specs: _s, other_refs: _o, ...a }) => a);

function accountChange(change: AccountChange, apply: boolean) {
  const what =
    change.kind === "Add"
      ? `add ${change.key}`
      : change.kind === "Set"
        ? `set ${change.key} ${change.field}`
        : change.kind === "Rename"
          ? `rename ${change.from} -> ${change.to}`
          : `remove ${change.key}`;
  if (change.kind === "Remove" && accounts.find((a) => a.key === change.key && (a.specs || a.other_refs))) {
    return Promise.resolve({ status: "error" as const, error: `"${change.key}" is still used. Repoint or remove those first.` });
  }
  if (!apply) {
    return ok({ applied: false, output: [what, "  1 file will change: src/users/users.json", "  Nothing written. Re-run with --apply to write it."], accounts: null, list_error: null });
  }
  if (change.kind === "Add") {
    accounts = [...accounts, { key: change.key, username: change.username, employee_no: change.employee_no, display_name: change.display_name, title: change.title, supervisor: change.supervisor, specs: 0, other_refs: 0 }];
  } else if (change.kind === "Rename") {
    accounts = accounts.map((a) => (a.key === change.from ? { ...a, key: change.to } : a));
  } else if (change.kind === "Remove") {
    accounts = accounts.filter((a) => a.key !== change.key);
  }
  return ok({ applied: true, output: [what, "  Wrote src/users/users.json"], accounts: listed(), list_error: null });
}

/** The Playwright tab's commands, answered from the sample clone. `real`
 * is the live getAppSettings, which keeps every other setting as it is. */
export function playwrightPatches(real: { getAppSettings: () => Promise<AppSettings> }) {
  return {
    getAppSettings: async () => ({ ...(await real.getAppSettings()), playwright_clone: demoClone }),
    setPlaywrightClone: async (path: string) => {
      demoClone = path;
      return ok({ ...(await real.getAppSettings()), playwright_clone: demoClone });
    },
    pwRepoStructure: (_clone: string) => ok(STRUCTURE),
    pwRepoRunning: () => ok(null),
    pwRepoStop: (run: number) => {
      void events.pwDone.emit({ run, code: null });
      return ok(null);
    },
    pwList: (_clone: string, _refresh: boolean) => ok(TREE),
    pwRun: (_clone: string, picked: string[], action: RunAction, _mode: unknown, _confirm: boolean) => {
      const run = ++nextRun;
      const count = action === "All" ? TESTS.length : action === "Failed" ? 3 : picked.length;
      setTimeout(() => streamRun(run, count), 50);
      return ok({
        kind: "Started" as const,
        run,
        command: action === "Failed" ? "npx playwright test --last-failed" : "npx playwright test --project=SL --project=Shared",
        summary: action === "Failed" ? "3 failures re-run" : `${count} selected · ${count} matched`,
        warnings: [],
      });
    },
    pwPreview: (_clone: string, picked: string[], _action: RunAction, _mode: unknown) =>
      ok({
        run_list: `# ${picked.length} tests\n${TESTS.filter((t) => picked.includes(t.id)).map((t) => `[${t.project}] > ${t.file}:${t.line}`).join("\n")}`,
        command: "npx playwright test --project=SL",
        expected: picked.length,
        warnings: [],
      }),
    pwReports: (_clone: string) => ok(reports),
    pwOpenReport: (_clone: string, _runId: string) => ok(null),
    pwUsersList: (_clone: string) => ok(listed()),
    pwUsersRefs: (_clone: string) => ok(accounts.map(({ key, specs, other_refs }): AccountRefs => ({ key, specs, other_refs }))),
    pwUsersPassword: (_clone: string, _key: string) => ok("Demo-Only-Pass-2741"),
    pwUsersChange: (_clone: string, change: AccountChange, apply: boolean) => accountChange(change, apply),
    pwSyncStates: (_clone: string, _org: string, _project: string) => ok(SYNC),
    pwHandoff: (_clone: string, _org: string, _project: string, cases: number[]) => ok(handoff(cases)),
    pwSyncUpdate: (_clone: string, _org: string, _project: string, cases: number[]) => ok(written(cases)),
    pwSyncAddNew: () => ok(written(SYNC.filter((r) => r.has_script && !r.in_repo).map((r) => r.case_id))),
    pwExportPreview: (_org: string, _project: string, caseIds: number[]) => ok(exportPreview(caseIds)),
    pwExportSaveMap: (_org: string, _project: string, map: ExportMap) => {
      exportMap = map;
      return ok(null);
    },
    pwExportWrite: (_org: string, _project: string, caseIds: number[]) => ok(written(caseIds)),
  };
}
