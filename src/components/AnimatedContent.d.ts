// Types for the vendored React Bits AnimatedContent (JS variant), matching
// the props of their official TS variant. Keeps the installed .jsx untouched.
import type { HTMLAttributes, ReactElement, ReactNode } from "react";

declare function AnimatedContent(
  props: {
    children?: ReactNode;
    container?: Element | string | null;
    distance?: number;
    direction?: "vertical" | "horizontal";
    reverse?: boolean;
    duration?: number;
    ease?: string;
    initialOpacity?: number;
    animateOpacity?: boolean;
    scale?: number;
    threshold?: number;
    delay?: number;
    disappearAfter?: number;
    disappearDuration?: number;
    disappearEase?: string;
    onComplete?: () => void;
    onDisappearanceComplete?: () => void;
    className?: string;
  } & HTMLAttributes<HTMLDivElement>,
): ReactElement;

export default AnimatedContent;
