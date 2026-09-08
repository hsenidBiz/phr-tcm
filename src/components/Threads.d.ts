// Types for the vendored React Bits Threads (JS + CSS variant), matching the
// props of their official TS variant. Keeps the installed .jsx untouched.
import type { HTMLAttributes, ReactElement } from "react";

declare function Threads(
  props: {
    /** Line color as [r, g, b] floats in 0..1. */
    color?: [number, number, number];
    amplitude?: number;
    distance?: number;
    enableMouseInteraction?: boolean;
  } & Omit<HTMLAttributes<HTMLDivElement>, "color">,
): ReactElement;

export default Threads;
