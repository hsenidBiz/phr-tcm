/** Display label only - the ADO value is unchanged. Capitalizes the raw
 * lowercase outcomes ("passed" -> "Passed") and spells out Not Applicable. */
export function outcomeLabel(o: string): string {
  if (!o) return "";
  const k = o.toLowerCase();
  if (k === "notapplicable") return "Not Applicable";
  return k[0].toUpperCase() + k.slice(1);
}
