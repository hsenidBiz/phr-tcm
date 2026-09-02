/**
 * The sample organisation the guided tour shows. Small on purpose: enough
 * for the screens the tour visits to look alive, and nothing more. It is
 * never written anywhere and never leaves the tour.
 */
import type {
  BoardData,
  CaseHistory,
  DbPresetOut,
  DbServerConfig,
  DetectedTool,
  EnsuredSuite,
  Org,
  PbiHit,
  PlanWithSuites,
  PrOverview,
  Project,
  Step,
  TestCaseFull,
  TestCaseSummary,
  TestPoint,
} from "../bindings";

export const TOUR_ORG = "Northwind";
export const TOUR_PROJECT = "Website";
export const TOUR_REPO_PATH = "C:\\Work\\website";

export const TOUR_ORGS: Org[] = [{ name: TOUR_ORG, url: "https://example.invalid/northwind" }];
export const TOUR_PROJECTS: Project[] = [{ id: "tour-website", name: TOUR_PROJECT }];

export const TOUR_PBI: PbiHit = {
  id: 4821,
  title: "Guest checkout",
  work_item_type: "Product Backlog Item",
};

const steps = (...pairs: [string, string][]): Step[] =>
  pairs.map(([action, expected]) => ({ action, expected }));

export const TOUR_CASES: TestCaseFull[] = [
  {
    id: 90101,
    title: "Guest checkout - a guest can pay by card",
    tags: "Checkout; Regression",
    automation_status: "Not Automated",
    steps: steps(
      ["Add a product to the basket.", "The basket shows one item."],
      ["Choose Check out as guest.", "The delivery details form opens."],
      ["Enter a card and confirm the order.", "The order confirmation appears."],
    ),
    step_ids: ["2", "3", "4"],
    steps_xml: "",
    module_value: "Checkout",
    preconditions: "A product is in stock and nobody is signed in.",
  },
  {
    id: 90102,
    title: "Guest checkout - the delivery address is required",
    tags: "Checkout; Regression",
    automation_status: "Not Automated",
    steps: steps(
      ["Leave the delivery address empty.", "The address box is empty."],
      ["Choose Continue.", "The form asks for a delivery address."],
    ),
    step_ids: ["2", "3"],
    steps_xml: "",
    module_value: "Checkout",
    preconditions: "A product is in the basket.",
  },
  {
    id: 90103,
    title: "Guest checkout - a declined card keeps the basket",
    tags: "Checkout; Payments",
    automation_status: "Planned",
    steps: steps(
      ["Pay with a card that will be declined.", "A message explains the payment failed."],
      ["Open the basket.", "The basket still holds the same item."],
    ),
    step_ids: ["2", "3"],
    steps_xml: "",
    module_value: "Checkout",
    preconditions: "A product is in the basket.",
  },
  {
    id: 90104,
    title: "Guest checkout - the order confirmation is emailed",
    tags: "Checkout",
    automation_status: "Not Automated",
    steps: steps(
      ["Complete an order as a guest.", "The order confirmation appears."],
      ["Open the guest's inbox.", "A confirmation email has arrived."],
    ),
    step_ids: ["2", "3"],
    steps_xml: "",
    module_value: "Checkout",
    preconditions: "A guest email address is available.",
  },
];

export const TOUR_CASE_SUMMARIES: TestCaseSummary[] = TOUR_CASES.map(
  ({ id, title, tags, automation_status }) => ({ id, title, tags, automation_status }),
);

export const TOUR_SUITE: EnsuredSuite = {
  plan_id: 7001,
  plan_name: "Website - 2026 R1",
  suite_id: 7042,
  created_plan: false,
};

export const TOUR_POINTS: TestPoint[] = TOUR_CASES.map((c, i) => ({
  point_id: 8100 + i,
  test_case_id: c.id,
  test_case_name: c.title,
  config_name: "Windows / Edge",
  tester: "Sam Taylor",
  last_outcome: ["Passed", "Failed", "Passed", "None"][i],
  last_run_id: null,
  last_result_id: null,
}));

