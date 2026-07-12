import type { PbiHit } from "../bindings";
import RecentPbis from "../components/RecentPbis";
import RunPanel from "./RunPanel";

export default function RunTests({
  org,
  project,
  pbi,
  onPickPbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  onPickPbi?: (pbi: PbiHit) => void;
}) {
  if (!org || !project || !pbi) {
    return (
      <div>
        <p className="text-sm text-muted">
          Pick an organization, project and PBI in the bar above to run its
          test cases.
        </p>
        <RecentPbis org={org} project={project} onPick={onPickPbi} />
      </div>
    );
  }
  return <RunPanel org={org} project={project} pbiId={pbi.id} pbiTitle={pbi.title} />;
}
