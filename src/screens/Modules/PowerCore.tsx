import { IconCore } from "../../lib/actionIcons";
import { CORE_LINE, CORE_NAME } from "./modulesCopy";

/** The tree's root: a glowing core with two rings pulsing outward (they stop
 * under reduced motion, see index.css). The orb is decoration; the name and
 * its line are read out. */
export default function PowerCore() {
  return (
    <div className="mod-core">
      <div aria-hidden className="mod-core-orb">
        <span className="mod-core-ring" />
        <span className="mod-core-ring mod-core-ring-late" />
        <IconCore aria-hidden className="size-6" />
      </div>
      {/* Above the orb, with a thin halo of the board's colour around the
          letters only (no box), so its rings pass behind the words. */}
      <div className="mod-core-text relative z-10 text-sm font-semibold text-text">{CORE_NAME}</div>
      <div className="mod-core-text relative z-10 -mt-1 text-xs text-muted">{CORE_LINE}</div>
    </div>
  );
}