export const TOUR_PLANS: PlanWithSuites[] = [
  {
    plan: { id: 7001, name: "Website - 2026 R1", area_path: "Website", root_suite_id: 7002 },
    suites: [
      {
        id: 7010,
        name: "Checkout",
        suite_type: "StaticTestSuite",
        requirement_id: null,
        parent_id: null,
      },
      {
        id: 7042,
        name: "Guest checkout",
        suite_type: "RequirementTestSuite",
        requirement_id: TOUR_PBI.id,
        parent_id: 7010,
      },
      {
        id: 7011,
        name: "Sign in",
        suite_type: "StaticTestSuite",
        requirement_id: null,
        parent_id: null,
      },
    ],
  },
];

export const TOUR_HISTORY: CaseHistory[] = [];

const st = (name: string, color: string, category: string) => ({ name, color, category });

export const TOUR_BOARD: BoardData = {
  items: [
    { id: 4821, title: "Guest checkout", work_item_type: "Product Backlog Item", state: "Committed", state_color: "007acc", column: "In Progress", assigned_to: "Sam Taylor", tags: "Checkout", priority: 2, changed_date: "2026-08-28T09:15:00Z" },
    { id: 4822, title: "The basket keeps items for 30 days", work_item_type: "Product Backlog Item", state: "New", state_color: "b2b2b2", column: "To Do", assigned_to: "Sam Taylor", tags: "Basket", priority: 2, changed_date: "2026-08-27T11:02:00Z" },
    { id: 4830, title: "Write the checkout test cases", work_item_type: "Task", state: "In Progress", state_color: "007acc", column: "In Progress", assigned_to: "Sam Taylor", tags: "", priority: 1, changed_date: "2026-08-28T14:40:00Z" },
    { id: 4831, title: "Card errors show the wrong message", work_item_type: "Bug", state: "New", state_color: "cc293d", column: "To Do", assigned_to: "Sam Taylor", tags: "Payments", priority: 1, changed_date: "2026-08-26T08:20:00Z" },
    { id: 4805, title: "Sign-in remembers me", work_item_type: "Product Backlog Item", state: "Done", state_color: "339947", column: "Done", assigned_to: "Sam Taylor", tags: "Sign in", priority: 3, changed_date: "2026-08-21T16:05:00Z" },
  ],
  states_by_type: {
    "Product Backlog Item": [
      st("New", "b2b2b2", "Proposed"),
      st("Committed", "007acc", "InProgress"),
      st("Done", "339947", "Completed"),
    ],
    Task: [
      st("To Do", "b2b2b2", "Proposed"),
      st("In Progress", "007acc", "InProgress"),
      st("Done", "339947", "Completed"),
    ],
    Bug: [
      st("New", "cc293d", "Proposed"),
      st("Active", "007acc", "InProgress"),
      st("Closed", "339947", "Completed"),
    ],
  },
};

export const TOUR_PR_OVERVIEW: PrOverview = { awaiting: [], mine: [] };

export const TOUR_TOOLS: DetectedTool[] = [
  {
    id: "claude-code",
    name: "Claude Code",
    installed: true,
    registered_servers: ["tcm-testcases"],
    scope: "project",
    global_registered_servers: [],
  },
  {
    id: "vscode",
    name: "VS Code",
    installed: true,
    registered_servers: [],
    scope: "project",
    global_registered_servers: [],
  },
  {
    id: "cursor",
    name: "Cursor",
    installed: true,
    registered_servers: [],
    scope: "project",
    global_registered_servers: [],
  },
];

export const TOUR_DB_PRESETS: DbPresetOut[] = [
  { label: "Sample - read only", connection_string: "Server=sample;Database=Northwind;User Id=reader;" },
];

// The AI Bridge screen's own default config - shown before the user picks a
// preset or types their own. Obviously sample: no real host or credential.
export const TOUR_DB_DEFAULTS: DbServerConfig = {
  exe_path: "C:\\Program Files\\Test Case Manager\\PeoplesHR.DBMCPServer.exe",
  db_type: "mssql",
  connection_string: "Server=sample;Database=Northwind;User Id=reader;",
  schema_filter: "dbo",
};

export const TOUR_BRIDGE = {
  port: 51999,
  mcp_exe: "C:\\Program Files\\Test Case Manager\\app.exe",
};
