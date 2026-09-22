// Reviewing a run: nothing is preselected, "Accept every proposal" only
// fills what nobody has decided yet, and saving writes the whole run with
// just verdict/note replaced - proposed/reason/steps must survive byte for
// byte. A sent run is read only.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Toaster } from "sonner";
import { afterEach, expect, test, vi } from "vitest";
import RunReview from "./RunReview";

afterEach(() => {
  clearMocks();
});

/** Case 1: proposed Failed, with a Sign in step, a failed step 2 that has
 * its own screenshot, and a step 3 skipped because step 2 already failed.
 * Case 2: proposed Passed. Case 3: nothing proposed. */
const RUN = {
  id: "run-1",
  pbi_id: 42,
  started_at: "1786000200000",
  mode: "unattended",
  cases: [
    {
      case_id: 201,
      title: "Valid login",
      verdict: "",
      note: "",
      proposed: "Failed",
      reason: 'step 2: button "Save" not found',
      steps: [
        { step_number: 0, outcomes: [{ ok: true, detail: "signed in as tester1" }] },
        {
          step_number: 2,
          outcomes: [{ ok: false, detail: 'button "Save" not found' }],
          screenshot: "shot-201-2.png",
        },
        { step_number: 3, outcomes: [{ ok: false, detail: "not run: an earlier step of this case failed" }] },
      ],
    },
    {
      case_id: 202,
      title: "Locked account",
      verdict: "",
      note: "",
      proposed: "Passed",
      reason: "every action of 2 steps passed",
      steps: [
        { step_number: 1, outcomes: [{ ok: true, detail: "page contains Locked out" }] },
        { step_number: 2, outcomes: [{ ok: true, detail: "page contains Locked out" }] },
      ],
    },
    {
      case_id: 203,
      title: "Password reset",
      verdict: "",
      note: "",
      proposed: "",
      reason: "this script checks nothing, so there is nothing to propose",
      steps: [],
    },
  ],
};

function renderReview(run: unknown, overrides: { onClose?: () => void } = {}) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_run") return run;
    if (cmd === "auto_run_shot") {
      const a = args as { name: string };
      return `data:image/png;base64,${a.name}`;
    }
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onClose = overrides.onClose ?? vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <RunReview
        org="acme"
        project="Web"
        pbiTitle="Login flow"
        runId="run-1"
        stepIds={{}}
        onClose={onClose}
      />
    </QueryClientProvider>,
  );
  return { onClose };
}

function caseCard(caseId: number) {
  return screen.getByRole("listitem", { name: new RegExp(`#${caseId}`) });
}

test("a proposal is shown and nothing is preselected", async () => {
  renderReview(RUN);

  expect(await screen.findByText(/proposed: failed - step 2/i)).toBeInTheDocument();
  expect(screen.getByText(/proposed: passed - every action/i)).toBeInTheDocument();
  expect(screen.getByText(/nothing proposed - this script checks nothing/i)).toBeInTheDocument();

  expect(screen.queryAllByRole("button", { pressed: true })).toHaveLength(0);
  // Nine verdict buttons total: three cases, three verdicts each.
  expect(screen.getAllByRole("button", { pressed: false })).toHaveLength(9);
});

