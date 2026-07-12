import type { PbiHit } from "../bindings";
import RunPanel from "./RunPanel";

export default function RunTests({
  org,
  project,
  pbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
}) {
  if (!org || !project || !pbi) {
    return (
      <p className="text-sm text-muted">
        Pick an organization, project and PBI in the bar above to run its test
        cases.
      </p>
    );
  }
  return <RunPanel org={org} project={project} pbiId={pbi.id} pbiTitle={pbi.title} />;
}
