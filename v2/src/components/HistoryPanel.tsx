// A work item's history, as a vertical timeline with a state summary on
// top.
//
// Azure DevOps puts a horizontal "State Graph" strip above its history -
// every hop, forever, scrolled sideways. An item that bounced between New
// and In Progress five times fills the strip and still doesn't answer the
// question people open history to ask: where has this been sitting, and
// how many times has it come back? The summary here answers that in one
// line per state; the timeline below carries the detail, expanded inline
// rather than in a separate pane you have to click into.

import { useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronRight, Link2, Link2Off, MessageSquare } from "lucide-react";
import { useMemo, useState } from "react";
import { commands, type WorkRevision } from "../bindings";
import { cn } from "../lib/cn";
import InlineDiff from "./InlineDiff";
import {
  displayValue,
  groupByDay,
  humanDuration,
  matchesFilter,
  relativeTime,
  stateJourney,
  summarize,
  type HistoryFilter,
} from "../lib/history";
import { unwrap } from "../lib/ipc";
import { Skeleton } from "./ui/skeleton";

const FILTERS: { id: HistoryFilter; label: string }[] = [
  { id: "all", label: "Everything" },
  { id: "state", label: "State" },
  { id: "fields", label: "Field edits" },
  { id: "links", label: "Links" },
];

/** Initials disc, matching the comments list's fallback. */
function Who({ name, avatar }: { name: string; avatar: string }) {
  const initials = name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase() ?? "")
    .join("");
  if (avatar) {
    return <img src={avatar} alt="" className="h-6 w-6 shrink-0 rounded-full object-cover" />;
  }
  return (
    <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-surface-2 text-[10px] font-semibold text-muted">
      {initials || "?"}
    </span>
  );
}

/** old → new. A side that is missing entirely reads as "set"/"cleared"
 * rather than an empty box; when both sides have a value the change is
 * marked WORD by word, so retitling "Login as admin" to "Log in as admin"
 * shows two words moving instead of the whole title being replaced. */
function Diff({ old: raw, next: rawNext }: { old: string; next: string }) {
  const before = displayValue(raw);
  const next = displayValue(rawNext);
  if (!before) {
    return <span className="rounded bg-success/15 px-1.5 py-0.5 text-success">{next}</span>;
  }
  if (!next) {
    return (
      <span className="rounded bg-danger/15 px-1.5 py-0.5 text-danger line-through">{before}</span>
    );
  }
  return <InlineDiff old={before} next={next} />;
}

