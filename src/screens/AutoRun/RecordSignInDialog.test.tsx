// The Record sign-in dialog: the address and account it needs, a recording
// driven by mocked events, the review of what each field is, and a save
// that is checked first.

import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { DraftStepView, FieldChoice, RecordingEvent, SignInDraftView } from "../../bindings";
import { toast } from "../../lib/toast";
import RecordSignInDialog, { defaultFieldRoles, stepWords } from "./RecordSignInDialog";

vi.mock("../../lib/toast", () => ({ toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
afterEach(() => {
  clearMocks();
  localStorage.clear();
  vi.clearAllMocks();
});

const ACCOUNTS = [{ key: "hr.admin", label: "HR Admin", username: "kim", password: "p" }];
const RECIPE = {
  start_url: "https://hr.example.internal/login",
  steps: [{ kind: "click", selector: { css: "#go" } }],
  signed_in: { css: "#marker" },
  allowed_origins: [],
  session_minutes: 480,
};

const DRAFT: SignInDraftView = {
  steps: [
    { kind: "field", readable: 'textbox "Email"', password: false },
    { kind: "click", readable: 'button "Next"', password: false },
    { kind: "field", readable: 'textbox "Password"', password: true },
    { kind: "field", readable: 'textbox "Domain"', password: false },
    { kind: "click", readable: 'button "Sign in"', password: false },
  ],
  marker: 'link "Kim"',
};

type Handler = (cmd: string, args: Record<string, unknown>) => unknown;

function mount(handler: Handler, opts: { accounts?: unknown[]; recipe?: unknown; env?: unknown } = {}) {
  const onClose = vi.fn();
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_list_accounts") return opts.accounts ?? ACCOUNTS;
      if (cmd === "auto_run_load_recipe") return opts.recipe ?? null;
      if (cmd === "env_list" && opts.env !== undefined) return opts.env;
      const out = handler(String(cmd), (args ?? {}) as Record<string, unknown>);
      return out === undefined ? null : out;
    },
    { shouldMockEvents: true },
  );
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RecordSignInDialog org="acme" project="Web" onClose={onClose} />
    </QueryClientProvider>,
  );
  return onClose;
}

async function send(payload: RecordingEvent) {
  const { emit } = await import("@tauri-apps/api/event");
  await act(async () => {
    await emit("recording-event", payload);
  });
}

const step = (kind: string, index: number, readable: string, password = false): RecordingEvent => ({
  kind,
  index,
  readable,
  detail: "",
  password,
});

async function startRecording() {
  const start = await screen.findByRole("button", { name: "Start" });
  await waitFor(() => expect(start).toBeEnabled());
  fireEvent.click(start);
  await screen.findByRole("button", { name: "Finish" });
}

/** Record, pick the check, and Finish into the review of `DRAFT`. */
async function toReview() {
  await startRecording();
  await send(step("marker", 0, 'link "Kim"'));
  fireEvent.click(await screen.findByRole("button", { name: "Finish" }));
  await screen.findByRole("button", { name: "Check and save" });
}

/** A recorder whose Stop hands back `DRAFT`; `save` answers the save. */
function recorder(save: Handler = () => ({ saved: true, failure: "" }), extra: Handler = () => undefined): Handler {
  return (cmd, args) => {
    const answered = extra(cmd, args);
    if (answered !== undefined) return answered;
    if (cmd === "auto_run_record_sign_in_start") return null;
    if (cmd === "auto_run_record_sign_in_pick") return null;
    if (cmd === "auto_run_record_sign_in_stop") return DRAFT;
    if (cmd === "auto_run_record_sign_in_save") return save(cmd, args);
    return undefined;
  };
}

test("Start needs an address and an account, and records with them in the chosen browser", async () => {
  const started: unknown[] = [];
  mount((cmd, args) => {
    if (cmd === "auto_run_record_sign_in_start") {
      started.push(args);
      return null;
    }
  });
  const start = await screen.findByRole("button", { name: "Start" });
  await waitFor(() => expect(screen.getByRole("combobox", { name: "Check with account" })).toHaveTextContent("HR Admin (hr.admin)"));
  expect(start).toBeDisabled();
  expect(screen.getByText(/never what you type/)).toBeInTheDocument();

  fireEvent.change(screen.getByRole("textbox", { name: "Start address" }), { target: { value: "  https://hr.example.internal/  " } });
  expect(start).toBeEnabled();
  fireEvent.change(screen.getByRole("textbox", { name: "Start address" }), { target: { value: "   " } });
  expect(start).toBeDisabled();

  localStorage.setItem("tcm-v2-autorun-browser", "chrome");
  fireEvent.change(screen.getByRole("textbox", { name: "Start address" }), { target: { value: "https://hr.example.internal/" } });
  fireEvent.click(start);
  await screen.findByRole("button", { name: "Finish" });
  expect(started).toEqual([
    { organization: "acme", project: "Web", startUrl: "https://hr.example.internal/", browserName: "chrome" },
  ]);
});

