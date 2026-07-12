import { useQuery } from "@tanstack/react-query";
import { commands } from "../bindings";
import { useFieldRefs } from "../hooks/useFieldRefs";
import { unwrap } from "../lib/ipc";
import { Input } from "./ui/input";
import { Select } from "./ui/select";

/** Module picker: the org defines set Modules (a picklist on the mapped
 * field), so this renders a dropdown of allowed values; free-entry only
 * when the field has no picklist or none is mapped. */
export default function ModuleField({
  org,
  project,
  value,
  onChange,
  className,
}: {
  org: string;
  project: string;
  value: string;
  onChange: (v: string) => void;
  className?: string;
}) {
  const { prefs } = useFieldRefs(org, project);

  const values = useQuery({
    queryKey: ["module-values", org, project, prefs.moduleRef],
    queryFn: () => unwrap(commands.testCaseFieldValues(org, project, prefs.moduleRef!)),
    enabled: Boolean(org && project && prefs.moduleRef),
    staleTime: 60 * 60_000,
  });

  if ((values.data?.length ?? 0) > 0) {
    return (
      <Select
        aria-label="Module"
        className={className}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="">Module</option>
        {!values.data!.includes(value) && value && <option value={value}>{value}</option>}
        {values.data!.map((m) => (
          <option key={m}>{m}</option>
        ))}
      </Select>
    );
  }
  return (
    <Input
      aria-label="Module"
      className={className}
      placeholder="Module (optional)"
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}
