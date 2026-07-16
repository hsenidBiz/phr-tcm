// Types for the vendored React Bits Aurora (JS + CSS variant), matching the
// props of their official TS variant. Keeps the installed .jsx untouched.
import type { ReactElement } from "react";

declare function Aurora(props: {
  colorStops?: string[];
  amplitude?: number;
  blend?: number;
  speed?: number;
  time?: number;
}): ReactElement;

export default Aurora;