test("accept every proposal fills only the unset ones that have a proposal", async () => {
  renderReview(RUN);
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(within(caseCard(202)).getByRole("button", { name: "Blocked" }));
  fireEvent.click(screen.getByRole("button", { name: "Accept every proposal" }));

  expect(within(caseCard(201)).getByRole("button", { name: "Failed" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(within(caseCard(202)).getByRole("button", { name: "Blocked" })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  expect(within(caseCard(202)).getByRole("button", { name: "Passed" })).toHaveAttribute(
    "aria-pressed",
    "false",
  );
  for (const v of ["Passed", "Failed", "Blocked"]) {
    expect(within(caseCard(203)).getByRole("button", { name: v })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  }
  expect(await screen.findByText("2 of 3 confirmed")).toBeInTheDocument();
});

test("saving writes the verdicts and notes and leaves the proposal alone", async () => {
  let saved: Record<string, unknown> | null = null;
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_run") return RUN;
    if (cmd === "auto_run_save_run") {
      saved = args as Record<string, unknown>;
      return null;
    }
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <RunReview org="acme" project="Web" pbiTitle="Login flow" runId="run-1" stepIds={{}} onClose={() => {}} />
    </QueryClientProvider>,
  );
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(within(caseCard(201)).getByRole("button", { name: "Failed" }));
  fireEvent.change(within(caseCard(201)).getByLabelText("Note for #201"), {
    target: { value: "wrong button label" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save review" }));

  await waitFor(() => expect(saved).not.toBeNull());
  const run = (saved as unknown as { run: typeof RUN }).run;
  expect(run.cases[0].verdict).toBe("Failed");
  expect(run.cases[0].note).toBe("wrong button label");
  // Every case not touched by hand keeps its own untouched verdict/note.
  expect(run.cases[1].verdict).toBe("");
  expect(run.cases[2].verdict).toBe("");
  // The evidence the machine produced must survive exactly as loaded.
  expect(run.cases[0].proposed).toBe(RUN.cases[0].proposed);
  expect(run.cases[0].reason).toBe(RUN.cases[0].reason);
  expect(run.cases[0].steps).toEqual(RUN.cases[0].steps);
  expect(run.cases[1].steps).toEqual(RUN.cases[1].steps);
  expect(run.cases[2].steps).toEqual(RUN.cases[2].steps);
});

test("the steps unfold, the sign-in is named, and a picture can be opened", async () => {
  renderReview(RUN);
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(within(caseCard(201)).getByRole("button", { name: "Show steps" }));

  const card = caseCard(201);
  expect(within(card).getByText("Sign in")).toBeInTheDocument();
  expect(within(card).getByText("Step 2")).toBeInTheDocument();
  expect(within(card).getByText('button "Save" not found')).toBeInTheDocument();
  expect(within(card).getByText("not run: an earlier step of this case failed")).toBeInTheDocument();

  fireEvent.click(within(card).getByRole("button", { name: "Picture" }));

  // The preview is its own Modal, portaled to document.body alongside the
  // review dialog rather than nested inside this case's card - the same
  // shape RunPane's own screenshot preview uses.
  const img = await screen.findByRole("img");
  expect(img).toHaveAttribute("src", "data:image/png;base64,shot-201-2.png");
});

test("a run that was sent is read only", async () => {
  const sent = {
    ...RUN,
    cases: RUN.cases.map((c) => ({ ...c, verdict: c.proposed || "Blocked" })),
    published: { run_id: 9, web_url: "https://dev.azure.com/acme/_testManagement/runs/9", at: "1786000300000" },
  };
  renderReview(sent);
  await screen.findByText(/proposed: failed/i);

  for (const v of ["Passed", "Failed", "Blocked"]) {
    expect(within(caseCard(201)).getByRole("button", { name: v })).toBeDisabled();
  }
  expect(screen.queryByRole("button", { name: "Save review" })).not.toBeInTheDocument();
  expect(await screen.findByText(/sent to azure devops/i)).toBeInTheDocument();
});

test("a run that is gone says so", async () => {
  const { onClose } = renderReview(null);

  expect(await screen.findByText("This run is no longer on this machine.")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("a save the app refuses is shown and the dialog stays open", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_load_run") return RUN;
    if (cmd === "auto_run_save_run") throw "disk is full";
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <RunReview org="acme" project="Web" pbiTitle="Login flow" runId="run-1" stepIds={{}} onClose={() => {}} />
    </QueryClientProvider>,
  );
  render(<Toaster />);
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(within(caseCard(201)).getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Save review" }));

  expect(await screen.findByText(/disk is full/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save review" })).toBeInTheDocument();
});
