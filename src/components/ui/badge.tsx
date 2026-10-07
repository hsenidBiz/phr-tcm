import { type HTMLAttributes } from "react";
import { cn } from "../../lib/cn";
import { trimLabels } from "./labelText";

export function Badge({
  className,
  color,
  children,
  ...props
}: HTMLAttributes<HTMLSpanElement> & { color?: string }) {
  return (
    <span
      className={cn(
        "inline-flex items-center rounded px-1.5 py-0.5 text-[10px] font-semibold",
        !color && "bg-surface-2 text-muted",
        className,
      )}
      style={color ? { backgroundColor: color, color: "#111" } : undefined}
      {...props}
    >
      {trimLabels(children)}
    </span>
  );
}
