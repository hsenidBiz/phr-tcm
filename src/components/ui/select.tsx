import { Check, ChevronsUpDown } from "lucide-react";
import {
  Children,
  isValidElement,
  useEffect,
  useRef,
  useState,
  type OptionHTMLAttributes,
  type ReactElement,
  type ReactNode,
  type SelectHTMLAttributes,
} from "react";
import { cn } from "../../lib/cn";

/**
 * The app's dropdown: a themed trigger + listbox, replacing the styled
 * NATIVE select whose popup was drawn by the OS and matched nothing else
 * on screen (the one part CSS cannot reach).
 *
 * Deliberately keeps the native select's API - `value`, `onChange` with an
 * `e.target.value`, `<option>` children, `aria-label` - so its twenty
 * call sites read exactly as before. The options are parsed out of the
 * children; the change handler receives a minimal `{ target: { value } }`,
 * which is the only shape any caller ever read.
 *
 * The searchable big-list cousin stays `ui/combobox.tsx`; this one is for
 * short fixed lists, where a search box would be noise.
 */
type OptionSpec = { value: string; label: string; disabled: boolean };

function optionsOf(children: ReactNode): OptionSpec[] {
  const out: OptionSpec[] = [];
  Children.forEach(children, (child) => {
    if (!isValidElement(child)) return;
    const el = child as ReactElement<OptionHTMLAttributes<HTMLOptionElement>>;
    if (el.type !== "option") return;
    const label =
      typeof el.props.children === "string" ? el.props.children : String(el.props.children ?? "");
    // A bare <option>Planned</option> has no value attribute - the native
    // element falls back to its text, and so does this.
    const value = el.props.value != null ? String(el.props.value) : label;
    out.push({ value, label, disabled: Boolean(el.props.disabled) });
  });
  return out;
}

export function Select({
  className,
  triggerClassName,
  children,
  value,
  onChange,
  disabled,
  "aria-label": ariaLabel,
  "data-tour": dataTour,
}: SelectHTMLAttributes<HTMLSelectElement> & {
  /** The wrapper div takes `className` (width, positioning). Padding and
   * sizing belong HERE - callers used to put py-* on `className` and
   * silently pad the wrapper around an unchanged 38px trigger. */
  triggerClassName?: string;
  /** The guided tour rings elements by this attribute. Named explicitly
   * because this component takes only the props it uses - anything else
   * is dropped, and a dropped anchor is a tour stop ringing nothing. */
  "data-tour"?: string;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const options = optionsOf(children);
  const current = options.find((o) => o.value === String(value ?? ""));

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [open]);

  const commit = (v: string) => {
    setOpen(false);
    // The minimal shape every caller reads. Nothing ever touched the rest
    // of the event, so nothing else is fabricated.
    onChange?.({ target: { value: v } } as unknown as React.ChangeEvent<HTMLSelectElement>);
  };

  const openAt = () => {
    setActive(Math.max(0, options.findIndex((o) => o.value === current?.value)));
    setOpen(true);
  };

  return (
    <div ref={rootRef} data-tour={dataTour} className={cn("relative", className)}>
      <button
        type="button"
        role="combobox"
        aria-expanded={open}
        aria-haspopup="listbox"
        aria-label={ariaLabel}
        disabled={disabled}
        className={cn(
          "flex w-full items-center justify-between gap-2 rounded-md border border-border bg-surface px-3 py-2 text-left text-sm text-text transition-colors hover:border-border-strong focus:border-accent focus:outline-none disabled:opacity-50",
          // Open state matches focus: the trigger stays lit while the
          // listbox is showing, same accent as every other open control.
          open && "border-accent",
          triggerClassName,
        )}
        onClick={() => (open ? setOpen(false) : openAt())}
        onKeyDown={(e) => {
          if (!open && (e.key === "ArrowDown" || e.key === "Enter" || e.key === " ")) {
            e.preventDefault();
            openAt();
          } else if (open) {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setActive((a) => Math.min(a + 1, options.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, 0));
            } else if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              const o = options[active];
              if (o && !o.disabled) commit(o.value);
            } else if (e.key === "Escape") {
              e.stopPropagation();
              setOpen(false);
            }
          }
        }}
      >
        <span className={cn("truncate", current ? "text-text" : "text-faint")}>
          {current?.label ?? ""}
        </span>
        <ChevronsUpDown size={13} className="shrink-0 text-muted" />
      </button>

      {open && (
        <ul
          role="listbox"
          aria-label={ariaLabel}
          className="absolute left-0 top-full z-40 mt-1 max-h-56 w-full min-w-32 overflow-y-auto rounded-md border border-border bg-surface p-1 shadow-xl"
        >
          {options.map((o, i) => (
            <li key={o.value + i}>
              <button
                type="button"
                role="option"
                aria-selected={o.value === current?.value}
                disabled={o.disabled}
                className={cn(
                  "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm",
                  i === active ? "bg-accent-soft text-accent" : "text-text hover:bg-surface-2",
                  o.disabled && "opacity-50",
                )}
                onMouseEnter={() => setActive(i)}
                onClick={() => commit(o.value)}
              >
                <Check
                  size={13}
                  className={cn(
                    "shrink-0",
                    o.value === current?.value ? "opacity-100" : "opacity-0",
                  )}
                />
                {o.label}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
