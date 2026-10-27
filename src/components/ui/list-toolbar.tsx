import { Children, type ReactNode } from "react";
import { Switch } from "./switch";

/**
 * The one toolbar line of a list screen: a wide search first, then that
 * screen's filters, each 40px tall so the line is level. It wraps on a
 * narrow window and the search (`flex-1 min-w-64`) gives way before any
 * filter (`shrink-0`) is cut off. Pass `SearchInput` with `className="w-full"`.
 */
export function ListToolbar({ search, children }: { search: ReactNode; children?: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <div className="min-w-64 flex-1">{search}</div>
      {Children.toArray(children).map((child, i) => (
        <div key={i} className="shrink-0">
          {child}
        </div>
      ))}
    </div>
  );
}

/**
 * An on/off filter for a toolbar: the app's switch inside a bordered 40px
 * chip with its label. The label wraps the switch, so clicking either
 * toggles it; the switch is named by `label`.
 */
export function ToolbarSwitch({
  checked,
  onCheckedChange,
  label,
  title,
}: {
  checked: boolean;
  onCheckedChange: (v: boolean) => void;
  label: string;
  title?: string;
}) {
  return (
    <label
      title={title}
      className="flex h-10 shrink-0 items-center gap-2 rounded-md border border-border px-3 text-sm text-muted"
    >
      <Switch checked={checked} onCheckedChange={onCheckedChange} ariaLabel={label} />
      {label}
    </label>
  );
}
