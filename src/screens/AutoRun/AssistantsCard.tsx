// The Assistants card: every connected assistant, the browser it has, what it
// is doing, and the three things a person can do about it - Stop its replay,
// End its discovery, Release its browser. It draws nothing while no assistant
// is connected. Rust sends the whole list again whenever any of it changes
// (`autorun-agents-changed`, at most once a second), so the rows are replaced
// outright rather than patched.

import { useEffect, useId, useRef, useState } from "react";
import { commands, events, type AgentActivity, type AgentView } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { IconRelease, IconStop } from "../../lib/actionIcons";

/** What a row says its assistant is doing. */
function activityText(a: AgentActivity, idleMinutes: number): string {
  switch (a.kind) {
    case "idle":
      return idleMinutes > 0 ? `idle for ${idleMinutes} min` : "idle";
    case "discovering":
      return a.area ? `discovering ${a.area}` : "discovering";
    case "mapping":
      return `mapping menus (${a.modules.join(", ")})`;
    case "exploring":
      return "exploring";
    case "replaying":
      // Before the first step the count is 0 of 0, which is not worth saying.
      return a.step === 0 && a.of === 0
        ? `replaying case ${a.case_id} (starting)`
        : `replaying case ${a.case_id} (step ${a.step} of ${a.of})`;
    case "working":
      return "working";
  }
}

/** Which browser the assistant has, and who is signed in there. */
function browserText(a: AgentView): string {
  if (a.slot === null) return "no Auto Run browser";
  let text = a.own_browser ? "its own browser" : "main Auto Run browser";
  if (a.browser) text += `, ${a.browser}`;
  if (a.account) text += `, signed in as ${a.account}`;
  return text;
}

/** Whether the assistant is exploring the app rather than running cases. */
function isDiscovering(a: AgentActivity): boolean {
  return a.kind === "discovering" || a.kind === "mapping" || a.kind === "exploring";
}

/** What a row last heard back from a button: a Release's answer, or why a
 * button did not go through. */
type Note = { text: string; error: boolean };

/** What a command answers: Ok with an optional sentence, or why it failed. */
type Answer = { status: "ok"; data: string | null } | { status: "error"; error: string };

export default function AssistantsCard() {
  const [agents, setAgents] = useState<AgentView[]>([]);
  const [notes, setNotes] = useState<Record<string, Note>>({});
  /** Rows with a button's answer still on its way. */
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  /** An event has already supplied the list, so the first load is stale. */
  const heard = useRef(false);
  const headingId = useId();

  useEffect(() => {
    let live = true;
    commands
      .autoRunAgents()
      .then((list) => {
        if (live && !heard.current && Array.isArray(list)) setAgents(list);
      })
      .catch(() => {});
    const un = events.autorunAgentsChanged.listen((e) => {
      heard.current = true;
      if (live) setAgents(e.payload.agents);
    });
    return () => {
      live = false;
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  /** Runs one button's command and shows what came of it under the row. */
  async function press(label: string, call: () => Promise<Answer>) {
    setBusy((b) => new Set(b).add(label));
    setNotes((n) => {
      const rest = { ...n };
      delete rest[label];
      return rest;
    });
    try {
      const r = await call();
      const note: Note | null =
        r.status === "error" ? { text: r.error, error: true } : r.data ? { text: r.data, error: false } : null;
      if (note) setNotes((n) => ({ ...n, [label]: note }));
    } catch {
      setNotes((n) => ({ ...n, [label]: { text: "That did not go through. Try again.", error: true } }));
    } finally {
      setBusy((b) => {
        const next = new Set(b);
        next.delete(label);
        return next;
      });
    }
  }

  if (agents.length === 0) return null;

  return (
    <section aria-labelledby={headingId} className="space-y-2 rounded-md border border-border bg-surface p-3">
      <h3 id={headingId} className="text-sm font-semibold text-text">
        Assistants
      </h3>
      <ul className="divide-y divide-border">
        {agents.map((a) => {
          const slot = a.slot;
          const note = notes[a.label];
          const working = busy.has(a.label);
          return (
            <li key={a.label} className="space-y-1 py-2 first:pt-0 last:pb-0">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="min-w-0 space-y-0.5 text-xs">
                  <p className="font-medium text-text">{a.client ? `${a.label} - ${a.client}` : a.label}</p>
                  <p className="text-muted">{browserText(a)}</p>
                  <p className="flex flex-wrap items-center gap-1.5 text-muted">
                    <span>{activityText(a.activity, a.idle_minutes)}</span>
                    {a.changed_for_testing && <Badge className="bg-warning/15 text-warning">changed for testing</Badge>}
                    {a.saves_blocked && <Badge>saves blocked</Badge>}
                  </p>
                </div>
                <div className="flex shrink-0 flex-wrap gap-2">
                  {a.can_stop && slot !== null && (
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Stop ${a.label}`}
                      disabled={working}
                      onClick={() => void press(a.label, () => commands.autoRunStopAgent(slot))}
                    >
                      <IconStop aria-hidden />
                      Stop
                    </Button>
                  )}
                  {isDiscovering(a.activity) && slot !== null && (
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`End ${a.label}'s discovery`}
                      disabled={working}
                      onClick={() => void press(a.label, () => commands.autoRunEndAgentDiscovery(slot))}
                    >
                      <IconStop aria-hidden />
                      End discovery
                    </Button>
                  )}
                  {slot !== null && (
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Release ${a.label}'s browser`}
                      disabled={working}
                      onClick={() => void press(a.label, () => commands.autoRunReleaseAgent(slot))}
                    >
                      <IconRelease aria-hidden />
                      Release
                    </Button>
                  )}
                </div>
              </div>
              {note && (
                <p
                  role={note.error ? "alert" : "status"}
                  className={note.error ? "text-xs text-danger" : "text-xs text-muted"}
                >
                  {note.text}
                </p>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
