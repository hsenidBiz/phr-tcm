// One test case's steps, read straight through: preconditions, then a
// numbered Action/Expected table.
//
// Extracted from the review list so the watched-file change report can show
// the SAME thing. The report used to show only the fragments that changed,
// which reads fine for a one-word edit and not at all for a case where you
// need to know what step 3 was in order to judge step 4. Being able to read
// the case in order is the whole point, and two hand-written copies of this
// table would have drifted the first time either was touched.

import type { Step } from "../bindings";

export default function CaseStepsTable({
  steps,
  preconditions,
}: {
  steps: Step[];
  /** Rendered above the table when present - a step often only makes sense
   * given the state the case starts in. */
  preconditions?: string;
}) {
  return (
    <>
      {preconditions ? (
        <p className="whitespace-pre-wrap border-b border-border/60 px-3 py-2 text-xs text-muted">
          <span className="font-semibold">Preconditions: </span>
          {preconditions}
        </p>
      ) : null}
      {steps.length > 0 ? (
        <table className="w-full border-collapse text-xs">
          <thead>
            <tr className="text-left text-faint">
              <th className="w-8 px-3 py-1 font-medium">#</th>
              <th className="px-3 py-1 font-medium">Action</th>
              <th className="px-3 py-1 font-medium">Expected</th>
            </tr>
          </thead>
          <tbody>
            {steps.map((s, si) => (
              <tr key={si} className="border-t border-border/40 align-top">
                <td className="px-3 py-1 text-faint">{si + 1}</td>
                <td className="whitespace-pre-wrap px-3 py-1 text-text">{s.action}</td>
                <td className="whitespace-pre-wrap px-3 py-1 text-muted">{s.expected}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : (
        <p className="px-3 py-2 text-xs text-muted">This test case has no steps.</p>
      )}
    </>
  );
}
