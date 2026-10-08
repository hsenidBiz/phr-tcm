// The body of an inline case diff on a queue row: each changed field as
// old and new words, then the step changes. Shared by the Azure DevOps diff
// on an UPDATE row and the watched-file diff on a NEW row, so the two read
// identically.

import type { StepDiff } from "../lib/caseDiff";
import InlineDiff from "./InlineDiff";
import StepDiffLines from "./StepDiffLines";

export default function CaseChangeDetail({
  fields,
  steps,
  org,
}: {
  fields: { name: string; old: string; new: string }[];
  steps: StepDiff[];
  org?: string;
}) {
  return (
    <>
      {/* Word-level, like the step lines below: editing one word of a
          title must not read as the whole title being replaced. */}
      {fields.map((f) => (
        <div key={f.name}>
          <span className="font-medium text-muted">{f.name}:</span> <InlineDiff old={f.old} next={f.new} />
        </div>
      ))}
      {steps.length > 0 && (
        <div className="space-y-1">
          <span className="font-medium text-muted">Steps:</span>
          {/* git word-diff style: -/+ lines with only the actually-changed
              words highlighted. */}
          {steps.map((d) => (
            <StepDiffLines key={d.index} d={d} org={org} />
          ))}
        </div>
      )}
    </>
  );
}
