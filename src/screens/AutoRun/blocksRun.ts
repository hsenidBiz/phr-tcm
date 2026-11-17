// Which Setup rows stand between the person and a run, for Simple mode: it
// hides the rows only an advanced user tends, and a hidden row that stops a
// run is shown again with a note. `fixtureBlock` is the one test of whether a
// failed fixture stops a run; `blockingRows` and the panel header's fixture
// reasons both use it. The header's other reasons are the panel's own.

export type SetupRowName =
  | "Environment"
  | "Site address"
  | "Sign-in"
  | "Accounts"
  | "Areas"
  | "Discovery"
  | "Components"
  | "Test files"
  | "Fixtures"
  | "Database";

/** The rows Simple mode leaves out unless one of them blocks a run. The
 * Sign-in row stays, but hides its recipe editor and recording there. */
export const SIMPLE_HIDDEN: ReadonlySet<SetupRowName> = new Set<SetupRowName>(["Discovery", "Components", "Fixtures"]);

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

/** Whether Simple mode shows `row`: it is not one of the hidden rows, or it
 * blocks a run. */
export function shownInSimple(row: SetupRowName, blocking: Set<SetupRowName>): boolean {
  return !SIMPLE_HIDDEN.has(row) || blocking.has(row);
}
