import { Checkbox as XiodCheckbox } from "xiod-ui/checkbox";
import { cn } from "../../lib/cn";

/**
 * The app's checkbox: XiodUI's (Base UI underneath), drawn in the app's
 * tokens through src/xiod-theme.css. A `<span role="checkbox">` with a
 * hidden native input beside it, so a wrapping `<label>` both names it and
 * toggles it, Space toggles it, and a click still reaches the row it sits
 * in. The tick draws itself in and rubs itself out (XiodUI keeps it
 * mounted), and stands still under reduced motion (the bridge).
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
  return (
    <XiodCheckbox
      checked={checked}
      // Checked wins over a stale indeterminate flag.
      indeterminate={!checked && indeterminate}
      onCheckedChange={(next) => onCheckedChange(next)}
      aria-label={ariaLabel}
      // The empty box keeps the stronger border it always had: at 16px the
      // field border XiodUI uses (`border-input`) is too faint to find.
      className={cn("border-border-strong", className)}
    />
  );
}
