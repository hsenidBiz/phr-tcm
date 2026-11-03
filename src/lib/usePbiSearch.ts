import { useQueries } from "@tanstack/react-query";
import type { PbiHit } from "../bindings";
import { searchAlternatives } from "./search";

type Combined = {
  data: PbiHit[] | undefined;
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  /** Some alternatives failed while others returned hits. */
  partial: boolean;
};

/** A PBI search against the server, one request per comma-separated
 * alternative ("1234, login"), their hits combined without duplicates in the
 * order they arrived. `keyPrefix` and `fetcher` are what a single search used
 * to be; with one alternative the query key is the same as before. */
export function usePbiSearch(
  keyPrefix: ReadonlyArray<unknown>,
  query: string,
  enabled: boolean,
  fetcher: (alternative: string) => Promise<PbiHit[]>,
): Combined {
  const result = useQueries({
    queries: searchAlternatives(query).map((alt) => ({
      queryKey: [...keyPrefix, alt],
      queryFn: () => fetcher(alt),
      enabled,
      retry: false,
    })),
    combine: (results): Combined => {
      const failed = results.find((r) => r.isError);
      const pending = results.some((r) => r.isLoading);
      if (results.length === 0 || pending) {
        return {
          data: undefined,
          isLoading: pending,
          isError: false,
          error: null,
          partial: false,
        };
      }
      if (failed && results.every((r) => r.isError || r.data === undefined)) {
        return { data: undefined, isLoading: false, isError: true, error: failed.error, partial: false };
      }
      const seen = new Set<number>();
      const data: PbiHit[] = [];
      for (const r of results) {
        for (const h of r.data ?? []) {
          if (seen.has(h.id)) continue;
          seen.add(h.id);
          data.push(h);
        }
      }
      return { data, isLoading: false, isError: false, error: null, partial: Boolean(failed) };
    },
  });
  // A disabled search has nothing to show, whatever an earlier one cached.
  return enabled ? result : { ...result, data: undefined, isLoading: false, partial: false };
}
