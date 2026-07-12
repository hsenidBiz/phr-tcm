import type { PbiHit } from "../bindings";
import RecentPbis from "../components/RecentPbis";
import ExistingCases from "./ExistingCases";

export default function EditCases({
  org,
  project,
  pbi,
  caseSelection,
  onClearSelection,
  onPickPbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  /** Suite-browser handoff: edit these exact cases instead of a PBI's. */
  caseSelection?: { label: string; caseIds: number[] } | null;
  onClearSelection?: () => void;
  onPickPbi?: (pbi: PbiHit) => void;
}) {
  if (caseSelection && org && project) {
    return (
      <div className="space-y-2">
        <div className="flex items-center gap-2 text-xs text-muted">
          <span>Showing cases handed over from the Test Suites browser.</span>
          <button className="text-accent hover:underline" onClick={onClearSelection}>
            Back to PBI cases
          </button>
        </div>
        <ExistingCases
          org={org}
          project={project}
          pbiId={null}
          caseIds={caseSelection.caseIds}
          label={caseSelection.label}
        />
      </div>
    );
  }
  if (!org || !project || !pbi) {
    return (
      <div>
        <p className="text-sm text-muted">
          Pick an organization, project and PBI in the bar above to edit its
          linked test cases.
        </p>
        <RecentPbis org={org} project={project} onPick={onPickPbi} />
      </div>
    );
  }
  return <ExistingCases org={org} project={project} pbiId={pbi.id} />;
}
