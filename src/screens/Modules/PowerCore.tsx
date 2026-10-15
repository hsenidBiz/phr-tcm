import { IconCore } from "../../lib/actionIcons";
import { CORE_LINE, CORE_NAME } from "./modulesCopy";

/** The tree's root: a glowing core with two rings pulsing outward. Purely
 * decorative (the rings stop under reduced motion, see index.css). */
export default function PowerCore() {
  return (
    <div aria-hidden className="mod-core">
      <div className="mod-core-orb">
        <span className="mod-core-ring" />
        <span className="mod-core-ring mod-core-ring-late" />
        <IconCore aria-hidden className="size-6" />
      </div>
      <div className="text-sm font-semibold text-text">{CORE_NAME}</div>
      <div className="-mt-1 text-xs text-muted">{CORE_LINE}</div>
    </div>
  );
}
