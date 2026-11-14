import { useQuery } from "@tanstack/react-query";
import { commands, type Step } from "../bindings";
import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";
import { unwrap } from "../lib/ipc";
import CaseText from "./CaseText";

type SharedItem = { title: string; steps: Step[] };

/** A Shared Steps reference with the steps inside it, read-only - for the
 * runner, where a tester has to see what to do. The steps live in the
 * Shared Steps work item, so they are read from it (shown from disk at
 * once, then read again) and never edited here. Numbered `3.1`, `3.2`
 * under the case's own step `3`. Without an org only the reference shows. */
export default function SharedStepSteps({
  id,
  org,
  stepNumber,
}: {
  id: number;
  org?: string;
  /** The case step this reference stands in, 1-based. */
  stepNumber: number;
}) {
  const item = useQuery({
    queryKey: ["shared-step-steps", org, id],
    ...persistentQuery<SharedItem>({
      key: cacheKeys.sharedStepSteps(org ?? "", id),
      fetcher: async () => {
        const found = await unwrap(commands.testCasesByIds(org!, [id], null, null));
        const hit = found[0];
        return { title: hit?.title ?? "", steps: (hit?.steps ?? []) as Step[] };
      },
      ...CACHE.outcomes,
      // Nothing read (no access, deleted item) is not worth keeping.
      store: (v) => v.title !== "" || v.steps.length > 0,
    }),
    enabled: Boolean(org),
    retry: false,
  });

  const steps = (item.data?.steps ?? []).filter((s) => s.shared == null);
  return (
    <div className="space-y-1">
      <div className="text-xs text-muted">
        <span className="font-semibold text-text">Shared steps #{id}</span>
        {item.data?.title ? <span>{` - ${item.data.title}`}</span> : null}
      </div>
      {org && item.isLoading && <p className="text-xs text-faint">Loading its steps…</p>}
      {org && item.isError && (
        <p className="text-xs text-danger">
          Its steps could not be read. Open Shared steps #{id} in Azure DevOps to see them.
        </p>
      )}
      {item.isSuccess && steps.length === 0 && (
        <p className="text-xs text-faint">It has no steps.</p>
      )}
      {steps.length > 0 && (
        <ol aria-label={`Steps in shared steps #${id}`} className="space-y-1 border-l-2 border-border pl-2">
          {steps.map((s, j) => (
            <li key={j} className="flex items-start gap-2">
              <span className="id-mono text-xs text-faint">
                {stepNumber}.{j + 1}
              </span>
              <div className="min-w-0 flex-1">
                <CaseText kind="step" text={s.action} />
                {s.expected && (
                  <div className="flex gap-1 text-xs text-muted">
                    <span className="shrink-0">Expect:</span>
                    <CaseText kind="step" text={s.expected} className="min-w-0 flex-1" />
                  </div>
                )}
              </div>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
