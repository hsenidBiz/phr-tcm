// The Assistants card: every connected assistant that has an Auto Run browser
// (one without is left out), the browser it has, what it is doing, and the three things a person can do about it - Stop its replay,
// End its discovery, Release its browser. It draws nothing until an assistant
// is doing something: while every connected assistant is idle (or none is
// connected) it is gone, and while it shows, an idle assistant keeps its row
// (dozing) so its browser can still be released. One shared office floor
// (AssistantScene.tsx) runs across the top of the card, in three corners with
// each assistant standing in the one matching its activity, and the rows sit
// below it, each with a small square in its character's colour. Rust sends
// the whole list again whenever any of it changes (`autorun-agents-changed`,
// at most once a second), so the rows are replaced outright rather than
// patched.

import { useEffect, useId, useState } from "react";
import { commands, type AgentActivity, type AgentView } from "../../bindings";
import { useAgents } from "../../hooks/useAgents";
import { activityText, isWorking } from "../../lib/agentActivity";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { IconRelease, IconStop } from "../../lib/actionIcons";
import { logUi } from "../../lib/uiLog";
import AssistantOffice, { hueFor } from "./AssistantScene";

// The old home of this helper; the card's test still imports it from here.
export { isWorking };

/** Which browser the assistant has, and who is signed in there. */
function browserText(a: AgentView): string {
  let text = a.own_browser ? "Its own browser" : "Main Auto Run browser";
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
  const { agents } = useAgents();
  const [notes, setNotes] = useState<Record<string, Note>>({});
  /** Rows with a button's answer still on its way. */
  const [busy, setBusy] = useState<ReadonlySet<string>>(new Set());
  const headingId = useId();

  // Forgets the answers and waits of any assistant not in the list: Rust
  // hands out the smallest free number, so a new session can be "Assistant 1"
  // again and must not inherit the last one's answer.
  useEffect(() => {
    const here = new Set(agents.map((a) => a.label));
    setNotes((n) => {
      const kept = Object.entries(n).filter(([l]) => here.has(l));
      return kept.length === Object.keys(n).length ? n : Object.fromEntries(kept);
    });
    setBusy((b) => {
      const kept = [...b].filter((l) => here.has(l));
      return kept.length === b.size ? b : new Set(kept);
    });
  }, [agents]);

  /** Runs one button's command and shows what came of it under the row. */
  async function press(label: string, call: () => Promise<Answer>, action: string, failed: string) {
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
    } catch (e) {
      // The raw error can carry an address: it goes to the app log, and the
      // person gets a sentence.
      logUi(`auto-run: assistants card: ${action} for ${label} failed: ${e instanceof Error ? e.message : String(e)}`);
      setNotes((n) => ({ ...n, [label]: { text: `${failed} Try again, or see Settings → Logs.`, error: true } }));
    } finally {
      setBusy((b) => {
        const next = new Set(b);
        next.delete(label);
        return next;
      });
    }
  }

  // An assistant connected without an Auto Run browser has nothing here to
  // stop or release: it gets no row and no place in the office.
  const shown = agents.filter((a) => a.slot !== null);
  const workingCount = shown.filter((a) => isWorking(a.activity)).length;
  if (workingCount === 0) return null;

  return (
    <section aria-labelledby={headingId} className="space-y-2 rounded-md border border-border bg-surface p-3">
      <div className="flex items-baseline gap-2">
        <h3 id={headingId} className="text-sm font-semibold text-text">
          Assistants
        </h3>
        <span className="text-xs text-muted">{workingCount} working</span>
      </div>
      <AssistantOffice agents={shown} />
      <ul className="divide-y divide-border">
        {shown.map((a) => {
          const slot = a.slot;
          const note = notes[a.label];
          const working = busy.has(a.label);
          return (
            <li key={a.label} className="py-2 first:pt-0 last:pb-0">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex min-w-0 items-center gap-3">
                  <div className="min-w-0 space-y-0.5 text-xs">
                    <p className="flex items-center gap-1.5 font-medium text-text">
                      <span
                        data-hue
                        aria-hidden="true"
                        className="inline-block size-2.5 shrink-0 rounded-sm"
                        style={{ background: hueFor(a.slot) }}
                      />
                      {a.client ? `${a.label} - ${a.client}` : a.label}
                    </p>
                    <p className="text-muted">{browserText(a)}</p>
                    <p className="flex flex-wrap items-center gap-1.5 text-muted">
                      <span>{activityText(a.activity, a.idle_minutes)}</span>
                      {a.changed_for_testing && (
                        <Badge className="bg-warning/15 text-warning">changed for testing</Badge>
                      )}
                      {a.saves_blocked && <Badge>saves blocked</Badge>}
                    </p>
                    {a.activity.kind === "replaying" && a.activity.of > 0 && (
                      <div data-progress aria-hidden="true" className="h-1 w-40 rounded bg-border">
                        <div
                          className="h-full rounded bg-accent"
                          style={{ width: `${Math.min(100, Math.round((a.activity.step / a.activity.of) * 100))}%` }}
                        />
                      </div>
                    )}
                  </div>
                </div>
                <div className="flex shrink-0 flex-wrap gap-2">
                  {a.can_stop && slot !== null && (
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`Stop ${a.label}`}
                      disabled={working}
                      onClick={() =>
                        void press(a.label, () => commands.autoRunStopAgent(slot), "stop", `Could not stop ${a.label}.`)
                      }
                    >
                      <IconStop aria-hidden />
                      Stop
                    </Button>
                  )}
                  {isDiscovering(a.activity) && slot !== null && (
                    <Button
                      size="sm"
                      variant="outline"
                      aria-label={`End discovery for ${a.label}`}
                      disabled={working}
                      onClick={() =>
                        void press(
                          a.label,
                          () => commands.autoRunEndAgentDiscovery(slot),
                          "end discovery",
                          `Could not end ${a.label}'s discovery.`,
                        )
                      }
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
                      onClick={() =>
                        void press(
                          a.label,
                          () => commands.autoRunReleaseAgent(slot),
                          "release",
                          `Could not release ${a.label}'s browser.`,
                        )
                      }
                    >
                      <IconRelease aria-hidden />
                      Release
                    </Button>
                  )}
                </div>
              </div>
              {/* Always present, so a screen reader that watches it hears the
                  answer arrive; only its text changes. A failure is its own
                  alert. */}
              <p role="status" aria-live="polite" className="text-xs text-muted [&:not(:empty)]:mt-1">
                {note && !note.error ? note.text : ""}
              </p>
              {note?.error && (
                <p role="alert" className="mt-1 text-xs text-danger">
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
