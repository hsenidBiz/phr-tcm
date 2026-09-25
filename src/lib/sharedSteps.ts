// A Shared Steps reference in words - one helper, so the runner's preview,
// a bug's repro steps and the script editor all say the same thing.

/** A Shared Steps reference as plain text: "Shared steps #812", plus
 * " - <title>" when the title is known. The same words SharedStepLabel
 * shows, for places that need text rather than an element. */
export function sharedStepText(id: number, title?: string | null): string {
  return title ? `Shared steps #${id} - ${title}` : `Shared steps #${id}`;
}

/** The query SharedStepLabel reads a reference's title through. A caller
 * that needs the title synchronously reads it from the query client. */
export function sharedStepQueryKey(org: string, id: number) {
  return ["shared-step", org, id] as const;
}
