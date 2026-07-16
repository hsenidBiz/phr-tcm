// Types for the vendored React Bits CountUp (JS variant), matching the props
// of their official TS variant. Keeps the installed .jsx untouched.
import type { ReactElement } from "react";

declare function CountUp(props: {
  to: number;
  from?: number;
  direction?: "up" | "down";
  delay?: number;
  duration?: number;
  className?: string;
  startWhen?: boolean;
  separator?: string;
  onStart?: () => void;
  onEnd?: () => void;
}): ReactElement;

export default CountUp;
