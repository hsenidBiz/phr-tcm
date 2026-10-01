// The project's Known quirks, as a list a person can tend: each note with
// who wrote it, when, what the runs since have shown, and the cases it
// came from - and the buttons to edit, retire (or restore) and delete it.
// Every change is saved the moment it is made; the list shown is always
// the list the app answered with.

import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { commands, type Quirk_Serialize } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { IconAdd, IconCancel, IconConfirm, IconEdit, IconRemove, IconRestore, IconRetire } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

type Quirk = Quirk_Serialize;
type Result = ReturnType<typeof commands.autoRunLoadQuirks>;

/** An epoch-milliseconds string as a UTC date, the way the assistants' guide shows it. */
function day(ms: string | null | undefined): string {
  const n = Number(ms);
  if (!ms || !Number.isFinite(n) || n <= 0) return "";
  return new Date(n).toISOString().slice(0, 10);
}

function author(q: Quirk): string {
  if (q.by !== "assistant") return "Person";
  return q.from === "api" ? "Assistant (API templates)" : "Assistant";
}

/** What the runs since have shown - only for a note tied to a repair's steps. */
function evidence(q: Quirk): string[] {
  if (q.sources.length === 0) return [];
  const out: string[] = [];
  if (q.confirmed > 0) out.push(`Confirmed ${q.confirmed}x, last ${day(q.last_confirmed)}`);
  if (q.doubted > 0) out.push(`Did not help ${q.doubted}x`);
  if (out.length === 0) out.push("Not yet tested by a run");
  return out;
}

function sourcesText(q: Quirk): string {
  return q.sources
    .map((s) => `${s.case_id} (${s.steps.length === 1 ? "step" : "steps"} ${s.steps.join(", ")})`)
    .join("; ");
}

type Mode = { id: string; kind: "edit" | "retire" | "delete"; text: string } | null;