test("the address comes from the saved recipe, and with no accounts Start stays disabled and says why", async () => {
  mount(() => undefined, { accounts: [], recipe: RECIPE });
  const address = await screen.findByRole("textbox", { name: "Start address" });
  await waitFor(() => expect(address).toHaveValue("https://hr.example.internal/login"));
  expect(await screen.findByText(/Add an account in Accounts first/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Start" })).toBeDisabled();
});

const ENV_LIST = (start_url: string) => ({
  active: "qa",
  environments: [
    { id: "qa", name: "QA", start_url, allowed_origins: [], db_id: "db", test_environment: false, has_default_password: false },
  ],
});

test("the address is the active environment's when it has one, and the recipe's when it has none", async () => {
  mount(() => undefined, { recipe: RECIPE, env: ENV_LIST("https://qa.example.internal/") });
  const address = await screen.findByRole("textbox", { name: "Start address" });
  await waitFor(() => expect(address).toHaveValue("https://qa.example.internal/"));
});

test("an environment with no address of its own leaves the recipe's address in the box", async () => {
  mount(() => undefined, { recipe: RECIPE, env: ENV_LIST("") });
  const address = await screen.findByRole("textbox", { name: "Start address" });
  await waitFor(() => expect(address).toHaveValue("https://hr.example.internal/login"));
});

test("the recording lists steps as they arrive, says which field is the password, and shows notes", async () => {
  mount(recorder());
  fireEvent.change(await screen.findByRole("textbox", { name: "Start address" }), {
    target: { value: "https://hr.example.internal/" },
  });
  await startRecording();
  await send(step("field", 1, 'textbox "Email"'));
  await send(step("click", 2, 'button "Next"'));
  await send(step("field", 3, 'textbox "Password"', true));
  await send({ kind: "unreadable", index: 0, readable: "", detail: "that click could not be named", password: false });
  const list = screen.getByRole("list", { name: "Recorded steps" });
  expect(within(list).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
    '1. Type into textbox "Email"',
    '2. Click button "Next"',
    '3. Type into textbox "Password" (password field)',
  ]);
  expect(screen.getByText("that click could not be named")).toBeInTheDocument();
});

test("Finish waits for the signed-in check, which I'm signed in asks for and Pick again replaces", async () => {
  let picks = 0;
  mount(recorder(undefined, (cmd) => {
    if (cmd === "auto_run_record_sign_in_pick") picks += 1;
  }), { recipe: RECIPE });
  await startRecording();
  await send(step("field", 1, 'textbox "Email"'));
  expect(screen.getByRole("button", { name: "Finish" })).toBeDisabled();

  fireEvent.click(screen.getByRole("button", { name: "I'm signed in" }));
  expect(
    await screen.findByText(/Now click Sign out, or something every signed-in account sees - not your own name/),
  ).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Finish" })).toBeDisabled();

  await send(step("marker", 0, 'link "Kim"'));
  expect(screen.getByText('link "Kim"')).toBeInTheDocument();
  expect(screen.queryByText(/Now click Sign out/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Finish" })).toBeEnabled();

  fireEvent.click(screen.getByRole("button", { name: "Pick again" }));
  // Pick mode is on again, and the dialog says so while the old check stays.
  expect(await screen.findByText(/Now click Sign out/)).toBeInTheDocument();
  expect(screen.getByText('link "Kim"')).toBeInTheDocument();
  await send(step("marker", 0, 'button "Sign out"'));
  expect(screen.getByText('button "Sign out"')).toBeInTheDocument();
  expect(screen.queryByText('link "Kim"')).not.toBeInTheDocument();
  await waitFor(() => expect(picks).toBe(2));
});

test("default roles: a password field is the password, the first other field the username, the rest fixed text", () => {
  expect(defaultFieldRoles(DRAFT.steps)).toEqual([
    { role: "username", text: "" },
    { role: "password", text: "" },
    { role: "text", text: "" },
  ]);
  const passwordFirst: DraftStepView[] = [
    { kind: "field", readable: "#pw", password: true },
    { kind: "field", readable: "#user", password: false },
  ];
  expect(defaultFieldRoles(passwordFirst).map((c) => c.role)).toEqual(["password", "username"]);
  // The email corrected after the password is still the username.
  const retyped: DraftStepView[] = [
    { kind: "field", readable: 'textbox "Email"', password: false },
    { kind: "field", readable: 'textbox "Password"', password: true },
    { kind: "field", readable: 'textbox "Email"', password: false },
    { kind: "field", readable: 'textbox "Domain"', password: false },
  ];
  expect(defaultFieldRoles(retyped).map((c) => c.role)).toEqual(["username", "password", "username", "text"]);
  expect(defaultFieldRoles([{ kind: "click", readable: "#go", password: false }])).toEqual([]);
  expect(stepWords({ kind: "click", readable: 'link "Next"', password: false })).toBe('Click link "Next"');
});

test("the review starts from the default roles, and fixed text must be written before Check and save", async () => {
  mount(recorder(), { recipe: RECIPE });
  await toReview();
  expect(screen.getByRole("combobox", { name: "Step 1 is" })).toHaveTextContent("Username");
  expect(screen.getByRole("combobox", { name: "Step 3 is" })).toHaveTextContent("Password");
  expect(screen.getByRole("combobox", { name: "Step 4 is" })).toHaveTextContent("Fixed text");
  expect(screen.queryByRole("combobox", { name: "Step 2 is" })).not.toBeInTheDocument();
  expect(screen.getByText('2. Click button "Next"')).toBeInTheDocument();
  expect(screen.getByText(/never put a password here/)).toBeInTheDocument();

  const save = screen.getByRole("button", { name: "Check and save" });
  expect(save).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "Fixed text for step 4" }), { target: { value: "   " } });
  expect(save).toBeDisabled();
  fireEvent.change(screen.getByRole("textbox", { name: "Fixed text for step 4" }), { target: { value: "CORP" } });
  expect(save).toBeEnabled();

  // Choosing Username instead takes the fixed text box away.
  fireEvent.click(screen.getByRole("combobox", { name: "Step 4 is" }));
  fireEvent.click(await screen.findByRole("option", { name: "Username" }));
  expect(screen.queryByRole("textbox", { name: "Fixed text for step 4" })).not.toBeInTheDocument();
});

