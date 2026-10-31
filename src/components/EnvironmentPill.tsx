import { cn } from "../lib/cn";

/** The active environment's name, in the window's title bar beside the
 *  title. Shown only once there is more than one environment to tell
 *  apart (the caller decides), so a profile with just Default never
 *  sees it. */
export default function EnvironmentPill({ name, className }: { name: string; className?: string }) {
  return (
    <span
      title="Active environment"
      className={cn(
        // inline-flex: its label is a block (label-trim), and inside an inline
        // parent a plain span drew its background as a sliver beside the text.
        "inline-flex items-center rounded-full bg-accent/15 px-1.5 py-0.5 text-[10px] font-medium leading-none text-accent-fill",
        className,
      )}
    >
      <span className="label-trim">{name}</span>
    </span>
  );
}
