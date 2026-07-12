import type { PbiHit } from "../bindings";
import ExistingCases from "./ExistingCases";

export default function EditCases({
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
        Pick an organization, project and PBI in the bar above to edit its
        linked test cases.
      </p>
    );
  }
  return <ExistingCases org={org} project={project} pbiId={pbi.id} />;
}