export default function QuirksList({
  org,
  project,
  quirks,
  onPendingChange,
}: {
  org: string;
  project: string;
  quirks: Quirk[];
  /** Whether something is typed and not saved yet - the dialog keeps itself open for it. */
  onPendingChange: (pending: boolean) => void;
}) {
  const qc = useQueryClient();
  const [draft, setDraft] = useState("");
  const [mode, setMode] = useState<Mode>(null);
  const [problem, setProblem] = useState("");
  const [busy, setBusy] = useState(false);
  const [showRetired, setShowRetired] = useState(false);

  const active = quirks.filter((q) => q.status !== "retired");
  const retired = quirks.filter((q) => q.status === "retired");

  const pending = draft.trim() !== "" || (mode !== null && mode.kind !== "delete" && mode.text.trim() !== "");
  useEffect(() => onPendingChange(pending), [pending, onPendingChange]);

  /** One change: sent, and the list it answers with shown. A refusal is shown in the app's own words. */
  const change = async (call: () => Result, after: () => void) => {
    setProblem("");
    setBusy(true);
    try {
      const list = await unwrapStr(call());
      qc.setQueryData(["autorun-quirks", org, project], list);
      after();
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const add = () => {
    if (busy || draft.trim() === "") return;
    void change(() => commands.autoRunAddQuirk(org, project, draft), () => setDraft(""));
  };

  const confirm = () => {
    if (!mode) return;
    const done = () => setMode(null);
    if (mode.kind === "edit") void change(() => commands.autoRunEditQuirk(org, project, mode.id, mode.text), done);
    if (mode.kind === "retire") {
      const reason = mode.text.trim() === "" ? null : mode.text;
      void change(() => commands.autoRunRetireQuirk(org, project, mode.id, reason), done);
    }
    if (mode.kind === "delete") void change(() => commands.autoRunDeleteQuirk(org, project, mode.id), done);
  };

  const row = (q: Quirk) => {
    const open = mode?.id === q.id ? mode : null;
    const isRetired = q.status === "retired";
    // While one row is open (an edit, a reason, a delete to confirm) or a
    // change is on its way, the other rows' buttons wait: opening another
    // row would throw away what was typed in this one.
    const locked = busy || (mode !== null && mode.id !== q.id);
    const facts = [author(q), day(q.at)].filter(Boolean);
    return (
      <li key={q.id} aria-label={q.text} className="space-y-1.5 rounded-md border border-border bg-surface px-3 py-2">
        {open?.kind === "edit" ? (
          <Input
            aria-label="Note text"
            className="w-full text-xs"
            value={open.text}
            onChange={(e) => setMode({ ...open, text: e.target.value })}
          />
        ) : (
          <p className="text-xs text-text">{q.text}</p>
        )}
        <p className="text-[11px] text-muted">
          {facts.join(" - ")}
          {evidence(q).map((e) => (
            <span key={e}> - {e}</span>
          ))}
        </p>
        {q.sources.length > 0 && <p className="text-[11px] text-muted">From cases: {sourcesText(q)}</p>}
        {isRetired && (
          <p className="text-[11px] text-muted">
            Retired {day(q.retired_at)}
            {q.retired_reason ? `: ${q.retired_reason}` : ""}
          </p>
        )}

        {open?.kind === "retire" && (
          <Input
            aria-label="Why retire it (optional)"
            className="w-full text-xs"
            placeholder="Why it no longer helps (optional)"
            value={open.text}
            onChange={(e) => setMode({ ...open, text: e.target.value })}
          />
        )}
        {open?.kind === "delete" && (
          <p className="text-xs text-warning">Delete this note for good? Retiring keeps it to bring back.</p>
        )}

        <div className="flex flex-wrap justify-end gap-2">
          {open ? (
            <>
              <Button
                size="sm"
                variant={open.kind === "delete" ? "danger" : "default"}
                disabled={busy || (open.kind === "edit" && open.text.trim() === "")}
                onClick={confirm}
              >
                {open.kind === "delete" ? <IconRemove aria-hidden /> : <IconConfirm aria-hidden />}
                {open.kind === "edit" ? "Save note" : open.kind === "retire" ? "Retire note" : "Delete note"}
              </Button>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => setMode(null)}>
                <IconCancel aria-hidden />
                Cancel
              </Button>
            </>
          ) : (
            <>
              {!isRetired && (
                <Button size="sm" variant="ghost" disabled={locked} onClick={() => setMode({ id: q.id, kind: "edit", text: q.text })}>
                  <IconEdit aria-hidden />
                  Edit
                </Button>
              )}
              {isRetired ? (
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={locked}
                  onClick={() => void change(() => commands.autoRunRestoreQuirk(org, project, q.id), () => {})}
                >
                  <IconRestore aria-hidden />
                  Restore
                </Button>
              ) : (
                <Button size="sm" variant="ghost" disabled={locked} onClick={() => setMode({ id: q.id, kind: "retire", text: "" })}>
                  <IconRetire aria-hidden />
                  Retire
                </Button>
              )}
              <Button size="sm" variant="ghost" disabled={locked} onClick={() => setMode({ id: q.id, kind: "delete", text: "" })}>
                <IconRemove aria-hidden />
                Delete
              </Button>
            </>
          )}
        </div>
      </li>
    );
  };

  return (
    <div className="space-y-2">
      {active.length === 0 ? (
        <p className="text-xs text-muted">No notes yet.</p>
      ) : (
        <ul aria-label="Active quirks" className="space-y-2">
          {active.map(row)}
        </ul>
      )}

      <div className="flex gap-2">
        <Input
          aria-label="Add a note"
          className="flex-1 text-xs"
          placeholder="Something about this application the next script should know"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") add();
          }}
        />
        <Button size="sm" disabled={busy || draft.trim() === ""} onClick={add}>
          <IconAdd aria-hidden />
          Add note
        </Button>
      </div>
      {problem && (
        <p role="alert" className="text-xs text-danger">
          {problem}
        </p>
      )}

      {retired.length > 0 && (
        <div className="space-y-2">
          <Button size="sm" variant="ghost" aria-expanded={showRetired} onClick={() => setShowRetired((v) => !v)}>
            Retired ({retired.length})
          </Button>
          {showRetired && (
            <ul aria-label="Retired quirks" className="space-y-2">
              {retired.map(row)}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
