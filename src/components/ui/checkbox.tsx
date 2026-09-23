import { useId } from "react";
import { Checkbox as XiodCheckbox } from "xiod-ui/checkbox";
import { cn } from "../../lib/cn";

/**
 * The app's checkbox: XiodUI's (Base UI underneath), drawn in the app's
 * tokens through src/xiod-theme.css. A `<span role="checkbox">` with a
 * hidden native input beside it, so a wrapping `<label>` both names it and
 * toggles it, Space toggles it, and a click still reaches the row it sits
 * in. The tick draws itself in and rubs itself out (XiodUI keeps it
 * mounted), and stands still under reduced motion (the bridge).
 *
 * Base UI's own fallback names the box from a wrapping `<label>` (via that
 * label's own id, read off the hidden input's `.labels`) whenever nothing
 * else already names it - and it does that regardless of `ariaLabel`,
 * because `aria-labelledby` always outranks `aria-label`. Left alone, a
 * call site that wraps the box in a `<label>` with different words than
 * its `ariaLabel` gets the label's text instead, or (since the box is
 * itself inside the node the label's name is computed from) both,
 * concatenated. So when `ariaLabel` is given, this wrapper points
 * `aria-labelledby` at a name of its own - a hidden span carrying that
 * text as ITS OWN `aria-label`, not as content - which pre-empts Base
 * UI's fallback outright, and (since the text sits in an attribute, not a
 * text node) can never turn up in an unrelated `getByText` search the way
 * a visually-hidden but readable text node could.
 */
export function Checkbox({
  checked,
  indeterminate = false,
  onCheckedChange,
  ariaLabel,
  className,
}: {
  checked: boolean;
  /** "Some but not all" - a dash and aria-checked="mixed". Only
   * meaningful while unchecked; clicking from mixed checks the rest
   * (onCheckedChange still receives true). */
  indeterminate?: boolean;
  onCheckedChange: (checked: boolean) => void;
  ariaLabel?: string;
  className?: string;
}) {
  const nameId = useId();
  return (
    <>
      <XiodCheckbox
        checked={checked}
        // Checked wins over a stale indeterminate flag.
        indeterminate={!checked && indeterminate}
        onCheckedChange={(next) => onCheckedChange(next)}
        aria-labelledby={ariaLabel ? nameId : undefined}
        // The empty box keeps the stronger border it always had: at 16px the
        // field border XiodUI uses (`border-input`) is too faint to find.
        className={cn("border-border-strong", className)}
      />
      {/* `hidden` keeps it off screen and out of the tab order; it is still
          read because `aria-labelledby` points at it directly (the
          accessible-name computation is explicitly required to look past
          `hidden`/display:none for a node it is told to read this way).
          Carrying the text via `aria-label` rather than as a child text
          node means the DOM never holds a stray, matchable copy of it -
          the same title a `getByText` elsewhere is legitimately looking
          for (e.g. QueueRow's `ariaLabel={`Select ${tc.title}`}`) cannot
          leak into that search through here. */}
      {ariaLabel && <span id={nameId} aria-label={ariaLabel} hidden />}
    </>
  );
}
