import type { ReactNode } from "react";
import { cn } from "../../lib/cn";

/**
 * A 16px box that turns, holding an icon that does not. A small button sizes
 * its icon at 14px, which is 21 device pixels at 150% scaling: spun on its
 * own, its centre sits on a half pixel and the icon bobs up and down as it
 * turns. 16px is a whole number of device pixels at 100%, 125% and 150%, so
 * the box turns about a pixel-exact centre and the icon with it. An icon-only
 * button passes a negative margin (`-m-px` for a 14px icon, `-m-0.5` for
 * 12px) so the holder takes the icon's own footprint and the button keeps
 * its size.
 */
export function SpinHolder({
  spinning,
  className,
  children,
}: {
  spinning: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <span aria-hidden data-spinning={spinning ? "" : undefined} className={cn("inline-flex size-4 shrink-0 items-center justify-center", spinning && "animate-spin", className)}>
      {children}
    </span>
  );
}
