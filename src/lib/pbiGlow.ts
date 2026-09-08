/** Cross-component signal: the review gate's final confirmation highlights
 * the PBI chip in the context bar so the user verifies the target before an
 * irreversible create. */
export const PBI_GLOW_EVENT = "tcm-pbi-glow";

export function setPbiGlow(on: boolean) {
  window.dispatchEvent(new CustomEvent(PBI_GLOW_EVENT, { detail: on }));
}
