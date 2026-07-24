import type { IterationRef } from "../bindings";

/** Combobox `details` map for iteration pickers: sprint date ranges
 * rendered like Azure DevOps's own iteration dropdown (dd/mm/yyyy).
 * Undated nodes (project root, grouping folders) get no annotation. */
export function iterationDetails(iterations: IterationRef[]): Record<string, string> {
  const fmt = (iso: string) => new Date(iso).toLocaleDateString("en-GB");
  const out: Record<string, string> = {};
  for (const it of iterations) {
    if (it.start_date && it.finish_date) {
      out[it.path] = `${fmt(it.start_date)} - ${fmt(it.finish_date)}`;
    }
  }
  return out;
}
