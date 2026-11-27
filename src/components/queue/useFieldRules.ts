import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo } from "react";
import { commands, type RuleCheck, type TestCase } from "../../bindings";
import type { FieldPrefs } from "../../lib/fieldPrefs";
import { fieldRuleSummary, RULES_UNCHECKED, rowFieldProblems, ruleInputs } from "../../lib/fieldRules";
import { toast } from "../../lib/toast";

/** Projects the "could not be checked" sentence has been shown for, this
 * run. Once is enough: the rules are asked for again on every edit. */
const saidUnchecked = new Set<string>();

/** The project's Test Case field rules against the queue: a warning per row
 * and a summary, before anything is sent. Fails open - rules that cannot be
 * read block nothing, and the person is told once. */
export function useFieldRules({
  org,
  project,
  queue,
  prefs,
}: {
  org: string;
  project: string;
  queue: TestCase[];
  prefs: FieldPrefs;
}) {
  const inputs = useMemo(() => ruleInputs(queue), [queue]);
  const check = useQuery({
    queryKey: ["field-rules", org, project, prefs.moduleRef, prefs.preconditionsRef, inputs],
    queryFn: async (): Promise<RuleCheck | null> =>
      // Null where the backend gave no answer at all: nothing to say.
      (await commands.checkFieldRules(org, project, queue, prefs.moduleRef, prefs.preconditionsRef)) ?? null,
    enabled: Boolean(org && project) && queue.length > 0,
    staleTime: 60_000,
    retry: false,
  });
  const data = check.data ?? null;

  const unchecked = data?.checked === false;
  useEffect(() => {
    const key = `${org}/${project}`;
    if (!unchecked || saidUnchecked.has(key)) return;
    saidUnchecked.add(key);
    toast.warning(RULES_UNCHECKED);
  }, [unchecked, org, project]);

  const rowProblems = useMemo(() => rowFieldProblems(data, queue.length), [data, queue.length]);
  const summary = useMemo(() => fieldRuleSummary(data?.problems ?? []), [data]);
  return { rowProblems, summary, blocked: rowProblems.some(Boolean) };
}
