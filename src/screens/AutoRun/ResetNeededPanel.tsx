// The pause before a batch (design §3, run batches): a checklist of the
// settings to set, what to put back and which cases changed it, the cases
// still to run folded by area, and Continue or Stop. The unattended run and
// the supervised pane show the same panel.

import { useState } from "react";
import type { Pause } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { IconContinue, IconHideDetails, IconShowDetails, IconStop, IconWarning } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { groupByArea, parseSetLine, pauseHeading, resetLines, setLines } from "./plan";

/** Areas shown before the rest hide behind "N more areas". */
const AREAS_SHOWN = 6;
/** Cases the open first area shows before "and N more". */
const CASES_SHOWN = 3;

type Props = {
  /** The batch it opens, the settings to set, the names to revert, and the
   * cases that changed each. */
  reset: Pick<Pause, "names" | "changed_by"> & Partial<Pick<Pause, "batch" | "set">>;
  /** Every case still to run, the next one first. */
  remaining: number[];
  titleOf: (id: number) => string | undefined;
  /** An answer is on its way: both buttons wait for it. */
  busy?: boolean;
  onContinue: () => void;
  onStop: () => void;
};

/** Keyed by batch, so the ticks and folds start fresh for each one. */
export default function ResetNeededPanel(props: Props) {
  return <BatchPanel key={props.reset.batch ?? 0} {...props} />;
}

function SetRow({ line, done, onToggle }: { line: string; done: boolean; onToggle: () => void }) {
  const parsed = parseSetLine(line);
  return (
    <li className="border-l-2 border-l-warning pl-2">
      <label className="flex items-start gap-2">
        <span className="mt-0.5 shrink-0">
          <Checkbox checked={done} onCheckedChange={onToggle} ariaLabel={line} />
        </span>
        <span className="min-w-0">
          <span className={cn("block", done ? "text-muted line-through" : "text-text")}>
            {parsed ? parsed.text : line}
          </span>
          {parsed && (
            <span className="mt-1 flex flex-wrap gap-1">
              {parsed.chips.map((chip) => (
                <span key={chip} className="rounded border border-border bg-surface px-1.5 py-0.5 text-xs text-muted">
                  {chip}
                </span>
              ))}
            </span>
          )}
        </span>
      </label>
    </li>
  );
}

function flip(from: ReadonlySet<string>, key: string, to: (next: Set<string>) => void) {
  const next = new Set(from);
  if (!next.delete(key)) next.add(key);
  to(next);
}

function BatchPanel({ reset, remaining, titleOf, busy = false, onContinue, onStop }: Props) {
  const heading = pauseHeading(reset.batch);
  const set = setLines({ batch: reset.batch ?? 0, set: reset.set ?? [] });
  const back = resetLines(reset, titleOf);
  const groups = groupByArea(remaining, titleOf);
  // Ticks are a reminder only: nothing here reads them but the counter.
  const [ticked, setTicked] = useState<ReadonlySet<string>>(new Set());
  const [open, setOpen] = useState<ReadonlySet<string>>(() => new Set(groups[0] ? [groups[0].area] : []));
  const [full, setFull] = useState<ReadonlySet<string>>(new Set());
  const [allAreas, setAllAreas] = useState(false);
  const shown = allAreas ? groups : groups.slice(0, AREAS_SHOWN);
  const done = set.filter((line) => ticked.has(line)).length;
  return (
    <section aria-label={heading} className="space-y-3 rounded-md border border-warning/40 bg-surface-2 p-3 text-xs">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h3 className="flex items-center gap-1.5 text-sm font-semibold text-warning">
            <IconWarning aria-hidden />
            {heading}
          </h3>
          <p className="mt-0.5 text-muted">Set these up in the app, tick each one, then press Continue.</p>
        </div>
        <div className="flex shrink-0 justify-end gap-2">
          <Button size="sm" variant="outline" aria-label="Stop the run at this pause" disabled={busy} onClick={onStop}>
            <IconStop aria-hidden />
            Stop
          </Button>
          <Button size="sm" aria-label="Continue the run" disabled={busy} onClick={onContinue}>
            <IconContinue aria-hidden />
            Continue
          </Button>
        </div>
      </div>
      {set.length > 0 && (
        <div>
          <p className="font-medium text-muted">
            Set up · {done} of {set.length} done
          </p>
          <ul aria-label="Set up" className="mt-1 space-y-1.5">
            {set.map((line) => (
              <SetRow key={line} line={line} done={ticked.has(line)} onToggle={() => flip(ticked, line, setTicked)} />
            ))}
          </ul>
        </div>
      )}
      {back.length > 0 && (
        <div>
          <p className="font-medium text-muted">Put back</p>
          <ul aria-label="Put back" className="mt-1 space-y-1">
            {back.map((line) => (
              <li key={line} className="border-l-2 border-l-warning pl-2 text-text">
                {line}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div>
        <p className="font-medium text-muted">
          Still to run · {remaining.length} {remaining.length === 1 ? "case" : "cases"} in {groups.length}{" "}
          {groups.length === 1 ? "area" : "areas"}
        </p>
        <div className="mt-1 space-y-0.5">
          {shown.map((group, at) => {
            const isOpen = open.has(group.area);
            // Only the first area is trimmed; "and N more" opens its whole list.
            const trimmed = at === 0 && !full.has(group.area) && group.items.length > CASES_SHOWN;
            const items = trimmed ? group.items.slice(0, CASES_SHOWN) : group.items;
            return (
              <div key={group.area}>
                <button
                  type="button"
                  aria-expanded={isOpen}
                  onClick={() => flip(open, group.area, setOpen)}
                  className="flex w-full items-center gap-1.5 rounded px-1 py-0.5 text-left text-text hover:bg-surface"
                >
                  {isOpen ? <IconHideDetails aria-hidden /> : <IconShowDetails aria-hidden />}
                  <span className="min-w-0 flex-1 truncate font-medium">{group.area}</span>
                  <span className="text-muted">{group.items.length}</span>
                </button>
                {isOpen && (
                  <>
                    <ul aria-label={`Cases in ${group.area}`} className="ml-6 space-y-0.5 py-0.5">
                      {items.map((c) => (
                        <li key={c.id} className="text-text">
                          <span className="text-muted">#{c.id}</span>
                          {c.title ? ` ${c.title}` : ""}
                        </li>
                      ))}
                    </ul>
                    {trimmed && (
                      <Button size="sm" variant="ghost" className="ml-5" onClick={() => flip(full, group.area, setFull)}>
                        and {group.items.length - CASES_SHOWN} more
                      </Button>
                    )}
                  </>
                )}
              </div>
            );
          })}
          {!allAreas && groups.length > AREAS_SHOWN && (
            <Button size="sm" variant="ghost" onClick={() => setAllAreas(true)}>
              {groups.length - AREAS_SHOWN} more areas
            </Button>
          )}
        </div>
      </div>
    </section>
  );
}
