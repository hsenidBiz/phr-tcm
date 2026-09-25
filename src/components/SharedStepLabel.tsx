import { useQuery } from "@tanstack/react-query";
import { commands } from "../bindings";
import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";
import { unwrap } from "../lib/ipc";
import { sharedStepQueryKey } from "../lib/sharedSteps";

/** A Shared Steps reference as one locked line - "Shared steps #812 - Sign
 * in as an admin". Its steps live in that work item, so the app neither
 * shows nor edits them: a row can be moved or removed, nothing more.
 *
 * The title is one work-item read per id: React Query shares it between
 * rows, and lib/cache keeps it across restarts for a day. Without an org
 * (a screen that has none) only the reference is shown. */
export default function SharedStepLabel({ id, org }: { id: number; org?: string }) {
  return (
    <span className="text-xs text-muted">
      <span className="font-semibold text-text">Shared steps #{id}</span>
      {org ? <SharedStepTitle id={id} org={org} /> : null}
    </span>
  );
}

function SharedStepTitle({ id, org }: { id: number; org: string }) {
  const title = useQuery({
    queryKey: sharedStepQueryKey(org, id),
    ...persistentQuery({
      key: cacheKeys.sharedStepTitle(org, id),
      fetcher: async () => {
        const found = await unwrap(commands.testCasesByIds(org, [id], null, null));
        return found[0]?.title ?? "";
      },
      ...CACHE.reference,
      // An empty answer (no access, deleted item) is not worth a day on disk.
      store: (t) => t !== "",
    }),
    retry: false,
  });
  return title.data ? <span>{` - ${title.data}`}</span> : null;
}
