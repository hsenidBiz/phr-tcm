// Types for the vendored React Bits SplitText (JS + CSS variant), matching
// the props of their official TS variant. Keeps the installed .jsx untouched.
import type { ReactElement } from "react";

declare function SplitText(props: {
  text: string;
  className?: string;
  delay?: number;
  duration?: number;
  ease?: string;
  splitType?: "chars" | "words" | "lines" | "words, chars";
  from?: gsap.TweenVars;
  to?: gsap.TweenVars;
  threshold?: number;
  rootMargin?: string;
  textAlign?: string;
  tag?: string;
  onLetterAnimationComplete?: () => void;
}): ReactElement;

export default SplitText;
