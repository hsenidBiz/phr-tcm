import { cn } from "../lib/cn";

/** The "Beta" mark: beside a beta version's changelog heading, and in the
 *  window's title bar while the app itself is a beta build. */
export default function BetaPill({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "rounded-full bg-warning/15 px-1.5 py-0.5 text-[10px] font-medium leading-none text-warning",
        className,
      )}
    >
      Beta
    </span>
  );
}
