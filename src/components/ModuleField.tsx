import { useQuery } from "@tanstack/react-query";
import { commands } from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { unwrap } from "../lib/ipc";
import Combobox from "./ui/combobox";

/** Module picker: a searchable dropdown of the org's allowed Modules (the
 * picklist on the mapped field). Custom entry is allowed too, so projects
 * with no picklist (or an unmapped field) can still type a value. */
export default function ModuleField({
  org,
  project,
  value,
  onChange,
  className,
  placeholder = "Module",
}: {
  org: string;
  project: string;
  value: string;
  onChange: (v: string) => void;
  className?: string;
  /** What the empty picker says (a bulk edit says "Leave unchanged"). */
  placeholder?: string;
}) {
  const { prefs } = useFieldRefs(org, project);

  const values = useQuery({
    queryKey: ["module-values", org, project, prefs.moduleRef],
    queryFn: () => unwrap(commands.testCaseFieldValues(org, project, prefs.moduleRef!)),
    enabled: Boolean(org && project && prefs.moduleRef),
    staleTime: 60 * 60_000,
  });

  return (
    <Combobox
      ariaLabel="Module"
      className={className}
      placeholder={placeholder}
      value={value}
      onChange={onChange}
      options={values.data ?? []}
      loading={values.isLoading}
      allowCustom
    />
  );
}
