/**
 * Pulls human-readable field names out of Azure DevOps rule-validation
 * error messages, so a failed board move can say WHICH fields block the
 * transition instead of dumping the raw server text. Typical shapes:
 *   "TF401320: Rule Error for field Remaining Work. Error code: Required..."
 *   "VS403691: ... The field 'Activity' cannot be empty."
 *   "Field 'Microsoft.VSTS.Common.Activity' is required"
 * Unknown shapes just return [] and the caller falls back to the raw text.
 */
export function requiredFieldsFromError(message: string): string[] {
  const found = new Set<string>();
  const add = (raw: string) => {
    // Reference names come dotted - the last segment is the display-ish name.
    const name = raw.trim().split(".").pop() ?? raw;
    // Split glued PascalCase ("RemainingWork" -> "Remaining Work").
    const spaced = name.replace(/([a-z])([A-Z])/g, "$1 $2").trim();
    if (spaced) found.add(spaced);
  };
  for (const m of message.matchAll(/field '([^']+)'/gi)) add(m[1]);
  for (const m of message.matchAll(/Rule Error for field ([A-Za-z0-9. ]+?)\./g)) add(m[1]);
  for (const m of message.matchAll(/'([^']+)' (?:cannot be empty|is required|must be)/gi)) add(m[1]);
  return [...found];
}
