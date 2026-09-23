import { forwardRef, type InputHTMLAttributes, type TextareaHTMLAttributes } from "react";
import { Textarea as XiodTextarea } from "xiod-ui/textarea";
import { cn } from "../../lib/cn";

const base =
  "rounded-md border border-border bg-surface px-3 py-2 text-sm text-text transition-colors placeholder:text-faint hover:border-border-strong focus:border-accent focus:outline-none disabled:opacity-50";

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  ({ className, ...props }, ref) => (
    <input ref={ref} className={cn(base, className)} {...props} />
  ),
);
Input.displayName = "Input";

/** XiodUI's text area sets its own size at sm: and up (`sm:text-sm`), and a
 * breakpoint class always outranks a plain one - so a call site's `text-xs`
 * would lose in any window wider than 640px. Its sm: twin goes along with
 * it. Whole class names, so Tailwind finds them in this file. */
const SM_TWIN: Record<string, string> = {
  "text-xs": "sm:text-xs",
  "text-sm": "sm:text-sm",
  "text-base": "sm:text-base",
  "text-lg": "sm:text-lg",
};

function smTwin(className?: string): string | undefined {
  const size = className?.split(/\s+/).find((c) => Object.prototype.hasOwnProperty.call(SM_TWIN, c));
  return size ? SM_TWIN[size] : undefined;
}

/**
 * XiodUI's text area: a styled box with the field filling it. The call
 * site's `className` sizes the BOX (h-*, w-*, flex-1, min-h-*, margins, a
 * ring), which is also what the user drags taller; the field fills it and
 * does not grow with its content. Every other prop, `ref` included, lands
 * on the `<textarea>`.
 */
export const Textarea = forwardRef<
  HTMLTextAreaElement,
  TextareaHTMLAttributes<HTMLTextAreaElement>
>(({ className, ...props }, ref) => (
  <XiodTextarea
    ref={ref}
    className={cn(
      "flex resize-y overflow-hidden text-sm [&>textarea]:min-h-0 [&>textarea]:resize-none [&>textarea]:[field-sizing:fixed] [&>textarea]:placeholder:text-faint",
      className,
      smTwin(className),
    )}
    {...props}
  />
));
Textarea.displayName = "Textarea";
