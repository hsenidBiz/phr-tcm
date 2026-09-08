// Types for the vendored React Bits ElectricBorder (JS + CSS variant),
// matching the props of their official TS variant. Keeps the installed
// .jsx untouched.
import type { CSSProperties, ReactElement, ReactNode } from "react";

declare function ElectricBorder(props: {
  children?: ReactNode;
  color?: string;
  speed?: number;
  chaos?: number;
  borderRadius?: number;
  className?: string;
  style?: CSSProperties;
}): ReactElement;

export default ElectricBorder;
