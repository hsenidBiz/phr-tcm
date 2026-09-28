/**
 * DEV-ONLY: the sample the help site's review page screenshots are taken of.
 *
 * `npm run docs:shots` imports this into the running capture app (never
 * the app itself, so it is not in a release build) and calls
 * `writeReviewSample` with a folder it has put a case file and a spec in.
 * The app then writes its real review page - and the Test map beside it -
 * through the same command that keeps an open page in step, which writes
 * the file without opening a browser. The script opens it in Edge.
 *
 * The cases carry one of everything the page shows: a new case and an
 * update, reviewer notes quoting the spec, a finding, a comment, tags,
 * module and status, and areas for the Test map.
 */
import { commands, type DraftFile, type TestCase_Deserialize } from "../bindings";
import { pagePalette } from "../lib/reportTheme";

const AREA = "Customer Portal\\Leave";

export const REVIEW_SAMPLE_CASES: TestCase_Deserialize[] = [
  {
    title: "Leave request - submit a request for approval",
    steps: [
      { action: "Pick Annual leave, 12 to 14 August, and add a reason.", expected: "The form shows 3 days." },
      { action: "Press Submit.", expected: "The request is listed as Pending approval." },
    ],
    tags: "leave; smoke",
    automation_status: "Planned",
    module_value: "Leave",
    preconditions: "An employee with 10 days of annual leave left.",
    update_id: null,
    comment: "Step 3 should also check that the manager receives the approval email.",
    reviewer_notes:
      "Spec: leave-spec.md > 2.1 Submitting a request\n\n> The employee picks the dates and the leave type, and the request goes to their manager for approval.",
    // On the first case, so every part of a case is on screen without
    // scrolling - a scrolled page shifted under the high-density capture.
    findings: [
      {
        kind: "spec",
        subject: "leave-spec.md > 2.1",
        title: "The spec does not say what happens when the dates overlap an existing request",
        detail: "Section 2.1 covers a new request, but not one that overlaps another. Worth asking before this case is final.",
      },
    ],
    area: `${AREA}\\Requests`,
  },
  {
    title: "Leave request - cancel a pending request",
    steps: [
      { action: "Open a request that is Pending approval.", expected: "Its details and a Cancel request button show." },
      { action: "Press Cancel request and confirm.", expected: "The request is listed as Cancelled." },
    ],
    tags: "leave",
    automation_status: "Planned",
    module_value: "Leave",
    preconditions: "A request waiting for approval.",
    update_id: 4312,
    reviewer_notes: "Spec: leave-spec.md > 2.3 Cancelling a request",
    area: `${AREA}\\Requests`,
  },
  {
    title: "Leave balance - shows the days left this year",
    steps: [{ action: "Open Leave.", expected: "The balance shows 10 days of annual leave left." }],
    tags: "leave",
    automation_status: "Planned",
    module_value: "Leave",
    preconditions: "",
    update_id: null,
    reviewer_notes: "Spec: leave-spec.md > 2.4 Leave balance",
    area: `${AREA}\\Balance`,
  },
  {
    title: "Sign in - remember me keeps you signed in",
    steps: [
      { action: "Sign in with Remember me ticked.", expected: "The home page opens." },
      { action: "Close the browser and open the portal again.", expected: "The home page opens without asking to sign in." },
    ],
    tags: "login",
    automation_status: "Automated",
    module_value: "Sign in",
    preconditions: "",
    update_id: 4318,
    area: "Customer Portal\\Sign in",
  },
];

/** Where the sample's spec appears to live, in the shots. The script writes
 *  the files to the temp folder, whose path names the Windows user; the
 *  spec pane's path line shows this instead - in the repository the capture
 *  app's AI Bridge tab shows (CAPTURE_REPO in dev/demo.ts). */
export const REVIEW_SAMPLE_SHOWN_PATH = "C:\\Projects\\customer-portal\\.test-cases\\leave-spec.md";

/** The spec the sample's cases cite, for the file the script writes. */
export const REVIEW_SAMPLE_SPEC = `# Leave requests

## 2.1 Submitting a request

The employee picks the dates and the leave type, and the request goes to their manager for approval.

## 2.2 Approving a request

The manager approves or rejects the request. The employee is told either way.

## 2.3 Cancelling a request

A request that is still waiting for approval can be cancelled by the employee.

## 2.4 Leave balance

The Leave page shows the days of each leave type left this year.
`;

/** Write the sample review page (and its Test map) into the temp folder,
 *  its case file and spec named from `dir`. Resolves once it is written. */
export async function writeReviewSample(dir: string): Promise<void> {
  const file = `${dir}\\leave-cases.json`;
  const files: DraftFile[] = [
    {
      path: file,
      label: "leave-cases.json",
      comment: "Covers sections 2.1 to 2.4 of the leave spec. The sign-in case is from the login spec.",
      specs: ["leave-spec.md"],
    },
  ];
  const r = await commands.refreshDraftHtml(
    REVIEW_SAMPLE_CASES,
    "PBI #1001",
    REVIEW_SAMPLE_CASES.map(() => file),
    REVIEW_SAMPLE_CASES.map((_, i) => `sample:${i}`),
    1001,
    files,
    pagePalette(),
  );
  if (r.status === "error") throw new Error(r.error);
}
