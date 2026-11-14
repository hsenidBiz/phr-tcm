import { Suspense } from "react";
import { useBackgroundRun } from "../lib/backgroundRun";
import { RunWindow } from "./lazyScreens";

/** Mounts the run window only while a run's window is open, so its code
 * loads on first use. Its own component, so a run's progress re-renders
 * this and not the whole of App. */
export function RunWindowHost() {
  const { run } = useBackgroundRun();
  if (!run?.open) return null;
  return (
    <Suspense fallback={null}>
      <RunWindow />
    </Suspense>
  );
}
