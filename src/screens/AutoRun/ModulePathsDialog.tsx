// Module paths: how an unattended run gets from the home page to each
// module's screen. A path is recorded by clicking through the menu in a
// real browser, checked by replaying it in a fresh one, and saved only if
// that replay lands where the recording did. The dialog also holds the
// project's "Scripts may open pages by address" switch.
//
// Nothing here reaches Azure DevOps: paths and the switch live on this
// machine, beside the project's sign-in recipe.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { commands, events } from "../../bindings";
import { Button } from "../../components/ui/button";
import Combobox from "../../components/ui/combobox";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import { Switch } from "../../components/ui/switch";
import { IconBack, IconCancel, IconRecord, IconRemove, IconRun, IconStop } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";

type Phase =
  | { kind: "list" }
  | { kind: "choose"; module: string }
  | { kind: "starting"; module: string }
  | { kind: "recording"; module: string; clicks: string[]; notes: string[] }
  | { kind: "checking"; module: string }
  | { kind: "failed"; module: string; why: string };

/** The browser the person last picked in Auto Run (the run panes' own
 * key), so a recording opens in the browser they already chose. */
function chosenBrowser(): string {
  try {
    return localStorage.getItem("tcm-v2-autorun-browser") === "chrome" ? "chrome" : "edge";
  } catch {
    return "edge";
  }
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function ModulePathsDialog({
  org,
  project,
  caseModules,
  onClose,
}: {
  org: string;
  project: string;
  /** The Module values of the cases loaded in Auto Run, for picking. */
  caseModules: string[];
  onClose: () => void;
}) {
  const qc = useQueryClient();
  const navKey = ["autorun-nav", org, project];
  const nav = useQuery({
    queryKey: navKey,
    queryFn: () => unwrapStr(commands.autoRunLoadNav(org, project)),
    retry: false,
  });
  const accounts = useQuery({
    queryKey: ["autorun-accounts"],
    queryFn: () => unwrapStr(commands.autoRunListAccounts()),
    retry: false,
  });
  const [picked, setPicked] = useState("");
  const [phase, setPhase] = useState<Phase>({ kind: "list" });
  const [confirming, setConfirming] = useState<string | null>(null);
  const [trying, setTrying] = useState<string | null>(null);
  const [tried, setTried] = useState<Record<string, { ok: boolean; detail: string }>>({});
  const [problem, setProblem] = useState("");
  /** Something from before this dialog opened still holds the recorder. */
  const [leftOpen, setLeftOpen] = useState(false);

  /** Whether this dialog asked to cancel the Start, check or Try it is
   * waiting on. That call's answer is then read as a cancel whatever it
   * says - by what was asked, never by matching the backend's words - and
   * a Start that won the race anyway is cancelled after all. Reset as each
   * of those begins. */
  const cancelAsked = useRef(false);

  const keys = (accounts.data ?? []).map((a) => a.key);
  const who = keys.includes(picked) ? picked : (keys[0] ?? "");
  const busy =
    phase.kind === "starting" || phase.kind === "recording" || phase.kind === "checking" || trying !== null;

  // Read inside the event listener below, whose own effect only runs once -
  // a plain closure over `phase` there would see whatever phase was current
  // on mount forever, so a "closed" event arriving during "starting" (where
  // there is no recording to free) would misread it as "recording" too.
  const phaseRef = useRef(phase);
  useEffect(() => {
    phaseRef.current = phase;
  }, [phase]);

  useEffect(() => {
    const un = events.recordingEvent.listen((e) => {
      const p = e.payload;
      // The recording browser went away: free the recorder at once - but
      // only while an actual recording is running. A "closed" event has no
      // slot to free during "starting" or once the phase has moved on.
      if (p.kind === "closed" && phaseRef.current.kind === "recording") {
        void commands.autoRunRecordCancel().catch(() => {});
      }
      setPhase((cur) => {
        if (cur.kind !== "recording") return cur;
        if (p.kind === "click") return { ...cur, clicks: [...cur.clicks, p.readable] };
        if (p.kind === "unreadable") return { ...cur, notes: [...cur.notes, p.detail] };
        if (p.kind === "closed") {
          return { kind: "failed", module: cur.module, why: "The recording browser was closed. Nothing was saved." };
        }
        return cur;
      });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  // A dialog that was closed with the Auto Run section (a section switch
  // unmounts it) can leave a recording, a Start, a check or a Try behind,
  // holding the recorder. Nothing else would ever end it, so say so and
  // offer Cancel. Only while this dialog has started nothing of its own.
  useEffect(() => {
    let live = true;
    commands
      .autoRunRecordingIsOpen()
      .then((open) => {
        if (live && open && phaseRef.current.kind === "list") setLeftOpen(true);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  const cancelled = () => {
    toast.info("The recording was cancelled. Nothing was saved.");
    setPhase({ kind: "list" });
  };

  const record = async (module: string) => {
    setProblem("");
    // From here on the recorder is this dialog's own: the offer to cancel
    // a leftover would cancel this recording instead.
    setLeftOpen(false);
    cancelAsked.current = false;
    setPhase({ kind: "starting", module });
    try {
      const r = await commands.autoRunRecordStart(org, project, module, who, chosenBrowser());
      if (r.status === "error") {
        if (cancelAsked.current) {
          cancelled();
          return;
        }
        setPhase({ kind: "failed", module, why: r.error });
        return;
      }
      if (cancelAsked.current) {
        // The Cancel crossed Start's success: the recording opened after
        // all. End it now rather than show one nobody wants.
        await commands.autoRunRecordCancel().catch(() => {});
        cancelled();
        return;
      }
      setPhase({ kind: "recording", module, clicks: [], notes: [] });
    } catch (e) {
      if (cancelAsked.current) {
        cancelled();
        return;
      }
      setPhase({ kind: "failed", module, why: message(e) });
    }
  };

  const stop = async () => {
    if (phase.kind !== "recording") return;
    const { module } = phase;
    cancelAsked.current = false;
    setPhase({ kind: "checking", module });
    try {
      const r = await commands.autoRunRecordStop();
      if (r.status === "error" || !r.data.saved) {
        if (cancelAsked.current) {
          cancelled();
          return;
        }
        setPhase({ kind: "failed", module, why: r.status === "error" ? r.error : r.data.failure });
        return;
      }
      toast.success(`Path saved for ${r.data.module}.`);
      await qc.invalidateQueries({ queryKey: navKey });
      setPhase({ kind: "list" });
    } catch (e) {
      if (cancelAsked.current) {
        cancelled();
        return;
      }
      setPhase({ kind: "failed", module, why: message(e) });
    }
  };

  const cancel = async () => {
    await commands.autoRunRecordCancel().catch(() => {});
    setPhase({ kind: "list" });
  };

  /** Cancel while Start, the check after Stop, or a Try is still pending.
   * It only asks the backend to stop - the still-open call settles once
   * that takes effect, and whoever is waiting on it moves on then. Setting
   * the phase here too would race a later Start. */
  const askToCancel = () => {
    cancelAsked.current = true;
    void commands.autoRunRecordCancel().catch(() => {});
  };

  /** The offer was made when the dialog opened; the leftover may have
   * ended by itself since (a check finishing). Ask again, so a Cancel only
   * ever reaches something still left behind - the offer is shown only
   * while this dialog runs nothing of its own, so whatever holds the
   * recorder now is not this dialog's. */
  const cancelLeftOpen = async () => {
    const stillHeld = await commands.autoRunRecordingIsOpen().catch(() => false);
    setLeftOpen(false);
    if (!stillHeld) return;
    await commands.autoRunRecordCancel().catch(() => {});
    toast.info("The recording was cancelled. Nothing was saved.");
  };

  const tryPath = async (module: string) => {
    // From here on the recorder is this dialog's own: the offer to cancel
    // a leftover would cancel this Try instead.
    setLeftOpen(false);
    cancelAsked.current = false;
    setTrying(module);
    const show = (result: { ok: boolean; detail: string; cancelled?: boolean }) => {
      // A cancelled Try says nothing about the path: drop any old answer
      // rather than show the cancel as the path failing. Cancelled by this
      // dialog (asked), or by anything else (the backend says so).
      if (result.cancelled || (cancelAsked.current && !result.ok)) {
        setTried((t) => {
          const next = { ...t };
          delete next[module];
          return next;
        });
        toast.info(`Stopped trying ${module}.`);
        return;
      }
      setTried((t) => ({ ...t, [module]: { ok: result.ok, detail: result.detail } }));
    };
    try {
      const r = await commands.autoRunTryModulePath(org, project, module, who, chosenBrowser());
      show(r.status === "ok" ? r.data : { ok: false, detail: r.error });
    } catch (e) {
      show({ ok: false, detail: message(e) });
    } finally {
      setTrying(null);
    }
  };

  const remove = useMutation({
    mutationFn: (module: string) => unwrapStr(commands.autoRunRemoveModulePath(org, project, module)),
    onSuccess: (view) => {
      qc.setQueryData(navKey, view);
      setConfirming(null);
    },
    onError: (e) => setProblem(message(e)),
  });

  const setDirect = useMutation({
    mutationFn: (allowed: boolean) => unwrapStr(commands.autoRunSetDirectUrls(org, project, allowed)),
    onSuccess: (view) => qc.setQueryData(navKey, view),
    onError: (e) => setProblem(message(e)),
  });

  /** Escape and the backdrop do nothing while a recording, a check or a
   * Try is running: only Stop or Cancel ends those. While Start is still
   * pending there is no visible Stop yet and nothing else can end it, so
   * both act as Cancel instead of leaving the dialog stuck until sign-in
   * settles on its own. */
  const closeIfIdle = () => {
    if (phase.kind === "starting") {
      askToCancel();
      return;
    }
    if (busy) return;
    onClose();
  };

  const modules = nav.data?.modules ?? [];

  return (
    <Modal onClose={closeIfIdle} className="flex max-h-[85vh] w-full max-w-2xl flex-col gap-3 p-5">
      <div>
        <h2 className="text-sm font-semibold text-text">Module paths</h2>
        <p className="mt-1 text-xs text-muted">
          How an unattended run reaches each module's screen after signing in. Record one by clicking
          through the menu; it is saved only if it works again in a fresh browser.
        </p>
      </div>
      {nav.isError && <p className="text-xs text-danger">{nav.error.message}</p>}
      {problem && <p className="text-xs text-danger">{problem}</p>}
      {leftOpen && phase.kind === "list" && trying === null && (
        <div className="flex items-center gap-2 rounded-md border border-border p-2 text-xs">
          <span className="min-w-0 flex-1 text-warning">
            A module path from before is still being recorded or checked.
          </span>
          <Button size="sm" variant="outline" onClick={cancelLeftOpen}>
            <IconCancel aria-hidden />
            Cancel that recording
          </Button>
        </div>
      )}

      <label className="flex items-center gap-2 text-xs text-muted">
        <Switch
          checked={nav.data?.direct_urls ?? true}
          ariaLabel="Scripts may open pages by address"
          disabled={!nav.data || setDirect.isPending}
          onCheckedChange={(on) => setDirect.mutate(on)}
        />
        Scripts may open pages by address
      </label>
      <p className="text-xs text-faint">
        Off: a script with a navigate step cannot be saved, and every run starts on the case's module
        screen.
      </p>

      <label className="flex items-center gap-2 text-xs text-muted">
        Record and try as
        <Select
          aria-label="Record and try as"
          className="w-56"
          value={who}
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

      {phase.kind === "list" && (
        <>
          <ul className="min-h-0 flex-1 space-y-2 overflow-auto">
            {modules.length === 0 && (
              <li className="text-xs text-muted">
                No module paths yet. Runs start from the home page, as they always have.
              </li>
            )}
            {modules.map((m) => (
              <li key={m.module} className="rounded-md border border-border p-2 text-xs">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-text">{m.module}</span>
                  <span className="min-w-0 flex-1 truncate text-muted">{m.clicks.join(" › ")}</span>
                </div>
                <p className="mt-1 text-faint">ends on {m.arrived}</p>
                {tried[m.module] && (
                  <p className={cn("mt-1", tried[m.module].ok ? "text-success" : "text-danger")}>
                    {tried[m.module].detail}
                  </p>
                )}
                {confirming === m.module ? (
                  <div className="mt-2 flex items-center justify-end gap-2">
                    <span className="text-muted">Remove the path for {m.module}?</span>
                    <Button size="sm" variant="ghost" onClick={() => setConfirming(null)}>
                      <IconCancel aria-hidden />
                      Keep it
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      disabled={remove.isPending}
                      onClick={() => remove.mutate(m.module)}
                    >
                      <IconRemove aria-hidden />
                      Remove
                    </Button>
                  </div>
                ) : (
                  <div className="mt-2 flex justify-end gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Re-record ${m.module}`}
                      disabled={!who || busy}
                      onClick={() => record(m.module)}
                    >
                      <IconRecord aria-hidden />
                      Re-record
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Try ${m.module}`}
                      disabled={!who || busy}
                      onClick={() => tryPath(m.module)}
                    >
                      <IconRun aria-hidden />
                      {trying === m.module ? "Trying" : "Try"}
                    </Button>
                    {trying === m.module && (
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-label={`Cancel trying ${m.module}`}
                        onClick={askToCancel}
                      >
                        <IconCancel aria-hidden />
                        Cancel
                      </Button>
                    )}
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Remove ${m.module}`}
                      disabled={busy}
                      onClick={() => setConfirming(m.module)}
                    >
                      <IconRemove aria-hidden />
                      Remove
                    </Button>
                  </div>
                )}
              </li>
            ))}
          </ul>
          <div className="flex justify-between gap-2">
            <Button
              size="sm"
              variant="outline"
              disabled={!who || busy}
              title={!who ? "Add an account first" : undefined}
              onClick={() => setPhase({ kind: "choose", module: "" })}
            >
              <IconRecord aria-hidden />
              Record a module…
            </Button>
            <Button size="sm" variant="ghost" disabled={busy} onClick={onClose}>
              <IconCancel aria-hidden />
              Close
            </Button>
          </div>
        </>
      )}

      {phase.kind === "choose" && (
        <div className="space-y-2">
          <label className="flex items-center gap-2 text-xs text-muted">
            Module
            <Combobox
              ariaLabel="Module"
              className="w-64"
              value={phase.module}
              options={caseModules}
              allowCustom
              placeholder="Pick or type a module"
              onChange={(v) => setPhase({ kind: "choose", module: v })}
            />
          </label>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={() => setPhase({ kind: "list" })}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={!phase.module.trim()} onClick={() => record(phase.module.trim())}>
              <IconRecord aria-hidden />
              Start recording
            </Button>
          </div>
        </div>
      )}

      {phase.kind === "starting" && (
        <div className="space-y-2">
          <p className="text-xs text-muted">Opening the browser and signing in as {who}…</p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={askToCancel}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
          </div>
        </div>
      )}

      {phase.kind === "recording" && (
        <div className="space-y-2">
          <p className="text-xs text-muted">
            Recording {phase.module}. In the browser that opened, click through the menu to the module's
            screen, then press Stop.
          </p>
          <ol aria-label="Recorded clicks" className="space-y-1 text-xs text-text">
            {phase.clicks.map((c, i) => (
              <li key={i}>{`${i + 1}. ${c}`}</li>
            ))}
          </ol>
          {phase.notes.map((n, i) => (
            <p key={i} className="text-xs text-warning">
              {n}
            </p>
          ))}
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={cancel}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button size="sm" disabled={phase.clicks.length === 0} onClick={stop}>
              <IconStop aria-hidden />
              Stop
            </Button>
          </div>
        </div>
      )}

      {phase.kind === "checking" && (
        <div className="space-y-2">
          <p className="text-xs text-muted">Checking the path for {phase.module} in a fresh browser…</p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={askToCancel}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
          </div>
        </div>
      )}

      {phase.kind === "failed" && (
        <div className="space-y-2">
          <p className="text-xs text-danger">{phase.why}</p>
          <div className="flex justify-end gap-2">
            <Button size="sm" variant="ghost" onClick={() => setPhase({ kind: "list" })}>
              <IconBack aria-hidden />
              Back to the list
            </Button>
            <Button size="sm" disabled={!who} onClick={() => record(phase.module)}>
              <IconRecord aria-hidden />
              Record again
            </Button>
          </div>
        </div>
      )}
    </Modal>
  );
}
