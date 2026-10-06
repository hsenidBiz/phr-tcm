/** The key an area or module name is compared by, as runs compare it
 * (`nav::module_key`): trimmed, every run of whitespace one space - a
 * non-breaking or other Unicode space included - and case ignored. */
export function areaKey(name: string): string {
  return name.trim().replace(/\s+/gu, " ").toLowerCase();
}

/** Whether two area or module names are one name. */
export function sameAreaName(a: string, b: string): boolean {
  return areaKey(a) === areaKey(b);
}