test("a failed check shows why and keeps the review; a second try saves, closes and refreshes the card", async () => {
  const saves: { fields: FieldChoice[]; account: string }[] = [];
  let answer = { saved: false, failure: "the check did not sign in - nothing was saved: sign-in stopped at step 3" };
  let recipeReads = 0;
  const onClose = vi.fn();
  mockIPC(
    (cmd, args) => {
      if (cmd === "auto_run_list_accounts") return ACCOUNTS;
      if (cmd === "auto_run_load_recipe") {
        recipeReads += 1;
        return RECIPE;
      }
      if (cmd === "auto_run_record_sign_in_save") {
        saves.push(args as never);
        return answer;
      }
      return recorder()(String(cmd), (args ?? {}) as Record<string, unknown>) ?? null;
    },
    { shouldMockEvents: true },
  );
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <RecordSignInDialog org="acme" project="Web" onClose={onClose} />
    </QueryClientProvider>,
  );
  await toReview();
  fireEvent.change(screen.getByRole("textbox", { name: "Fixed text for step 4" }), { target: { value: "CORP" } });
  fireEvent.click(screen.getByRole("button", { name: "Check and save" }));
  expect(await screen.findByText(answer.failure)).toBeInTheDocument();
  expect(screen.getByRole("combobox", { name: "Step 1 is" })).toHaveTextContent("Username");
  expect(screen.getByRole("textbox", { name: "Fixed text for step 4" })).toHaveValue("CORP");
  expect(saves[0]).toEqual({
    organization: "acme",
    project: "Web",
    account: "hr.admin",
    browserName: "edge",
    fields: [
      { role: "username", text: "" },
      { role: "password", text: "" },
      { role: "text", text: "CORP" },
    ],
  });
  expect(onClose).not.toHaveBeenCalled();

  answer = { saved: true, failure: "" };
  const readsBefore = recipeReads;
  fireEvent.click(screen.getByRole("button", { name: "Check and save" }));
  await waitFor(() => expect(onClose).toHaveBeenCalled());
  expect(toast.success).toHaveBeenCalledWith("Sign-in recipe recorded and saved.");
  expect(recipeReads).toBeGreaterThan(readsBefore);
  expect(saves).toHaveLength(2);
});

