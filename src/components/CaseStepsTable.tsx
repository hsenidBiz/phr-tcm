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
import { renderMarkdown } from "../lib/markdown";
import SharedStepLabel from "./SharedStepLabel";

export default function CaseStepsTable({
  steps,
  preconditions,
  reviewerNotes,
  org,
}: {
  steps: Step[];
  /** Rendered above the table when present - a step often only makes sense
   * given the state the case starts in. */
  preconditions?: string;
  /** Where the case came from: the spec section or the code symbol behind
   * it. Shown above the steps, the same place and order the browser review
   * page puts it, because that is where a reviewer looks for it.
   *
   * Until now this field existed everywhere EXCEPT the app: it round-trips
   * through the JSON, renders in the browser page, and shows up in a change
   * diff - but no screen displayed it. Someone who received a shared draft,
   * which is exactly who the field is written for, could not read it
   * without exporting to a browser first. */
  reviewerNotes?: string;
  /** For the titles of Shared Steps rows; without it only the reference shows. */
  org?: string;
}) {
  const notes = reviewerNotes?.trim();
  return (
    <>
      {preconditions ? (
        <p className="whitespace-pre-wrap border-b border-border/60 px-3 py-2 text-xs text-muted">
          <span className="font-semibold">Preconditions: </span>
          {preconditions}
        </p>
      ) : null}
      {notes ? (
        // The same box the browser review page draws around notes: tinted
        // with the accent and given a left rule, so it reads as commentary
        // ABOUT the case rather than part of it.
        <div className="mx-3 my-2 rounded-md border border-accent/30 border-l-[3px] border-l-accent bg-accent-soft px-3 py-2">
          <p className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-accent">
            Reviewer notes
          </p>
          {/* Markdown, like everywhere else this field is shown - a citation
              carries a link and sometimes a quote. `renderMarkdown`
              sanitises; see lib/markdown.ts for why that is not optional. */}
          <div
            className="md-preview text-xs text-muted"
            dangerouslySetInnerHTML={{ __html: renderMarkdown(notes) }}
          />
        </div>
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
                {s.shared != null ? (
                  <td colSpan={2} className="px-3 py-1">
                    <SharedStepLabel id={s.shared} org={org} />
                  </td>
                ) : (
                  <>
                    <td className="whitespace-pre-wrap px-3 py-1 text-text">{s.action}</td>
                    <td className="whitespace-pre-wrap px-3 py-1 text-muted">{s.expected}</td>
                  </>
                )}
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
