// Reviewing a run: nothing is preselected, "Accept every proposal" only
// fills what nobody has decided yet, and saving writes the whole run with
// just verdict/note replaced - proposed/reason/steps must survive byte for
// byte. A sent run is read only.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Toaster } from "../../components/ui/toaster";
import { afterEach, expect, test, vi } from "vitest";
import RunReview, { stepLabel } from "./RunReview";

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

function renderReview(
  run: unknown,
  overrides: {
    onClose?: () => void;
    pbiTitle?: string;
    /** The PBI currently selected on the Auto Run screen - defaults to
     * the fixture run's own `pbi_id` (42), so existing tests keep
     * exercising the same-PBI path unless they say otherwise. */
    pbiId?: number;
    runId?: string;
    stepIds?: Record<number, string[]>;
    sharedSteps?: Record<number, number[]>;
    /** Extra command handling for tests that need `auto_run_save_run` or
     * `auto_run_publish` to do something other than answer null. */
    extra?: (cmd: string, args: unknown) => unknown;
  } = {},
) {
  mockIPC((cmd, args) => {
    if (cmd === "auto_run_load_run") return run;
    if (cmd === "auto_run_shot") {
      const a = args as { name: string };
      return `data:image/png;base64,${a.name}`;
    }
    return overrides.extra?.(String(cmd), args) ?? null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onClose = overrides.onClose ?? vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <RunReview
        org="acme"
        project="Web"
        pbiTitle={overrides.pbiTitle ?? "Login flow"}
        pbiId={overrides.pbiId ?? 42}
        runId={overrides.runId ?? "run-1"}
        stepIds={overrides.stepIds ?? {}}
        sharedSteps={overrides.sharedSteps ?? {}}
        onClose={onClose}
      />
    </QueryClientProvider>,
  );
  return { onClose };
}

/** A run with two confirmed cases and one still unconfirmed - the shape
 * Task 8's Send button needs: something to send, and something left out. */
const SEND_RUN = {
  id: "run-9",
  pbi_id: 42,
  started_at: "1786000400000",
  mode: "unattended",
  cases: [
    { case_id: 1, title: "Case A", verdict: "Passed", note: "", proposed: "Passed", reason: "ok", steps: [] },
    { case_id: 2, title: "Case B", verdict: "Failed", note: "", proposed: "Failed", reason: "bad", steps: [] },
    { case_id: 3, title: "Case C", verdict: "", note: "", proposed: "", reason: "", steps: [] },
  ],
};
const SEND_STEP_IDS = { 1: ["2", "3", "4"], 2: ["2"] };

function caseCard(caseId: number) {
  return screen.getByRole("listitem", { name: new RegExp(`#${caseId}`) });
}

test("a proposal is shown and nothing is preselected", async () => {
  renderReview(RUN);

  expect(await screen.findByText(/proposed: failed - step 2/i)).toBeInTheDocument();
  expect(screen.getByText(/proposed: passed - every action/i)).toBeInTheDocument();
  expect(screen.getByText(/nothing proposed - this script checks nothing/i)).toBeInTheDocument();

  // Counted inside the verdict pickers only: the result filter above the
  // list is a toggle row too, with All pressed.
  const verdictButtons = screen
    .getAllByRole("group", { name: /^Verdict for #/ })
    .flatMap((g) => within(g).getAllByRole("button"));
  expect(verdictButtons.filter((b) => b.getAttribute("aria-pressed") === "true")).toHaveLength(0);
  // Nine verdict buttons total: three cases, three verdicts each.
  expect(verdictButtons.filter((b) => b.getAttribute("aria-pressed") === "false")).toHaveLength(9);
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
  const counted: unknown[] = [];
  mockIPC((cmd, args) => {
    // A review re-writes a run already counted toward the quirks when the
    // run pane saved it: it must never be counted again.
    if (cmd === "auto_run_count_evidence") counted.push(args);
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
      <RunReview org="acme" project="Web" pbiTitle="Login flow" pbiId={42} runId="run-1" stepIds={{}} sharedSteps={{}} onClose={() => {}} />
    </QueryClientProvider>,
  );
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(within(caseCard(201)).getByRole("button", { name: "Failed" }));
  fireEvent.change(within(caseCard(201)).getByLabelText("Note for #201"), {
    target: { value: "wrong button label" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Save review" }));

  await waitFor(() => expect(saved).not.toBeNull());
  expect(counted).toEqual([]);
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

test("a step the script never checked says why, read from the script itself", async () => {
  renderReview(RUN, {
    extra: (cmd, args) => {
      if (cmd === "auto_run_load_script") {
        const caseId = (args as { caseId: number }).caseId;
        if (caseId !== 201) return null;
        return {
          case_id: 201,
          title: "Valid login",
          steps: [
            {
              step_number: 2,
              actions: [{ kind: "click", selector: "#save" }],
              unchecked: "the PDF cannot be read from the accessibility tree",
            },
          ],
        };
      }
      return null;
    },
  });
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(within(caseCard(201)).getByRole("button", { name: "Show steps" }));

  expect(
    await screen.findByText("not checked: the PDF cannot be read from the accessibility tree"),
  ).toBeInTheDocument();
  // Case 202 has no script loaded at all (mocked to null) - no reason to show.
  expect(within(caseCard(202)).queryByText(/not checked:/)).not.toBeInTheDocument();
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

test("a run that fails to load shows the error, not Loading forever", async () => {
  mockIPC((cmd) => {
    if (cmd === "auto_run_load_run") throw "the disk is unreadable";
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const onClose = vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <RunReview org="acme" project="Web" pbiTitle="Login flow" pbiId={42} runId="run-1" stepIds={{}} sharedSteps={{}} onClose={onClose} />
    </QueryClientProvider>,
  );

  expect(await screen.findByText(/the disk is unreadable/i)).toBeInTheDocument();
  expect(screen.queryByText(/loading/i)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Close" }));
  expect(onClose).toHaveBeenCalledTimes(1);
});

test("a run reviewed for a different PBI cannot be sent from here", async () => {
  const publishCalls: unknown[] = [];
  renderReview(SEND_RUN, {
    pbiId: 99,
    extra: (cmd, args) => {
      if (cmd === "auto_run_publish") {
        publishCalls.push(args);
      }
      return null;
    },
  });
  await screen.findByText(/proposed: passed/i);

  const sendButton = screen.getByRole("button", { name: "Send to Azure DevOps" });
  expect(sendButton).toBeDisabled();
  expect(sendButton).toHaveAttribute("title", "this run is for PBI #42 - select that PBI to send it");
  expect(publishCalls).toHaveLength(0);
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
      <RunReview org="acme" project="Web" pbiTitle="Login flow" pbiId={42} runId="run-1" stepIds={{}} sharedSteps={{}} onClose={() => {}} />
    </QueryClientProvider>,
  );
  render(<Toaster />);
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(within(caseCard(201)).getByRole("button", { name: "Failed" }));
  fireEvent.click(screen.getByRole("button", { name: "Save review" }));

  expect(await screen.findByText(/disk is full/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Save review" })).toBeInTheDocument();
});

test("send is offered only for a saved run with a confirmed verdict that was not sent", async () => {
  let saved: Record<string, unknown> | null = null;
  renderReview(RUN, {
    extra: (cmd, args) => {
      if (cmd === "auto_run_save_run") {
        saved = args as Record<string, unknown>;
        return null;
      }
      return null;
    },
  });
  await screen.findByText(/proposed: failed/i);

  // Fresh, unconfirmed: nothing to send yet.
  expect(screen.getByRole("button", { name: "Send to Azure DevOps" })).toBeDisabled();

  // A verdict was picked but not saved - sending now would send whatever
  // is on disk, not what was just picked.
  fireEvent.click(within(caseCard(201)).getByRole("button", { name: "Failed" }));
  const sendButton = screen.getByRole("button", { name: "Send to Azure DevOps" });
  expect(sendButton).toBeDisabled();
  expect(sendButton).toHaveAttribute("title", "Save the review first");

  // Saved: no longer dirty, and there is a confirmed case to send.
  fireEvent.click(screen.getByRole("button", { name: "Save review" }));
  await waitFor(() => expect(saved).not.toBeNull());
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "Send to Azure DevOps" })).not.toBeDisabled(),
  );
});

test("a published run offers no Send button", async () => {
  const sent = {
    ...RUN,
    cases: RUN.cases.map((c) => ({ ...c, verdict: c.proposed || "Blocked" })),
    published: {
      run_id: 9,
      web_url: "https://dev.azure.com/acme/_testManagement/runs/9",
      at: "1786000300000",
    },
  };
  renderReview(sent);
  await screen.findByText(/proposed: failed/i);

  expect(screen.queryByRole("button", { name: "Send to Azure DevOps" })).not.toBeInTheDocument();
});

test("sending says what it will do and sends only after the person agrees", async () => {
  const publishCalls: unknown[] = [];
  renderReview(SEND_RUN, {
    pbiTitle: "Leave module",
    runId: "run-9",
    stepIds: SEND_STEP_IDS,
    extra: (cmd, args) => {
      if (cmd === "auto_run_publish") {
        publishCalls.push(args);
        return {
          status: "sent",
          run_id: 9,
          web_url: "https://dev.azure.com/acme/_testManagement/runs/9",
          sent: [1, 2],
          skipped: [],
          problems: [],
        };
      }
      return null;
    },
  });
  await screen.findByText(/proposed: passed/i);

  fireEvent.click(screen.getByRole("button", { name: "Send to Azure DevOps" }));

  expect(await screen.findByText(/Leave module/)).toBeInTheDocument();
  expect(screen.getByText(/2 confirmed results/)).toBeInTheDocument();
  expect(screen.getByText(/1 unconfirmed/)).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(publishCalls).toHaveLength(0);
  expect(screen.queryByText(/2 confirmed results/)).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Send to Azure DevOps" }));
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));

  await waitFor(() => expect(publishCalls).toHaveLength(1));
  expect(publishCalls[0]).toEqual({
    organization: "acme",
    project: "Web",
    pbiId: 42,
    runId: "run-9",
    runName: "Leave module - Auto Run",
    cases: [
      { case_id: 1, step_ids: ["2", "3", "4"], shared_steps: [] },
      { case_id: 2, step_ids: ["2"], shared_steps: [] },
    ],
  });
});

test("each case's Shared Steps rows travel with the send", async () => {
  const publishCalls: unknown[] = [];
  renderReview(SEND_RUN, {
    pbiTitle: "Leave module",
    runId: "run-9",
    stepIds: SEND_STEP_IDS,
    sharedSteps: { 1: [2] },
    extra: (cmd, args) => {
      if (cmd === "auto_run_publish") {
        publishCalls.push(args);
        return {
          status: "sent",
          run_id: 9,
          web_url: "https://dev.azure.com/acme/_testManagement/runs/9",
          sent: [1, 2],
          skipped: [],
          problems: [],
        };
      }
      return null;
    },
  });
  await screen.findByText(/proposed: passed/i);
  fireEvent.click(screen.getByRole("button", { name: "Send to Azure DevOps" }));
  fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(publishCalls).toHaveLength(1));
  expect((publishCalls[0] as { cases: unknown[] }).cases).toEqual([
    { case_id: 1, step_ids: ["2", "3", "4"], shared_steps: [2] },
    { case_id: 2, step_ids: ["2"], shared_steps: [] },
  ]);
});

test("what was sent, skipped and went wrong is all shown", async () => {
  renderReview(SEND_RUN, {
    pbiTitle: "Leave module",
    runId: "run-9",
    stepIds: SEND_STEP_IDS,
    extra: (cmd) => {
      if (cmd === "auto_run_publish") {
        return {
          status: "sent",
          run_id: 9,
          web_url: "https://dev.azure.com/acme/_testManagement/runs/9",
          sent: [1, 2],
          skipped: [{ case_id: 3, why: "not confirmed" }],
          problems: ["a comment was too long and was left off"],
        };
      }
      return null;
    },
  });
  await screen.findByText(/proposed: passed/i);

  fireEvent.click(screen.getByRole("button", { name: "Send to Azure DevOps" }));
  fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));

  expect(await screen.findByText(/Sent: 2 results recorded/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Open the run" })).toBeInTheDocument();
  expect(screen.getByText(/not confirmed/)).toBeInTheDocument();
  expect(screen.getByText(/a comment was too long/)).toBeInTheDocument();

  expect(within(caseCard(1)).getByRole("button", { name: "Passed" })).toBeDisabled();
});

test("a refusal is shown in its own words and nothing changes", async () => {
  renderReview(SEND_RUN, {
    pbiTitle: "Leave module",
    runId: "run-9",
    stepIds: SEND_STEP_IDS,
    extra: (cmd) => {
      if (cmd === "auto_run_publish") {
        return { status: "refused", why: "This run has already been sent once." };
      }
      return null;
    },
  });
  await screen.findByText(/proposed: passed/i);

  fireEvent.click(screen.getByRole("button", { name: "Send to Azure DevOps" }));
  fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));

  expect(await screen.findByText("This run has already been sent once.")).toBeInTheDocument();
  expect(within(caseCard(1)).getByRole("button", { name: "Passed" })).not.toBeDisabled();
  expect(screen.getByRole("button", { name: "Send to Azure DevOps" })).not.toBeDisabled();
});

test("a failed send can be tried again", async () => {
  renderReview(SEND_RUN, {
    pbiTitle: "Leave module",
    runId: "run-9",
    stepIds: SEND_STEP_IDS,
    extra: (cmd) => {
      if (cmd === "auto_run_publish") {
        throw { kind: "Http", detail: { status: 500, body: "boom" } };
      }
      return null;
    },
  });
  render(<Toaster />);
  await screen.findByText(/proposed: passed/i);

  fireEvent.click(screen.getByRole("button", { name: "Send to Azure DevOps" }));
  fireEvent.click(await screen.findByRole("button", { name: "Confirm" }));

  expect(await screen.findByText(/boom/i)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Send to Azure DevOps" })).not.toBeDisabled();
  expect(within(caseCard(1)).getByRole("button", { name: "Passed" })).not.toBeDisabled();
});

/** A run with exactly one confirmed case and one unconfirmed one - the
 * shape that reads wrong ("1 confirmed results", "1 unconfirmed cases are
 * left out") if the sentence never branches on the count. */
const ONE_EACH_RUN = {
  id: "run-10",
  pbi_id: 42,
  started_at: "1786000500000",
  mode: "unattended",
  cases: [
    { case_id: 1, title: "Case A", verdict: "Passed", note: "", proposed: "Passed", reason: "ok", steps: [] },
    { case_id: 2, title: "Case B", verdict: "", note: "", proposed: "", reason: "", steps: [] },
  ],
};

test("the confirmation text reads correctly for one", async () => {
  renderReview(ONE_EACH_RUN, { runId: "run-10" });
  await screen.findByText(/proposed: passed/i);

  fireEvent.click(screen.getByRole("button", { name: "Send to Azure DevOps" }));

  expect(await screen.findByText(/1 confirmed result\b/)).toBeInTheDocument();
  expect(screen.queryByText(/1 confirmed results/)).not.toBeInTheDocument();
  expect(await screen.findByText(/1 unconfirmed case is left out/)).toBeInTheDocument();
});

test("the review names the sign-in, the trip to the module and each step", () => {
  expect(stepLabel(0)).toBe("Sign in");
  expect(stepLabel(-1)).toBe("Module");
  expect(stepLabel(3)).toBe("Step 3");
});

test("each case's verdicts are the shared picker, labelled by the case", async () => {
  renderReview(RUN);
  await screen.findByText(/proposed: failed/i);

  for (const id of [201, 202, 203]) {
    const group = screen.getByRole("group", { name: `Verdict for #${id}` });
    expect(within(group).getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Passed",
      "Failed",
      "Blocked",
    ]);
    // The group sits inside its own case's card, not somewhere shared.
    expect(caseCard(id)).toContainElement(group);
  }
  // A second press on the same verdict clears it here - the review's own
  // rule, kept by the shared picker handing the choice back.
  const failed = within(screen.getByRole("group", { name: "Verdict for #201" })).getByRole("button", {
    name: "Failed",
  });
  fireEvent.click(failed);
  expect(failed).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(failed);
  expect(failed).toHaveAttribute("aria-pressed", "false");
});

test("Send to Azure DevOps is the primary button and Save review the outline one", async () => {
  renderReview(SEND_RUN, { stepIds: SEND_STEP_IDS });
  await screen.findByText("2 of 3 confirmed");

  const send = screen.getByRole("button", { name: "Send to Azure DevOps" });
  const save = screen.getByRole("button", { name: "Save review" });
  // The variant classes of components/ui/button: default (primary) fills
  // with the accent, outline draws a border and no fill.
  expect(send.className).toContain("bg-accent");
  expect(save.className).not.toContain("bg-accent");
  expect(save.className).toContain("border-border");
});

test("Accept every proposal sits in the footer beside the confirmed count", async () => {
  renderReview(RUN);
  const count = await screen.findByText("0 of 3 confirmed");
  const accept = screen.getByRole("button", { name: "Accept every proposal" });
  expect(count.parentElement).toContainElement(accept);
  // ...and the footer is below every case, not a row of its own above them.
  const lastCard = caseCard(203);
  expect(lastCard.compareDocumentPosition(accept) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

// Filtering the review by result. A case counts under the person's own
// verdict once confirmed, else under the machine's proposal, else Not run.
const filterRow = () => screen.getByRole("group", { name: "Filter by result" });
const filterButton = (name: RegExp) => within(filterRow()).getByRole("button", { name });

test("the filter row counts each result, and All is pressed by default", async () => {
  renderReview(RUN);
  await screen.findByText(/proposed: failed/i);

  expect(filterButton(/^All \(3\)$/)).toHaveAttribute("aria-pressed", "true");
  expect(filterButton(/^Passed \(1\)$/)).toHaveAttribute("aria-pressed", "false");
  expect(filterButton(/^Failed \(1\)$/)).toBeInTheDocument();
  expect(filterButton(/^Blocked \(0\)$/)).toBeInTheDocument();
  expect(filterButton(/^Not run \(1\)$/)).toBeInTheDocument();
});

test("a filter shows only the matching cases, one filter at a time", async () => {
  renderReview(RUN);
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(filterButton(/^Failed/));
  expect(filterButton(/^Failed/)).toHaveAttribute("aria-pressed", "true");
  expect(filterButton(/^All/)).toHaveAttribute("aria-pressed", "false");
  expect(caseCard(201)).toBeInTheDocument();
  expect(screen.queryByRole("listitem", { name: /#202/ })).not.toBeInTheDocument();
  expect(screen.queryByRole("listitem", { name: /#203/ })).not.toBeInTheDocument();

  fireEvent.click(filterButton(/^Not run/));
  expect(caseCard(203)).toBeInTheDocument();
  expect(screen.queryByRole("listitem", { name: /#201/ })).not.toBeInTheDocument();

  fireEvent.click(filterButton(/^All/));
  expect(screen.getAllByRole("listitem", { name: /^Case #/ })).toHaveLength(3);
});

test("a filter that matches nothing says so", async () => {
  renderReview(RUN);
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(filterButton(/^Blocked/));
  expect(screen.getByText("No case in this run matches that filter.")).toBeInTheDocument();
  expect(screen.queryAllByRole("listitem", { name: /^Case #/ })).toHaveLength(0);
});

test("a confirmed verdict overrides the proposal, and the counts follow it", async () => {
  renderReview(RUN);
  await screen.findByText(/proposed: failed/i);

  // #202 is proposed Passed; the person says Blocked.
  fireEvent.click(within(caseCard(202)).getByRole("button", { name: "Blocked" }));
  expect(filterButton(/^Passed \(0\)$/)).toBeInTheDocument();
  expect(filterButton(/^Blocked \(1\)$/)).toBeInTheDocument();

  fireEvent.click(filterButton(/^Blocked/));
  expect(caseCard(202)).toBeInTheDocument();
  expect(screen.getAllByRole("listitem", { name: /^Case #/ })).toHaveLength(1);
});

test("Accept every proposal and Save act on every case, whatever the filter shows", async () => {
  let saved: { run: typeof RUN } | null = null;
  renderReview(RUN, {
    extra: (cmd, args) => {
      if (cmd === "auto_run_save_run") saved = args as { run: typeof RUN };
      return null;
    },
  });
  await screen.findByText(/proposed: failed/i);

  // Only #201 (proposed Failed) is listed...
  fireEvent.click(filterButton(/^Failed/));
  expect(screen.getByText(/the filter only changes what is listed/i)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Accept every proposal" }));
  // ...but #202's proposal was accepted too.
  expect(screen.getByText("2 of 3 confirmed")).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Save review" }));
  await waitFor(() => expect(saved).not.toBeNull());
  expect(saved!.run.cases.map((c) => [c.case_id, c.verdict])).toEqual([
    [201, "Failed"],
    [202, "Passed"],
    [203, ""],
  ]);
});

test("Send sends every confirmed case, not only the ones the filter shows", async () => {
  const publishCalls: { cases: { case_id: number }[] }[] = [];
  renderReview(SEND_RUN, {
    runId: "run-9",
    stepIds: SEND_STEP_IDS,
    extra: (cmd, args) => {
      if (cmd === "auto_run_publish") {
        publishCalls.push(args as { cases: { case_id: number }[] });
        return { status: "sent", run_id: 9, web_url: "https://example.test/9", sent: [1, 2], skipped: [], problems: [] };
      }
      return null;
    },
  });
  await screen.findByText(/proposed: passed/i);

  fireEvent.click(filterButton(/^Not run/));
  fireEvent.click(screen.getByRole("button", { name: "Send to Azure DevOps" }));
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));

  await waitFor(() => expect(publishCalls).toHaveLength(1));
  expect(publishCalls[0].cases.map((c) => c.case_id)).toEqual([1, 2]);
});

// Review's own flow: filter to Failed, find a false failure, mark it Passed
// - and keep its card to check it and write the note. The counts move at
// once; the list only when a filter is picked again.
test("a case re-bucketed under a filter stays listed, with its note field and focus", async () => {
  renderReview(RUN);
  await screen.findByText(/proposed: failed/i);

  fireEvent.click(filterButton(/^Failed/));
  const passed = within(caseCard(201)).getByRole("button", { name: "Passed" });
  passed.focus();
  fireEvent.click(passed);

  expect(caseCard(201)).toBeInTheDocument();
  expect(within(caseCard(201)).getByLabelText("Note for #201")).toBeInTheDocument();
  expect(document.activeElement).toBe(passed);
  expect(filterButton(/^Failed \(0\)$/)).toBeInTheDocument();
  expect(filterButton(/^Passed \(2\)$/)).toBeInTheDocument();
  expect(screen.queryByText("No case in this run matches that filter.")).not.toBeInTheDocument();

  // Picking the filter again re-reads it: #201 is no longer Failed.
  fireEvent.click(filterButton(/^Passed/));
  fireEvent.click(filterButton(/^Failed/));
  expect(screen.queryByRole("listitem", { name: /#201/ })).not.toBeInTheDocument();
  expect(screen.getByText("No case in this run matches that filter.")).toBeInTheDocument();
});

test("a case run a second time after a transient failure is labelled Retried", async () => {
  const first = "the browser stopped answering at step 1: the browser did not respond in time";
  const retried = {
    ...RUN,
    cases: RUN.cases.map((c) =>
      c.case_id === 202
        ? { ...c, retried: first, reason: `passed on a second try after a transient failure: ${first}` }
        : c,
    ),
  };
  renderReview(retried);

  await screen.findByText(/proposed: passed - passed on a second try/i);
  const label = within(caseCard(202)).getByText("Retried");
  expect(label).toHaveAttribute("title", `Run a second time after a transient failure: ${first}`);
  expect(label).toHaveClass("text-warning");
  expect(within(caseCard(201)).queryByText("Retried")).not.toBeInTheDocument();
});
