// The review gate's git-style step rendering: a changed step is ONE
// neutral line with only the differing words marked; whole-step adds and
// removes keep their +/- tinted lines.

import { type StepDiff } from "../lib/caseDiff";
import { cn } from "../lib/cn";
import InlineDiff from "./InlineDiff";
import SharedStepLabel from "./SharedStepLabel";

/** The action ⇒ expected-result divider.
 *
 * A step line is one run of prose with a hinge in the middle, and the
 * hinge is the thing you scan for: everything before it is what the tester
 * DOES, everything after is what should HAPPEN. In muted grey at body
 * weight it disappeared into the sentence. Accent-coloured, bolder and
 * spaced out, it becomes the landmark that lets a long line be read as two
 * halves at a glance. */
function Arrow() {
  return <span className="px-1 font-bold text-accent">{"⇒"}</span>;
}

/** A changed step is ONE neutral line with only the differing words marked
 * (green = inserted, red struck = deleted). Whole-step adds/removes keep
 * their +/- tinted line, since the entire step is new or gone.
 *
 * `org` is only for a Shared Steps row's title lookup (`SharedStepLabel`);
 * a caller with no org (SyncReport, which has none to thread through) still
 * gets the reference number, just not the title. */
export default function StepDiffLines({ d, org }: { d: StepDiff; org?: string }) {
  if (d.kind === "changed") {
    // A Shared Steps reference carries no action/expected text of its own -
    // both sides are always "". Diffing that blindly rendered "#2" with
    // nothing after it, so a re-pointed reference is shown by its numbers
    // instead of a word diff.
    if (d.old!.shared != null || d.new!.shared != null) {
      return (
        <div className="flex gap-2 rounded bg-surface-2/60 px-2 py-1 text-text">
          <span className="select-none font-semibold text-faint">±</span>
          <span className="whitespace-pre-wrap">
            <span className="id-mono text-faint">#{d.index + 1}</span>{" "}
            {d.old!.shared != null && <SharedStepLabel id={d.old!.shared} org={org} />}
            {d.old!.shared != null && d.new!.shared != null && <Arrow />}
            {d.new!.shared != null && <SharedStepLabel id={d.new!.shared} org={org} />}
          </span>
        </div>
      );
    }
    return (
      <div className="flex gap-2 rounded bg-surface-2/60 px-2 py-1 text-text">
        <span className="select-none font-semibold text-faint">±</span>
        <span className="whitespace-pre-wrap">
          <span className="id-mono text-faint">#{d.index + 1}</span>{" "}
          <InlineDiff old={d.old!.action} next={d.new!.action} />
          {(d.old!.expected || d.new!.expected) && (
            <span className="text-muted">
              <Arrow />
              <InlineDiff
                old={d.old!.expected}
                next={d.new!.expected}
                emptyLabel="(no expected result)"
              />
            </span>
          )}
        </span>
      </div>
    );
  }
  const step = (d.new ?? d.old)!;
  const added = d.kind === "added";
  return (
    <div
      className={cn(
        "flex gap-2 rounded px-2 py-1",
        added ? "bg-success/10 text-success" : "bg-danger/10 text-danger",
      )}
    >
      <span className="select-none font-semibold">{added ? "+" : "-"}</span>
      <span className="whitespace-pre-wrap">
        <span className="id-mono opacity-70">#{d.index + 1}</span>{" "}
        {step.shared != null ? (
          <SharedStepLabel id={step.shared} org={org} />
        ) : (
          <>
            {step.action}
            {step.expected && (
              <span className="opacity-80">
                {/* Not the accent on an added/removed line: those are already
                    fully green or red, and a third colour inside them reads as
                    a third meaning. Weight and spacing carry it here. */}
                <span className="px-1 font-bold">{"⇒"}</span>
                {step.expected}
              </span>
            )}
          </>
        )}
      </span>
    </div>
  );
}