function Entry({ r }: { r: WorkRevision }) {
  const [open, setOpen] = useState(false);
  const detailed = r.fields.length > 0;
  return (
    <li className="relative pl-6">
      {/* Rail: a dot on a line, so the eye follows one thread down. */}
      <span className="absolute left-0 top-0 h-full w-px bg-border" aria-hidden />
      <span
        className={cn(
          "absolute -left-[3px] top-2.5 h-[7px] w-[7px] rounded-full",
          r.state_to ? "bg-accent" : "bg-border-strong",
        )}
        aria-hidden
      />
      <div className="py-1.5">
        <div className="flex items-start gap-2">
          <Who name={r.by} avatar={r.avatar_url} />
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-baseline gap-x-2">
              <span className="text-sm font-medium text-text">{r.by}</span>
              <span className="text-xs text-faint" title={r.at || undefined}>
                {relativeTime(r.at)}
              </span>
            </div>
            <div className="flex flex-wrap items-center gap-x-2 text-xs text-muted">
              <span>{summarize(r)}</span>
              {r.comment_added && <MessageSquare size={11} className="text-faint" />}
              {r.links_added.length > 0 && <Link2 size={11} className="text-success" />}
              {r.links_removed.length > 0 && <Link2Off size={11} className="text-danger" />}
              {detailed && (
                <button
                  aria-expanded={open}
                  aria-label={open ? `Hide details of ${r.by}'s change` : `Show details of ${r.by}'s change`}
                  className="flex items-center gap-0.5 text-faint hover:text-accent"
                  onClick={() => setOpen((o) => !o)}
                >
                  {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
                  {open ? "less" : "details"}
                </button>
              )}
            </div>

            {open && (
              <dl className="mt-1.5 space-y-1 rounded-md border border-border bg-surface-2 p-2 text-xs">
                {r.fields.map((f) => (
                  <div key={f.reference_name} className="flex flex-wrap items-baseline gap-x-2">
                    <dt className="min-w-24 shrink-0 text-faint">{f.label}</dt>
                    <dd className="min-w-0 break-words">
                      <Diff old={f.old} next={f.new} />
                    </dd>
                  </div>
                ))}
              </dl>
            )}
          </div>
        </div>
      </div>
    </li>
  );
}

export default function HistoryPanel({
  org,
  project,
  itemId,
}: {
  org: string;
  project: string;
  itemId: number;
}) {
  const [filter, setFilter] = useState<HistoryFilter>("all");
  const history = useQuery({
    queryKey: ["wi-history", org, project, itemId],
    queryFn: () => unwrap(commands.workItemHistory(org, project, itemId)),
    retry: false,
  });

  const all = useMemo(() => history.data ?? [], [history.data]);
  const journey = useMemo(() => stateJourney(all), [all]);
  const shown = useMemo(() => all.filter((r) => matchesFilter(r, filter)), [all, filter]);
  const days = useMemo(() => groupByDay(shown), [shown]);

  if (history.isPending) {
    return (
      <div className="space-y-2 p-1">
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-16 w-full" />
        <Skeleton className="h-16 w-full" />
      </div>
    );
  }
  if (history.isError) {
    return <p className="p-1 text-sm text-danger">Could not load history: {history.error.message}</p>;
  }
  if (all.length === 0) {
    return <p className="p-1 text-sm text-muted">No changes recorded yet.</p>;
  }

  return (
    <div className="space-y-3">
      {journey.length > 0 && (
        <section className="rounded-md border border-border bg-surface p-2">
          <h4 className="mb-1.5 text-[10px] font-semibold uppercase tracking-wide text-faint">
            Time in each state
          </h4>
          <ul className="space-y-1">
            {journey.map((s) => (
              <li key={s.state} className="flex items-baseline gap-2 text-xs">
                <span
                  className={cn(
                    "rounded px-1.5 py-0.5 font-medium",
                    s.current ? "bg-accent-soft text-accent" : "bg-surface-2 text-muted",
                  )}
                >
                  {s.state}
                </span>
                {s.current && <span className="text-[10px] text-accent">now</span>}
                <span className="ml-auto flex items-baseline gap-2 text-faint">
                  {/* The number ADO's strip makes you count by hand. */}
                  {s.visits > 1 && <span>entered {s.visits}×</span>}
                  {humanDuration(s.totalMs) && (
                    <span className="id-mono text-muted">{humanDuration(s.totalMs)}</span>
                  )}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <div className="flex flex-wrap gap-1">
        {FILTERS.map((f) => (
          <button
            key={f.id}
            aria-pressed={filter === f.id}
            className={cn(
              "rounded-full px-2 py-0.5 text-[11px] transition-colors",
              filter === f.id
                ? "bg-accent-soft text-accent"
                : "text-faint hover:bg-surface-2 hover:text-text",
            )}
            onClick={() => setFilter(f.id)}
          >
            {f.label}
          </button>
        ))}
      </div>

      {shown.length === 0 ? (
        <p className="text-sm text-muted">Nothing matches that filter.</p>
      ) : (
        <div className="space-y-3">
          {days.map((day) => (
            <div key={day.label}>
              <h4 className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-faint">
                {day.label}
              </h4>
              <ul>
                {day.items.map((r) => (
                  <Entry key={r.rev} r={r} />
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
