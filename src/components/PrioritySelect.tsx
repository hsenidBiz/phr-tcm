import { Select } from "./ui/select";

/** What a create writes when a case sets no priority (the Rust side's
 * `DEFAULT_PRIORITY`). */
export const DEFAULT_PRIORITY = 2;

/** Azure DevOps' Priority picklist. */
export const PRIORITIES = [1, 2, 3, 4] as const;

/**
 * The test case Priority field (`Microsoft.VSTS.Common.Priority`).
 *
 * `value` null means the case sets none. On a new case that is created
 * with 2, so the select shows 2. On an update it means "leave Azure
 * DevOps' value alone", which `keepLabel` names as its own option, so an
 * update never quietly resets a priority to 2.
 */
export default function PrioritySelect({
  value,
  onChange,
  keepLabel,
  className,
}: {
  value: number | null | undefined;
  onChange: (next: number | null) => void;
  /** When given, an extra first option with this label stands for null. */
  keepLabel?: string;
  className?: string;
}) {
  const shown = value == null ? (keepLabel ? "" : String(DEFAULT_PRIORITY)) : String(value);
  return (
    <Select
      aria-label="Priority"
      className={className}
      value={shown}
      onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))}
    >
      {keepLabel && <option value="">{keepLabel}</option>}
      {PRIORITIES.map((p) => (
        <option key={p} value={String(p)}>
          {`Priority ${p}`}
        </option>
      ))}
    </Select>
  );
}
