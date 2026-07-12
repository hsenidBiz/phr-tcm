import { forwardRef, type SelectHTMLAttributes } from "react";
import { cn } from "../../lib/cn";

/** Styled native select - Radix select arrives with Iteration 2 if needed. */
export const Select = forwardRef<HTMLSelectElement, SelectHTMLAttributes<HTMLSelectElement>>(
  ({ className, ...props }, ref) => (
    <select
      ref={ref}
      className={cn(
        "rounded-md border border-border bg-surface px-3 py-2 text-sm text-text transition-colors hover:border-border-strong focus:border-accent focus:outline-none disabled:opacity-50",
        className,
      )}
      {...props}
    />
  ),
);
Select.displayName = "Select";
