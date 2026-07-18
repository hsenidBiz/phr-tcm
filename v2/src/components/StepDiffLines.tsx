// The review gate's git-style step rendering: a changed step is ONE
// neutral line with only the differing words marked; whole-step adds and
// removes keep their +/- tinted lines.

import { type StepDiff } from "../lib/caseDiff";
import { cn } from "../lib/cn";
import { inlineWordDiff, type InlineSegment } from "../lib/wordDiff";

/** Inline word-diff segments: unchanged text plain, insertions green,
 * deletions red-struck - exactly where they sit in the sentence. */
function Segments({ segments }: { segments: InlineSegment[] }) {
  return (
    <>
      {segments.map((s, i) => (
        <span
          key={i}
          className={
            s.kind === "added"
              ? "rounded-sm bg-success/25 px-0.5 font-medium text-success"
              : s.kind === "removed"
                ? "rounded-sm bg-danger/20 px-0.5 text-danger line-through"
                : undefined
          }
        >
          {i > 0 ? " " : ""}
          {s.text}
        </span>
      ))}
    </>
  );
}

/** A changed step is ONE neutral line with only the differing words marked
 * (green = inserted, red struck = deleted). Whole-step adds/removes keep
 * their +/- tinted line, since the entire step is new or gone. */
export default function StepDiffLines({ d }: { d: StepDiff }) {
  if (d.kind === "changed") {
    return (
      <div className="flex gap-2 rounded bg-surface-2/60 px-2 py-1 text-text">
        <span className="select-none font-semibold text-faint">±</span>
        <span className="whitespace-pre-wrap">
          <span className="id-mono text-faint">#{d.index + 1}</span>{" "}
          <Segments segments={inlineWordDiff(d.old!.action, d.new!.action)} />
          {(d.old!.expected || d.new!.expected) && (
            <span className="text-muted">
              {" ⇒ "}
              <Segments segments={inlineWordDiff(d.old!.expected, d.new!.expected)} />
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
        <span className="id-mono opacity-70">#{d.index + 1}</span> {step.action}
        {step.expected && <span className="opacity-80"> ⇒ {step.expected}</span>}
      </span>
    </div>
  );
}

