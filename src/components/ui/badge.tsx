import { type HTMLAttributes } from "react";
import { cn } from "../../lib/cn";

export function Badge({
  className,
  color,
  ...props
}: HTMLAttributes<HTMLSpanElement> & { color?: string }) {
  return (
    <span
      className={cn(
        "pill-label inline-flex items-center rounded px-1.5 text-[10px] font-semibold",
        !color && "bg-surface-2 text-muted",
        className,
      )}
      style={color ? { backgroundColor: color, color: "#111" } : undefined}
      {...props}
    />
  );
}
