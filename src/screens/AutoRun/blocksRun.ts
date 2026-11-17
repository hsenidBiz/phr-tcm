// Which Setup rows stand between the person and a run. Simple mode hides the
// rows only an advanced user tends; a hidden row that stops a run is shown
// again, with a note saying why. The header's "Needs attention" reasons and
// Simple's rule both read `blockingRows`, so they can never disagree.

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

/** The rows that stop a run now. `essentialMissing` is the header's other
 * reasons (no site, no accounts, ...): those rows are never hidden, so it adds
 * no row here. */
export function blockingRows(input: {
  essentialMissing: boolean;
  signIn: "saved" | "builtin" | "none" | null;
  discovering: boolean;
  fixturesFailed: number;
  fixturesFailedHere: number;
}): Set<SetupRowName> {
  const out = new Set<SetupRowName>();
  if (input.discovering) out.add("Discovery");
  if (input.fixturesFailed > 0 || input.fixturesFailedHere > 0) out.add("Fixtures");
  if (input.signIn === "none") out.add("Sign-in");
  return out;
}

/** Whether Simple mode shows `row`: it is not one of the hidden rows, or it
 * blocks a run. */
export function shownInSimple(row: SetupRowName, blocking: Set<SetupRowName>): boolean {
  return !SIMPLE_HIDDEN.has(row) || blocking.has(row);
}
