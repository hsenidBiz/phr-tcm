// Recent activity: what the assistants saved, proposed, tried and changed in
// this organisation and environment over the last week, newest first, grouped
// by day. Nothing is drawn while nothing was recorded. Hide folds it down to
// its heading, remembered for this person across restarts.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useState } from "react";
import { commands, events, type AssistantRecord } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconHideDetails, IconShowDetails } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { requestAutoRunFocus } from "../../lib/autoRunFocus";
import { useEnvironments } from "../../lib/environments";
import { unwrapStr } from "../../lib/ipc";

/** How many days back the timeline reaches. */
export const TIMELINE_DAYS = 7;

/** Where the folded state lives: a per-person convenience, so a lost or
 * blocked store just shows the list again. */
export const HIDDEN_KEY = "tcm-v2-recent-activity-hidden";

function readHidden(): boolean {
  try {
    return localStorage.getItem(HIDDEN_KEY) === "1";
  } catch {
    return false;
  }
}

function writeHidden(hidden: boolean): void {
  try {
    if (hidden) localStorage.setItem(HIDDEN_KEY, "1");
    else localStorage.removeItem(HIDDEN_KEY);
  } catch {
    // session-only
  }
}

/** The moment a record's `at` (UTC, "YYYY-MM-DD HH:MM:SS") names. */
export function recordDate(at: string): Date {
  return new Date(`${at.replace(" ", "T")}Z`);
}

function dayKey(d: Date): string {
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

function dayHeading(d: Date): string {
  const now = new Date();
  if (dayKey(d) === dayKey(now)) return "Today";
  const yesterday = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 1);
  if (dayKey(d) === dayKey(yesterday)) return "Yesterday";
  return d.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
}

export default function RecentActivity({ org }: { org: string }) {
  const qc = useQueryClient();
  const envId = useEnvironments().data?.active ?? "";
  const headingId = useId();
  const bodyId = useId();
  const [only, setOnly] = useState<string | null>(null);
  const [hidden, setHidden] = useState(readHidden);
  const toggle = () => {
    const next = !hidden;
    setHidden(next);
    writeHidden(next);
  };

  const query = useQuery({
    queryKey: ["autorun-timeline", org, envId],
    queryFn: () => unwrapStr(commands.autoRunAssistantTimeline(org, TIMELINE_DAYS)),
    enabled: Boolean(org),
    retry: false,
  });

  useEffect(() => {
    const un = events.assistantActivityChanged.listen(() => {
      void qc.invalidateQueries({ queryKey: ["autorun-timeline"] });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [qc]);

  const records: AssistantRecord[] = query.data ?? [];
  if (records.length === 0) return null;

  const labels = [...new Set(records.map((r) => r.label))];
  const chosen = only !== null && labels.includes(only) ? only : null;
  const visible = chosen === null ? records : records.filter((r) => r.label === chosen);

  const days: { heading: string; rows: AssistantRecord[] }[] = [];
  let lastKey = "";
  for (const r of visible) {
    const d = recordDate(r.at);
    const k = dayKey(d);
    if (k !== lastKey) {
      days.push({ heading: dayHeading(d), rows: [] });
      lastKey = k;
    }
    days[days.length - 1].rows.push(r);
  }

  return (
    <section aria-labelledby={headingId} className="space-y-2 rounded-md border border-border bg-surface p-3">
      <div className="flex items-center justify-between gap-2">
        <h3 id={headingId} className="text-sm font-semibold text-text">
          Recent activity
        </h3>
        <Button
          size="sm"
          variant="ghost"
          aria-expanded={!hidden}
          aria-controls={bodyId}
          aria-label={hidden ? "Show recent activity" : "Hide recent activity"}
          onClick={toggle}
        >
          {hidden ? <IconShowDetails aria-hidden /> : <IconHideDetails aria-hidden />}
          {hidden ? "Show" : "Hide"}
        </Button>
      </div>
      {!hidden && (
        <div id={bodyId} className="space-y-2">
          {labels.length > 1 && (
            <div className="flex flex-wrap gap-1.5">
              {[null, ...labels].map((l) => (
                <button
                  key={l ?? "all"}
                  type="button"
                  aria-pressed={chosen === l}
                  onClick={() => setOnly(l)}
                  className={cn(
                    "rounded-full border border-border px-2 py-0.5 text-xs",
                    chosen === l ? "bg-accent-soft text-text" : "text-muted",
                  )}
                >
                  {l ?? "All"}
                </button>
              ))}
            </div>
          )}
          {days.map((day, i) => (
            <div key={`${day.heading}-${i}`} className="space-y-1">
              <h4 className="text-xs font-medium text-muted">{day.heading}</h4>
              <ul className="space-y-0.5">
                {day.rows.map((r, j) => (
                  <li
                    key={`${r.at}-${j}`}
                    data-ok={String(r.ok)}
                    className={cn("flex flex-wrap items-center gap-x-2 text-xs", r.ok ? "text-muted" : "text-faint")}
                  >
                    <span className="tabular-nums">
                      {recordDate(r.at).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}
                    </span>
                    <span>{r.label}</span>
                    <span>{r.action}</span>
                    {r.case_id != null && (
                      <button
                        type="button"
                        className="text-accent underline-offset-2 hover:underline"
                        onClick={() => requestAutoRunFocus({ kind: "case", caseId: r.case_id as number })}
                      >
                        {`Case ${r.case_id}`}
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
