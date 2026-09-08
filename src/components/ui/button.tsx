import { cva, type VariantProps } from "class-variance-authority";
import { forwardRef, type ButtonHTMLAttributes } from "react";
import { cn } from "../../lib/cn";

const buttonVariants = cva(
  // A leading icon is sized by the button rather than by each call site, so
  // the same action can't end up 14px on one screen and 16px on another.
  // `shrink-0` keeps it from being squeezed when a long label wraps the row.
  "inline-flex items-center justify-center gap-2 rounded-md font-medium transition-colors focus-visible:outline-2 focus-visible:outline-accent disabled:pointer-events-none disabled:opacity-50 [&_svg]:shrink-0 [&_svg]:pointer-events-none",
  {
    variants: {
      variant: {
        default: "bg-accent text-on-accent hover:bg-accent-hover",
        outline:
          "border border-border text-text hover:border-accent hover:text-accent",
        ghost: "text-muted hover:bg-surface-2 hover:text-text",
        danger: "bg-danger text-on-accent hover:opacity-90",
        pill: "rounded-full border border-accent/60 text-accent hover:bg-accent-soft",
      },
      size: {
        sm: "px-3 py-1.5 text-xs [&_svg]:size-3.5",
        md: "px-4 py-2 text-sm [&_svg]:size-4",
      },
    },
    defaultVariants: { variant: "default", size: "md" },
  },
);

export interface ButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, ...props }, ref) => (
    <button
      ref={ref}
      className={cn(buttonVariants({ variant, size }), className)}
      {...props}
    />
  ),
);
Button.displayName = "Button";
