import type { PbiHit } from "../bindings";
import PickPbiEmpty from "../components/PickPbiEmpty";
import RunPanel from "./RunPanel";

/** A Test plan folder opened from Test plans: Run Tests works on its own
 * cases, in the folder's own project. */
export type RunFolderTarget = {
  planId: number;
  suiteId: number;
  planName: string;
  title: string;
  project: string;
};

export default function RunTests({
  org,
  project,
  pbi,
  folder,
  onPickPbi,
  onCloseFolder,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  folder?: RunFolderTarget | null;
  onPickPbi?: (pbi: PbiHit) => void;
  /** Leave the folder: back to the PBI's Run Tests, or to Test plans when
   * no PBI is picked. */
  onCloseFolder?: () => void;
}) {
  if (org && folder) {
    return (
      <RunPanel
        // A different folder starts a fresh selection.
        key={`${folder.project}/${folder.planId}/${folder.suiteId}`}
        org={org}
        project={folder.project || project}
        folder={folder}
        onCloseFolder={onCloseFolder}
        closeLabel={pbi ? `Back to PBI #${pbi.id}` : "Back to Test plans"}
      />
    );
  }
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
