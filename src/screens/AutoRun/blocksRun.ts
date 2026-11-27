// Which Setup rows stand between the person and a run, for Basic mode: it
// hides the rows only an advanced user tends, and a hidden row that stops a
// run is shown again with a note - except Fixtures, which Simple never shows,
// failed or not (the assistant sorts a failed fixture out on its own).
// `fixtureBlock` is the one test of whether a failed fixture stops a run;
// `blockingRows` and the panel header's fixture reasons both use it, and
// `NEVER_SHOWN_IN_SIMPLE` says Simple leaves the row and those reasons out.
// The header's other reasons are the panel's own.

export type SetupRowName =
  | "Environment"
  | "Site address"
  | "Sign-in"
  | "Accounts"
  | "Areas"
  | "Discovery"
  | "Components"
  | "Known quirks"
  | "Test files"
  | "Fixtures"
  | "Database";

/** The rows Basic mode leaves out unless one of them blocks a run. The
 * Sign-in row stays, but hides its recipe editor and recording there. */
export const SIMPLE_HIDDEN: ReadonlySet<SetupRowName> = new Set<SetupRowName>(["Discovery", "Components", "Known quirks", "Fixtures"]);

/** The hidden rows Simple does not bring back even when they block a run. */
export const NEVER_SHOWN_IN_SIMPLE: ReadonlySet<SetupRowName> = new Set<SetupRowName>(["Fixtures"]);

/** The note under a row that Simple shows only because it blocks a run. */
export const BLOCKER_NOTE = "Shown because a run needs it. Ask the assistant to sort it out, or switch to Advanced.";

/** Whether a failed fixture stops a run: either count is above zero. */
export function fixtureBlock(fixturesFailed: number, fixturesFailedHere: number): boolean {
  return fixturesFailed > 0 || fixturesFailedHere > 0;
}

/** The rows that stop a run now. */
export function blockingRows(input: {
  signIn: "saved" | "builtin" | "none" | null;
  /** The saved recipe could not be read: a run cannot tell how to sign in. */
  recipeUnreadable?: boolean;
  discovering: boolean;
  fixturesFailed: number;
  fixturesFailedHere: number;
}): Set<SetupRowName> {
  const out = new Set<SetupRowName>();
  if (input.discovering) out.add("Discovery");
  if (fixtureBlock(input.fixturesFailed, input.fixturesFailedHere)) out.add("Fixtures");
  if (input.signIn === "none" || input.recipeUnreadable) out.add("Sign-in");
  return out;
}

/** Whether Basic mode shows `row`: it is not one of the hidden rows, or it
 * blocks a run and is not one Simple never shows. */
export function shownInSimple(row: SetupRowName, blocking: Set<SetupRowName>): boolean {
  if (NEVER_SHOWN_IN_SIMPLE.has(row)) return false;
  return !SIMPLE_HIDDEN.has(row) || blocking.has(row);
}
