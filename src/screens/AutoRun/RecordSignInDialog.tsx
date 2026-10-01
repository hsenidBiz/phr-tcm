// Recording a project's sign-in recipe instead of writing it as JSON: the
// person signs in by hand once in a visible browser, picks something only
// a signed-in person sees, then says which recorded field is the
// username, the password or a fixed text. The recipe is saved only once
// it has signed in on its own in a fresh browser.
//
// Nothing typed in the recording browser ever reaches this dialog: the
// steps arrive as locator words (`textbox "Email"`), and the backend keeps
// the locators themselves for the save. The only text a saved recipe
// holds that the person wrote is the fixed text they give here.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  commands,
  events,
  type DraftStepView,
  type FieldChoice,
  type FieldRole,
  type SignInDraftView,
} from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import {
  IconCancel,
  IconConfirm,
  IconFinish,
  IconPickOnPage,
  IconRecord,
} from "../../lib/actionIcons";
import { effectiveSite, useEnvironments } from "../../lib/environments";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";
import { chosenBrowser } from "./AreasDialog";

/** One recorded step as the recording lists it. */
type Row = { kind: "click" | "field"; readable: string; password: boolean };

type Phase =
  | { kind: "before" }
  | { kind: "starting" }
  | { kind: "recording"; rows: Row[]; notes: string[]; picking: boolean; marker: string }
  | { kind: "finishing" }
  | { kind: "review"; draft: SignInDraftView; roles: FieldChoice[]; failure: string }
  | { kind: "checking"; draft: SignInDraftView; roles: FieldChoice[] }
  | { kind: "failed"; why: string };

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const CANCELLED = "The recording was cancelled. Nothing was saved.";

/** What each field step fills in, before the person changes anything: a
 * password field is the password, the first other field the username,
 * and any further field a fixed text still to be written. A field typed
 * into again (the email corrected after the password) is what it was the
 * first time. */
export function defaultFieldRoles(steps: DraftStepView[]): FieldChoice[] {
  let usernameGiven = false;
  const seen = new Map<string, FieldChoice["role"]>();
  return steps
    .filter((s) => s.kind === "field")
    .map((s): FieldChoice => {
      const again = seen.get(s.readable);
      if (again) return { role: again, text: "" };
      let role: FieldChoice["role"] = "text";
      if (s.password) role = "password";
      else if (!usernameGiven) {
        usernameGiven = true;
        role = "username";
      }
      seen.set(s.readable, role);
      return { role, text: "" };
    });
}

/** A step in the words the recording list uses. */
export function stepWords(step: { kind: string; readable: string; password: boolean }): string {
  if (step.kind === "field") {
    return `Type into ${step.readable}${step.password ? " (password field)" : ""}`;
  }
  return `Click ${step.readable}`;
}

