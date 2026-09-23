// The optional extras' unlock: a key sequence typed while the Settings
// screen is open (wired in screens/settingsExtras.ts; what it unlocks is
// lib/extras.ts). Pure, so every rule is a unit test and the React side
// only feeds keys in.

/** Up Up Down Down Left Right Left Right B A Enter. Letters are compared
 * lower-cased, so B and b are the same input. */
export const SEQUENCE: readonly string[] = [
  "ArrowUp", "ArrowUp", "ArrowDown", "ArrowDown",
  "ArrowLeft", "ArrowRight", "ArrowLeft", "ArrowRight",
  "b", "a", "Enter",
];

export const SEQUENCE_LENGTH = SEQUENCE.length;

/** From this many correct inputs on, each further correct one shakes the
 * Settings panel. */
export const SHAKE_FROM = 5;

/** Keys that are never an input on their own. Shift is how a capital B is
 * typed: counting it as a wrong key would reset an attempt that is going
 * right. */
const MODIFIERS = new Set(["Shift", "Control", "Alt", "AltGraph", "Meta", "CapsLock"]);

/**
 * The progress after `key`, given `progress` correct inputs so far.
 * `SEQUENCE_LENGTH` means complete; the caller starts again from 0.
 *
 * A wrong key resets to 0 - except ArrowUp, the only key that can also
 * START an attempt: it counts as input 1 of a new one, and after exactly
 * two ArrowUps a third keeps the attempt at two (the longest start of the
 * sequence that the presses so far still end with).
 */
export function next(progress: number, key: string): number {
  const at = Number.isInteger(progress) && progress >= 0 && progress < SEQUENCE_LENGTH ? progress : 0;
  if (MODIFIERS.has(key)) return at;
  const k = key.length === 1 ? key.toLowerCase() : key;
  if (SEQUENCE[at] === k) return at + 1;
  if (k === "ArrowUp") return at === 2 ? 2 : 1;
  return 0;
}

/** Where typing happens, so a key pressed there never counts and never
 * resets. Includes an open listbox or menu, and a select's trigger (arrow
 * keys on it change its value). */
const EDITABLE = [
  "input",
  "textarea",
  "select",
  "[contenteditable]:not([contenteditable='false'])",
  "[role='listbox']",
  "[role='menu']",
  "[role='combobox']",
].join(", ");

export function isEditableTarget(target: EventTarget | null): boolean {
  if (!target || typeof (target as Element).closest !== "function") return false;
  return (target as Element).closest(EDITABLE) !== null;
}
