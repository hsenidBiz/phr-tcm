/** A count and the word that goes with it: "1 case", "3 cases". Pass
 * `many` when adding an "s" is not the plural ("1 entry", "2 entries"). */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}
