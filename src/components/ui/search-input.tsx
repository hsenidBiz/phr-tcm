import { forwardRef, useRef, type InputHTMLAttributes } from "react";
import { IconCancel, IconSearch } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { Input } from "./input";

type SearchInputProps = Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "type"> & {
  value: string;
  /** Gets the new text; the clear button calls it with "" (unless `onClear`). */
  onChange: (text: string) => void;
  /** A caller that needs more than emptying the text does it here. */
  onClear?: () => void;
};

/**
 * The app's search box: the shared Input at 40px with a magnifier at its
 * start and, once it has text, a clear button at its end (focus returns to
 * the box). `className` sizes the WRAPPER (w-56, flex-1, min-w-*); the field
 * always fills it. Every other prop lands on the `<input>`.
 */
export const SearchInput = forwardRef<HTMLInputElement, SearchInputProps>(
  ({ className, value, onChange, onClear, ...props }, ref) => {
    const inner = useRef<HTMLInputElement | null>(null);
    const setRef = (el: HTMLInputElement | null) => {
      inner.current = el;
      if (typeof ref === "function") ref(el);
      else if (ref) ref.current = el;
    };
    return (
      <span className={cn("relative flex items-center", className)}>
        <IconSearch
          aria-hidden
          className="pointer-events-none absolute left-3 size-4 shrink-0 text-faint"
        />
        <Input
          ref={setRef}
          type="text"
          className="h-10 w-full min-w-0 pl-9 pr-9 text-sm"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          {...props}
        />
        {value !== "" && (
          <button
            type="button"
            aria-label="Clear search"
            title="Clear search"
            className="absolute right-2 inline-flex size-6 items-center justify-center rounded text-muted transition-colors hover:text-accent focus-visible:outline-2 focus-visible:outline-accent [&_svg]:size-3.5"
            onClick={() => {
              if (onClear) onClear();
              else onChange("");
              inner.current?.focus();
            }}
          >
            <IconCancel aria-hidden />
          </button>
        )}
      </span>
    );
  },
);
SearchInput.displayName = "SearchInput";
