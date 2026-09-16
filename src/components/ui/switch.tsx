import { useState } from "react";
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
  disabled = false,
  className,
}: {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  ariaLabel?: string;
  disabled?: boolean;
  className?: string;
}) {
  // Set on the first click, so the knob's bounce plays when the user flips
  // the switch and not once for every switch as a screen appears.
  const [used, setUsed] = useState(false);
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      className={cn(
        "t-toggle relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border transition-colors",
        used && "is-init",
        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent",
        checked
          ? "border-accent bg-accent"
          : "border-border-strong bg-surface-2 hover:border-accent",
        disabled && "cursor-not-allowed opacity-50",
        className,
      )}
      onClick={() => {
        setUsed(true);
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
