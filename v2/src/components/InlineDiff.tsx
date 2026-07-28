// One before/after value, rendered as a single sentence with only the
// words that actually moved marked up.
//
// This exists because the alternative kept coming back: strike the whole
// old value, print the whole new one in green. Correct, and useless -
// changing one word in a title reads as "all of this went, all of this
// arrived", and the reader has to diff it themselves. Steps were fixed
// first; every other before/after in the app now shares this renderer so
// they cannot drift apart again.

import { cn } from "../lib/cn";
import { inlineWordDiff, type InlineSegment } from "../lib/wordDiff";

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

/**
 * `old` → `next` as one marked-up stream.
 *
 * A side that is empty is NOT word-diffed: with nothing to compare against
 * every word is "added", which paints the whole value green and loses the
 * one fact worth showing - that there was nothing there before. Those two
 * cases get an explicit label instead.
 */
export default function InlineDiff({
  old,
  next,
  emptyLabel = "(empty)",
  className,
}: {
  old: string;
  next: string;
  /** What to call a side with no value. */
  emptyLabel?: string;
  className?: string;
}) {
  const before = old.trim();
  const after = next.trim();

  if (!before) {
    return (
      <span className={cn("whitespace-pre-wrap", className)}>
        <span className="text-faint">{emptyLabel}</span>{" "}
        <span className="text-faint">→</span>{" "}
        <span className="rounded-sm bg-success/25 px-0.5 font-medium text-success">
          {after || emptyLabel}
        </span>
      </span>
    );
  }
  if (!after) {
    return (
      <span className={cn("whitespace-pre-wrap", className)}>
        <span className="rounded-sm bg-danger/20 px-0.5 text-danger line-through">{before}</span>{" "}
        <span className="text-faint">→</span> <span className="text-faint">{emptyLabel}</span>
      </span>
    );
  }
  return (
    <span className={cn("whitespace-pre-wrap", className)}>
      <Segments segments={inlineWordDiff(before, after)} />
    </span>
  );
}