export default function RecordSignInDialog({
  org,
  project,
  onClose,
}: {
  org: string;
  project: string;
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const recipeKey = ["autorun-recipe", org, project];
  // The same queries the Setup card reads, so the address opens filled in
  // and a save shows on the card at once.
  const recipe = useQuery({
    queryKey: recipeKey,
    queryFn: async () => (await unwrapStr(commands.autoRunLoadRecipe(org, project))) ?? null,
    retry: false,
  });
  const accounts = useQuery({
    queryKey: ["autorun-accounts"],
    queryFn: () => unwrapStr(commands.autoRunListAccounts()),
    retry: false,
  });
  // The address a run would go to now: the active environment's when it
  // has one, else the recipe's. Recording there is what makes the saved
  // recipe sign in to the site the person is actually testing.
  const envs = useEnvironments();
  const [start, setStart] = useState<string | null>(null);
  const [picked, setPicked] = useState("");
  const [phase, setPhase] = useState<Phase>({ kind: "before" });
  /** Something from before this dialog opened still holds the recorder. */
  const [leftOpen, setLeftOpen] = useState(false);
  /** Whether this dialog asked to cancel the Start or check it is waiting
   * on: that call's answer is then read as a cancel, whatever it says. */
  const cancelAsked = useRef(false);

  const startValue = start ?? effectiveSite(envs.data, recipe.data).start_url;
  const keys = (accounts.data ?? []).map((a) => a.key);
  const who = keys.includes(picked) ? picked : (keys[0] ?? "");
  const noAccounts = accounts.isSuccess && keys.length === 0;

  const phaseRef = useRef(phase);
  useEffect(() => {
    phaseRef.current = phase;
  }, [phase]);

  useEffect(() => {
    const un = events.recordingEvent.listen((e) => {
      const p = e.payload;
      if (p.kind === "closed" && phaseRef.current.kind === "recording") {
        void commands.autoRunRecordCancel().catch(() => {});
      }
      setPhase((cur) => {
        if (cur.kind !== "recording") return cur;
        if (p.kind === "click" || p.kind === "field") {
          const row: Row = { kind: p.kind, readable: p.readable, password: p.kind === "field" && p.password };
          return { ...cur, rows: [...cur.rows, row] };
        }
        if (p.kind === "marker") return { ...cur, marker: p.readable, picking: false };
        if (p.kind === "unreadable") return { ...cur, notes: [...cur.notes, p.detail] };
        if (p.kind === "closed") {
          return { kind: "failed", why: "The recording browser was closed. Nothing was saved." };
        }
        return cur;
      });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  // A recording, a Start or a check left behind by a dialog that was
  // closed with the Auto Run section still holds the recorder: offer to
  // end it, while this dialog has started nothing of its own.
  useEffect(() => {
    let live = true;
    commands
      .autoRunRecordingIsOpen()
      .then((open) => {
        if (live && open && phaseRef.current.kind === "before") setLeftOpen(true);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  const cancelLeftOpen = async () => {
    const stillHeld = await commands.autoRunRecordingIsOpen().catch(() => false);
    setLeftOpen(false);
    if (!stillHeld) return;
    await commands.autoRunRecordCancel().catch(() => {});
    toast.info("The recording was cancelled. Nothing was saved.");
  };

  /** Cancel while Start or the check is pending: only asks the backend.
   * The pending call settles once that takes effect. */
  const askToCancel = () => {
    cancelAsked.current = true;
    void commands.autoRunRecordCancel().catch(() => {});
  };

  const cancelledStart = () => {
    toast.info(CANCELLED);
    setPhase({ kind: "before" });
  };

  const record = async () => {
    setLeftOpen(false);
    cancelAsked.current = false;
    setPhase({ kind: "starting" });
    try {
      const r = await commands.autoRunRecordSignInStart(org, project, startValue.trim(), chosenBrowser());
      if (cancelAsked.current) {
        // The Cancel crossed Start's success: end the recording after all.
        if (r.status === "ok") await commands.autoRunRecordCancel().catch(() => {});
        cancelledStart();
        return;
      }
      if (r.status === "error") {
        setPhase({ kind: "failed", why: r.error });
        return;
      }
      setPhase({ kind: "recording", rows: [], notes: [], picking: false, marker: "" });
    } catch (e) {
      if (cancelAsked.current) {
        cancelledStart();
        return;
      }
      setPhase({ kind: "failed", why: message(e) });
    }
  };

  const pick = async () => {
    const r = await commands.autoRunRecordSignInPick().catch((e) => ({ status: "error" as const, error: message(e) }));
    if (r.status === "error") {
      setPhase((cur) => (cur.kind === "recording" ? { ...cur, notes: [...cur.notes, r.error] } : cur));
      return;
    }
    setPhase((cur) => (cur.kind === "recording" ? { ...cur, picking: true } : cur));
  };

  const finish = async () => {
    setPhase({ kind: "finishing" });
    try {
      const r = await commands.autoRunRecordSignInStop();
      if (r.status === "error") {
        setPhase({ kind: "failed", why: r.error });
        return;
      }
      setPhase({ kind: "review", draft: r.data, roles: defaultFieldRoles(r.data.steps), failure: "" });
    } catch (e) {
      setPhase({ kind: "failed", why: message(e) });
    }
  };

  const cancelRecording = async () => {
    await commands.autoRunRecordCancel().catch(() => {});
    toast.info(CANCELLED);
    setPhase({ kind: "before" });
  };

  const save = async () => {
    if (phase.kind !== "review") return;
    const { draft, roles } = phase;
    cancelAsked.current = false;
    setPhase({ kind: "checking", draft, roles });
    const back = (failure: string) => setPhase({ kind: "review", draft, roles, failure });
    try {
      const r = await commands.autoRunRecordSignInSave(org, project, who, chosenBrowser(), roles);
      if (cancelAsked.current && (r.status === "error" || !r.data.saved)) {
        toast.info("The check was cancelled. The recipe was not saved.");
        back("");
        return;
      }
      if (r.status === "error") {
        back(r.error);
        return;
      }
      if (!r.data.saved) {
        back(r.data.failure);
        return;
      }
      toast.success("Sign-in recipe recorded and saved.");
      await qc.invalidateQueries({ queryKey: recipeKey });
      onClose();
    } catch (e) {
      if (cancelAsked.current) {
        toast.info("The check was cancelled. The recipe was not saved.");
        back("");
        return;
      }
      back(message(e));
    }
  };

  const setRole = (i: number, role: FieldRole) =>
    setPhase((cur) =>
      cur.kind === "review"
        ? { ...cur, roles: cur.roles.map((c, j) => (j === i ? { role, text: c.text ?? "" } : c)) }
        : cur,
    );
  const setText = (i: number, text: string) =>
    setPhase((cur) =>
      cur.kind === "review" ? { ...cur, roles: cur.roles.map((c, j) => (j === i ? { ...c, text } : c)) } : cur,
    );

  /** Escape and the backdrop: while Start is pending they cancel it (there
   * is nothing else to press yet); while recording or checking they do
   * nothing - Cancel does that. */
  const closeIfIdle = () => {
    if (phase.kind === "starting") {
      askToCancel();
      return;
    }
    if (phase.kind === "recording" || phase.kind === "checking" || phase.kind === "finishing") return;
    onClose();
  };

  // Not while the environments are still loading: the box would be showing
  // the recipe's address for a moment, and Start must not record against it.
  const canStart = startValue.trim() !== "" && who !== "" && !envs.isLoading;

  /** The account the check signs in as: chosen before recording, and
   * changeable in the review, so a check that failed for the account's
   * sake can be tried again without recording again. */
  const accountPicker = (disabled: boolean) => (
    <label className="flex flex-wrap items-center gap-2 text-xs text-muted">
      Check with account
      <Select
        aria-label="Check with account"
        className="w-56"
        value={who}
        disabled={disabled || keys.length === 0}
        onChange={(e) => setPicked(e.target.value)}
      >
        {keys.length === 0 && <option value="">No accounts yet</option>}
        {(accounts.data ?? []).map((a) => (
          <option key={a.key} value={a.key}>
            {a.label ? `${a.label} (${a.key})` : a.key}
          </option>
        ))}
      </Select>
    </label>
  );

  return (
    <Modal onClose={closeIfIdle} className="flex max-h-[85vh] w-full max-w-2xl flex-col gap-3 p-5">
      <div>
        <h2 className="text-sm font-semibold text-text">Record sign-in</h2>
        <p className="mt-1 text-xs text-muted">
          Sign in once by hand and the app writes the sign-in recipe. It is saved only if it signs in again
          on its own in a fresh browser.
        </p>
      </div>

      {phase.kind === "before" && (
        <div className="space-y-3">
          {leftOpen && (
            <div className="flex items-center gap-2 rounded-md border border-border p-2 text-xs">
              <span className="min-w-0 flex-1 text-warning">
                Something from before is still being recorded or checked.
              </span>
              <Button size="sm" variant="outline" onClick={cancelLeftOpen}>
                <IconCancel aria-hidden />
                Cancel that recording
              </Button>
            </div>
          )}
          <label className="block space-y-1">
            <span className="text-xs font-medium text-muted">Start address</span>
            <Input
              aria-label="Start address"
              placeholder="https://hr.example.internal/"
              value={startValue}
              onChange={(e) => setStart(e.target.value)}
            />
          </label>
          {accountPicker(false)}
          {noAccounts && (
            <p className="text-xs text-warning">
              Add an account in Accounts first - the recording is checked by signing in with it.
            </p>
          )}
          <p className="text-xs text-muted">
            Sign in by hand in the browser that opens. Clicks and which fields you type into are recorded -
            never what you type.
          </p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={onClose}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={!canStart} onClick={record}>
              <IconRecord aria-hidden />
              Start
            </Button>
          </div>
        </div>
      )}

      {phase.kind === "starting" && (
        <div className="space-y-2">
          <p className="text-xs text-muted">Opening the browser at the start address…</p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={askToCancel}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
          </div>
        </div>
      )}

      {phase.kind === "recording" && (
        <div className="min-h-0 space-y-2 overflow-auto">
          <p className="text-xs text-muted">
            Sign in in the browser that opened. When you are in, press I'm signed in.
          </p>
          <ol aria-label="Recorded steps" className="space-y-1 text-xs text-text">
            {phase.rows.map((row, i) => (
              <li key={i}>{`${i + 1}. ${stepWords(row)}`}</li>
            ))}
          </ol>
          {phase.notes.map((n, i) => (
            <p key={i} className="text-xs text-warning">
              {n}
            </p>
          ))}
          {phase.marker && (
            <p className="text-xs text-text">
              <span className="text-muted">Signed-in check: </span>
              {phase.marker}
            </p>
          )}
          {phase.picking && (
            <p className="text-xs text-accent">
              Now click Sign out, or something every signed-in account sees - not your own name. That
              click is not carried out.
            </p>
          )}
          <div className="flex flex-wrap justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={cancelRecording}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" variant="outline" onClick={pick}>
              <IconPickOnPage aria-hidden />
              {phase.marker ? "Pick again" : "I'm signed in"}
            </Button>
            <Button size="sm" disabled={!phase.marker} onClick={finish}>
              <IconFinish aria-hidden />
              Finish
            </Button>
          </div>
        </div>
      )}

      {phase.kind === "finishing" && <p className="text-xs text-muted">Closing the recording browser…</p>}

      {(phase.kind === "review" || phase.kind === "checking") && (
        <Review
          draft={phase.draft}
          roles={phase.roles}
          checking={phase.kind === "checking"}
          account={accountPicker(phase.kind === "checking")}
          failure={phase.kind === "review" ? phase.failure : ""}
          onRole={setRole}
          onText={setText}
          onSave={save}
          onRecordAgain={() => setPhase({ kind: "before" })}
          onCancel={phase.kind === "checking" ? askToCancel : onClose}
        />
      )}

      {phase.kind === "failed" && (
        <div className="space-y-2">
          <p className="text-xs text-danger">{phase.why}</p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={onClose}>
              <IconCancel aria-hidden />
              Close
            </Button>
            <Button size="sm" onClick={() => setPhase({ kind: "before" })}>
              <IconRecord aria-hidden />
              Record again
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}

const ROLE_LABELS: { value: FieldRole; label: string }[] = [
  { value: "username", label: "Username" },
  { value: "password", label: "Password" },
  { value: "text", label: "Fixed text" },
];

/** After Finish: each recorded step, and for each field what it fills in. */
function Review({
  draft,
  roles,
  checking,
  account,
  failure,
  onRole,
  onText,
  onSave,
  onRecordAgain,
  onCancel,
}: {
  draft: SignInDraftView;
  roles: FieldChoice[];
  checking: boolean;
  /** The "Check with account" control. */
  account: ReactNode;
  failure: string;
  onRole: (i: number, role: FieldRole) => void;
  onText: (i: number, text: string) => void;
  onSave: () => void;
  onRecordAgain: () => void;
  onCancel: () => void;
}) {
  const missingText = roles.some((c) => c.role === "text" && (c.text ?? "").trim() === "");
  /** Each step's place among the field steps (the order `roles` is in);
   * -1 for a click. */
  const fieldIndex = draft.steps.reduce<number[]>((out, step) => {
    const before = out.filter((i) => i >= 0).length;
    return [...out, step.kind === "field" ? before : -1];
  }, []);
  return (
    <div className="flex min-h-0 flex-col gap-3">
      <p className="text-xs text-muted">Say what each field you typed into is. Nothing you typed was kept.</p>
      <ol aria-label="Recorded steps" className="min-h-0 space-y-2 overflow-auto text-xs text-text">
        {draft.steps.map((step, n) => {
          const i = fieldIndex[n];
          if (i < 0) return <li key={n}>{`${n + 1}. ${stepWords(step)}`}</li>;
          const choice = roles[i] ?? { role: "text", text: "" };
          return (
            <li key={n} className="space-y-1 rounded-md border border-border p-2">
              <div className="flex flex-wrap items-center gap-2">
                <span className="min-w-0 flex-1">{`${n + 1}. ${stepWords(step)}`}</span>
                <Select
                  aria-label={`Step ${n + 1} is`}
                  className="w-40"
                  value={choice.role}
                  disabled={checking}
                  onChange={(e) => onRole(i, e.target.value as FieldRole)}
                >
                  {ROLE_LABELS.map((r) => (
                    <option key={r.value} value={r.value}>
                      {r.label}
                    </option>
                  ))}
                </Select>
              </div>
              {choice.role === "text" && (
                <div className="space-y-1">
                  <Input
                    aria-label={`Fixed text for step ${n + 1}`}
                    value={choice.text ?? ""}
                    disabled={checking}
                    onChange={(e) => onText(i, e.target.value)}
                  />
                  <p className="text-faint">Saved in the recipe as written - never put a password here.</p>
                </div>
              )}
            </li>
          );
        })}
      </ol>
      <p className="text-xs text-text">
        <span className="text-muted">Signed-in check: </span>
        {draft.marker || "none picked"}
      </p>
      {account}
      {checking && (
        <p className="text-xs text-muted">
          Checking - signing in with this recipe in a fresh browser. This can take a minute…
        </p>
      )}
      {failure && <p className="text-xs text-danger">{failure}</p>}
      <div className="flex flex-wrap justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" variant="outline" disabled={checking} onClick={onRecordAgain}>
          <IconRecord aria-hidden />
          Record again
        </Button>
        <Button size="sm" disabled={checking || missingText} onClick={onSave}>
          <IconConfirm aria-hidden />
          {checking ? "Checking" : "Check and save"}
        </Button>
      </div>
    </div>
  );
}
