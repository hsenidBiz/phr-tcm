import { useLayoutEffect, useRef, useState } from "react";
import { cn } from "../../lib/cn";

/**
 * On/off switch: a track with a knob that slides. Used where the choice
 * takes effect immediately and reads as a state rather than a selection -
 * a checkbox says "include this in what I am about to submit", a switch
 * says "this is on right now", which is what the MCP tool toggles are.
 *
 * `role="switch"` (not checkbox) so assistive tech announces "on"/"off";
 * clicking a wrapping <label> activates it like a native input.
 */
export function Switch({
  checked,
  onCheckedChange,
  ariaLabel,
  ariaDescribedBy,
  disabled = false,
  title,
  className,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  ariaLabel?: string;
  ariaDescribedBy?: string;
  disabled?: boolean;
  /** Shown on hover; with `disabled`, pair it with `disabled:pointer-events-auto`. */
  title?: string;
  className?: string;
}) {
  // The knob's bounce plays when the switch really changes after the user
  // flipped it - not once for every switch as a screen appears, and not at
  // the click itself. A switch whose value comes back from a save changes
  // only once the save answers; marking it at the click played the bounce
  // toward the OLD value first, so the knob leapt to the far end and slid
  // back, then bounced again when the new value arrived.
  const clicked = useRef(false);
  const shown = useRef(checked);
  const [moved, setMoved] = useState(false);
  useLayoutEffect(() => {
    if (shown.current === checked) return;
    shown.current = checked;
    if (clicked.current) setMoved(true);
  }, [checked]);
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      aria-describedby={ariaDescribedBy}
      disabled={disabled}
      title={title}
      className={cn(
        "t-toggle relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border transition-colors",
        moved && "is-init",
        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
        checked
          ? "border-accent bg-accent"
          : "border-border-strong bg-surface-2 hover:border-accent",
        disabled && "cursor-not-allowed opacity-50",
        className,
      )}
      onClick={() => {
        clicked.current = true;
        onCheckedChange(!checked);
      }}
    >
      <span
        aria-hidden
        className={cn(
          // Travel is the track width minus the knob and both insets, so
          // the knob lands flush at each end rather than near it. The same
          // two positions are the ends of the bounce in index.css.
          "t-toggle-thumb inline-block h-3 w-3 rounded-full",
          checked ? "translate-x-[16px] bg-on-accent" : "translate-x-[3px] bg-border-strong",
        )}
      />
    </button>
  );
}
