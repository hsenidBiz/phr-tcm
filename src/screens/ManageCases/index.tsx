import { useState } from "react";
import SuitePicker, { type PickedSuite } from "./SuitePicker";

/** Bulk operations on one suite's test cases: re-order them, move them to
 * another PBI, copy them into folders. The suite comes from the plan tree;
 * everything below the picker works on that one suite. */
export default function ManageCases({ org, project }: { org: string; project: string }) {
  const [picked, setPicked] = useState<PickedSuite | null>(null);

  if (!org || !project) {
    return (
      <p className="text-sm text-muted">
        Pick an organization and project in the bar above to manage test cases.
      </p>
    );
  }

  return (
    <div className="space-y-4">
      <SuitePicker org={org} project={project} picked={picked} onPick={setPicked} />
      {!picked && (
        <p className="text-sm text-muted">
          Pick a test plan and a suite. Its test cases appear here in the order Azure DevOps shows them.
        </p>
      )}
      {picked && (
        <p className="text-sm text-muted">
          {picked.planName} / {picked.suite.name}
        </p>
      )}
    </div>
  );
}
