import type { PbiHit } from "../bindings";
import PickPbiEmpty from "../components/PickPbiEmpty";
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
      <PickPbiEmpty
        message="Pick an organization, project and PBI in the bar above to run its test cases."
        org={org}
        project={project}
        onPickPbi={onPickPbi}
      />
    );
  }
  return <RunPanel org={org} project={project} pbiId={pbi.id} pbiTitle={pbi.title} />;
}
