// Settings' optional extras: the key sequence that turns them on, typed
// while the Settings screen is open. The rules are lib/extrasSequence.ts;
// this is only the wiring - one keydown listener, added when Settings
// mounts and removed when it unmounts, so no other screen ever listens.
import { useEffect, type RefObject } from "react";
import { burstConfetti } from "../lib/confetti";
import { reducedMotion } from "../lib/exitGhost";
import { extrasUnlockedSnapshot, setExtrasUnlocked } from "../lib/extras";
import { SEQUENCE_LENGTH, SHAKE_FROM, isEditableTarget, next } from "../lib/extrasSequence";
import { toast } from "../lib/toast";
import { tourRunningSnapshot } from "../tour/tourState";

/** Fallback for a save failure that is not an Error (Rust logs the
 * reason either way). `setExtrasUnlocked` throws `Error(r.error)`, so the
 * real wording comes from Rust's own sentence (commands/misc.rs) and is
 * toasted as-is - kept here only so the two never drift apart. */
export const SAVE_FAILED = "Could not save this setting. The app log in Settings has the details.";

/** The message to toast for a failed save: the thrown Error's own text
 * when there is one, else the fallback above. */
export function saveFailedMessage(e: unknown): string {
  return e instanceof Error ? e.message : SAVE_FAILED;
}

/** Replay the panel's short shake. Removing the class and forcing a style
 * read lets the same animation play again on the very next press. */
function shake(el: HTMLElement | null): void {
  if (!el || reducedMotion()) return;
  el.classList.remove("t-shake");
  void el.offsetWidth;
  el.classList.add("t-shake");
}

async function complete(): Promise<void> {
  // Entering it again while unlocked replays the confetti; nothing else.
  if (extrasUnlockedSnapshot()) {
    if (!reducedMotion()) burstConfetti();
    return;
  }
  // Saved first: a celebration for a switch that then comes back off at
  // the next launch would be a lie.
  try {
    await setExtrasUnlocked(true);
  } catch (e) {
    toast.error(saveFailedMessage(e));
    return;
  }
  if (!reducedMotion()) burstConfetti();
  toast.success("Unlocked.");
}

export function useExtrasSequence(panel: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    let progress = 0;
    const onKey = (e: KeyboardEvent) => {
      // A held key repeats and a chord is a shortcut: neither is an input,
      // and neither resets one.
      if (e.repeat || e.ctrlKey || e.metaKey || e.altKey) return;
      if (tourRunningSnapshot()) return;
      // Typing never counts and never resets.
      if (isEditableTarget(e.target)) return;
      const after = next(progress, e.key);
      if (after === progress + 1 && after >= SHAKE_FROM) shake(panel.current);
      if (after === SEQUENCE_LENGTH) {
        progress = 0;
        // The final Enter must not also press whatever button has focus.
        e.preventDefault();
        void complete();
        return;
      }
      progress = after;
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [panel]);
}
