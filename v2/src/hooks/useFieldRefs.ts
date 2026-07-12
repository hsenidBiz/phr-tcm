import { useQuery } from "@tanstack/react-query";
import { commands } from "../bindings";
import { autoPick, loadFieldPrefs, saveFieldPrefs, type FieldPrefs } from "../lib/fieldPrefs";
import { unwrap } from "../lib/ipc";

/** The project's available Test Case fields plus the chosen (or auto-picked)
 * module/preconditions refs. Discovery failure degrades to skip - it never
 * blocks creating test cases (v1 behaviour). */
export function useFieldRefs(org: string, project: string) {
  const fields = useQuery({
    queryKey: ["tc-fields", org, project],
    queryFn: () => unwrap(commands.listTestCaseFields(org, project)),
    enabled: Boolean(org && project),
    staleTime: 10 * 60_000,
    retry: false,
  });

  let prefs: FieldPrefs = { moduleRef: null, preconditionsRef: null };
  if (org && project) {
    const stored = loadFieldPrefs(org, project);
    if (stored) {
      prefs = stored;
    } else if (fields.data) {
      prefs = autoPick(fields.data);
      saveFieldPrefs(org, project, prefs);
    }
  }

  return { fields, prefs };
}