test("the check shows it is running, and Cancel cancels it and keeps the review", async () => {
  let resolveSave: ((v: unknown) => void) | null = null;
  let cancels = 0;
  mount(
    recorder(
      () =>
        new Promise((resolve) => {
          resolveSave = resolve;
        }),
      (cmd) => {
        if (cmd === "auto_run_record_cancel") cancels += 1;
      },
    ),
    { recipe: RECIPE },
  );
  await toReview();
  fireEvent.change(screen.getByRole("textbox", { name: "Fixed text for step 4" }), { target: { value: "CORP" } });
  fireEvent.click(screen.getByRole("button", { name: "Check and save" }));
  expect(await screen.findByText(/This can take a minute/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Checking" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(cancels).toBe(1));
  await act(async () => {
    resolveSave?.({ saved: false, failure: "the check was cancelled - the recipe was not saved" });
  });
  expect(await screen.findByRole("button", { name: "Check and save" })).toBeEnabled();
  expect(screen.queryByText("the check was cancelled - the recipe was not saved")).not.toBeInTheDocument();
  expect(toast.info).toHaveBeenCalledWith("The check was cancelled. The recipe was not saved.");
});

test("Cancel during a recording cancels it, and Record again goes back to the start", async () => {
  let cancels = 0;
  mount(
    recorder(undefined, (cmd) => {
      if (cmd === "auto_run_record_cancel") cancels += 1;
    }),
    { recipe: RECIPE },
  );
  await startRecording();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(cancels).toBe(1));
  expect(await screen.findByRole("button", { name: "Start" })).toBeInTheDocument();
  expect(toast.info).toHaveBeenCalledWith("The recording was cancelled. Nothing was saved.");

  await toReview();
  fireEvent.click(screen.getByRole("button", { name: "Record again" }));
  expect(await screen.findByRole("button", { name: "Start" })).toBeInTheDocument();
});

test("Cancel while the browser is opening cancels the pending Start", async () => {
  let rejectStart: ((reason: string) => void) | null = null;
  let cancels = 0;
  mount(
    (cmd) => {
      if (cmd === "auto_run_record_sign_in_start") {
        return new Promise((_resolve, reject) => {
          rejectStart = reject;
        });
      }
      if (cmd === "auto_run_record_cancel") cancels += 1;
    },
    { recipe: RECIPE },
  );
  const start = await screen.findByRole("button", { name: "Start" });
  await waitFor(() => expect(start).toBeEnabled());
  fireEvent.click(start);
  fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(cancels).toBe(1));
  await act(async () => {
    rejectStart?.("the recording was cancelled - nothing was saved");
  });
  expect(await screen.findByRole("button", { name: "Start" })).toBeInTheDocument();
  expect(toast.info).toHaveBeenCalledWith("The recording was cancelled. Nothing was saved.");
});

test("closing the recording browser ends the recording and frees the recorder", async () => {
  let cancels = 0;
  mount(
    recorder(undefined, (cmd) => {
      if (cmd === "auto_run_record_cancel") cancels += 1;
    }),
    { recipe: RECIPE },
  );
  await startRecording();
  await send({ kind: "closed", index: 0, readable: "", detail: "the recording browser was closed - nothing was saved", password: false });
  expect(await screen.findByText("The recording browser was closed. Nothing was saved.")).toBeInTheDocument();
  await waitFor(() => expect(cancels).toBe(1));
});

/** Emits `closed` the moment `name`'s button is in the page - from the
 * page-change notice of the very commit that shows it, before React's
 * passive effects have run. React's scheduler is made to yield after every
 * task while this waits (its clock jumps on each read), which is what a
 * busy machine does: the commit lands in one task and the passive effects
 * in a later one, and the close arrives in between. */
