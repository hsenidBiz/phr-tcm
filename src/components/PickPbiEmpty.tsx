import type { PbiHit } from "../bindings";
import EqRing from "./EqRing";
import FlaskLogo from "./FlaskLogo";
import RecentPbis from "./RecentPbis";

/** Shared pick-a-PBI empty state: prompt + recent PBIs on the left, an
 * ambient flask animation filling the otherwise-empty right half
 * (decorative only - hidden from AT and from narrow windows, stilled
 * under prefers-reduced-motion). The halo ring doubles as a circular
 * equalizer while system audio plays. */
export default function PickPbiEmpty({
  message,
  org,
  project,
  onPickPbi,
}: {
  message: string;
  org: string;
  project: string;
  onPickPbi?: (pbi: PbiHit) => void;
}) {
  return (
    <div className="flex min-h-[60vh] gap-10">
      <div className="min-w-0 flex-1">
        <p className="text-sm text-muted">{message}</p>
        <RecentPbis org={org} project={project} onPick={onPickPbi} />
      </div>
      <div className="hidden flex-1 items-center justify-center lg:flex" aria-hidden="true">
        <div className="empty-art">
          <span className="halo" />
          <span className="halo halo2" />
          <EqRing radius={76} maxLen={20} color="var(--color-accent)" />
          <FlaskLogo size={96} />
          <span className="bubble b1" />
          <span className="bubble b2" />
          <span className="bubble b3" />
        </div>
      </div>
    </div>
  );
}
