// Types for the vendored React Bits ShinyText (JS + CSS variant), matching
// the props of their official TS variant. Keeps the installed .jsx untouched.
import type { ReactElement } from "react";

declare function ShinyText(props: {
  text: string;
  disabled?: boolean;
  speed?: number;
  className?: string;
  color?: string;
  shineColor?: string;
  spread?: number;
  yoyo?: boolean;
  pauseOnHover?: boolean;
  direction?: "left" | "right";
  delay?: number;
}): ReactElement;

export default ShinyText;