function closeAsSoonAsShown(name: string): () => void {
  let t = performance.now();
  const clock = vi.spyOn(performance, "now").mockImplementation(() => (t += 50));
  const seen = new MutationObserver(() => {
    if (!screen.queryByRole("button", { name })) return;
    seen.disconnect();
    void import("@tauri-apps/api/event").then(({ emit }) =>
      emit("recording-event", { kind: "closed", index: 0, readable: "", detail: "closed", password: false }),
    );
  });
  seen.observe(document.body, { childList: true, subtree: true });
  return () => {
    seen.disconnect();
    clock.mockRestore();
  };
}

test("a close that arrives as the recording screen appears still frees the recorder", async () => {
  let cancels = 0;
  mount(
    recorder(undefined, (cmd) => {
      if (cmd === "auto_run_record_cancel") cancels += 1;
    }),
    { recipe: RECIPE },
  );
  const start = await screen.findByRole("button", { name: "Start" });
  await waitFor(() => expect(start).toBeEnabled());
  const stop = closeAsSoonAsShown("Finish");
  try {
    fireEvent.click(start);
    expect(await screen.findByText("The recording browser was closed. Nothing was saved.")).toBeInTheDocument();
  } finally {
    stop();
  }
  await waitFor(() => expect(cancels).toBe(1));
});

test("a recording left open from before can be cancelled when the dialog opens", async () => {
  let open = true;
  let cancels = 0;
  mount((cmd) => {
    if (cmd === "auto_run_recording_is_open") return open;
    if (cmd === "auto_run_record_cancel") {
      cancels += 1;
      open = false;
      return null;
    }
  });
  fireEvent.click(await screen.findByRole("button", { name: "Cancel recording" }));
  await waitFor(() => expect(cancels).toBe(1));
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Cancel recording" })).not.toBeInTheDocument(),
  );
});

/// Nothing a person typed ever reaches this dialog: the event, the draft
/// and a step carry locator words and a password flag, no value.
test("what the dialog is sent carries no typed value", () => {
  const eventKeys: (keyof RecordingEvent)[] = ["kind", "index", "readable", "detail", "password"];
  const stepKeys: (keyof DraftStepView)[] = ["kind", "readable", "password"];
  const draftKeys: (keyof SignInDraftView)[] = ["steps", "marker"];
  expect(Object.keys(step("field", 1, 'textbox "Email"')).sort()).toEqual([...eventKeys].sort());
  expect(Object.keys(DRAFT.steps[0]).sort()).toEqual([...stepKeys].sort());
  expect(Object.keys(DRAFT).sort()).toEqual([...draftKeys].sort());
  // A compile-time check as much as a runtime one: `value` is not a key of
  // any of these, so this line would not type-check if one grew it.
  type NoValue<T> = "value" extends keyof T ? never : true;
  const none: [NoValue<RecordingEvent>, NoValue<DraftStepView>, NoValue<SignInDraftView>] = [true, true, true];
  expect(none).toEqual([true, true, true]);
});

test("the checking account can be changed in the review, and not while the check runs", async () => {
  const saves: { account: string }[] = [];
  let resolveSave: ((v: unknown) => void) | null = null;
  mount(
    recorder((_cmd, args) => {
      saves.push(args as never);
      return new Promise((resolve) => {
        resolveSave = resolve;
      });
    }),
    {
      recipe: RECIPE,
      accounts: [...ACCOUNTS, { key: "hr.clerk", label: "HR Clerk", username: "lee", password: "p" }],
    },
  );
  await toReview();
  const pick = screen.getByRole("combobox", { name: "Check with account" });
  expect(pick).toHaveTextContent("HR Admin (hr.admin)");
  fireEvent.click(pick);
  fireEvent.click(await screen.findByRole("option", { name: "HR Clerk (hr.clerk)" }));
  fireEvent.change(screen.getByRole("textbox", { name: "Fixed text for step 4" }), { target: { value: "CORP" } });
  fireEvent.click(screen.getByRole("button", { name: "Check and save" }));
  await waitFor(() => expect(saves).toHaveLength(1));
  expect(saves[0].account).toBe("hr.clerk");
  expect(screen.getByRole("combobox", { name: "Check with account" })).toBeDisabled();
  await act(async () => {
    resolveSave?.({ saved: false, failure: "the check did not sign in - nothing was saved: x" });
  });
  expect(await screen.findByRole("combobox", { name: "Check with account" })).toBeEnabled();
});
