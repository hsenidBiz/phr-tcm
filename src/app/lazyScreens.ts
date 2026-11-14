import { lazy, type ComponentType } from "react";
import type { PbiHit } from "../bindings";

// Every screen and the command palette load on demand: on a slow machine the
// whole startup bundle is parsed before anything is drawn, and most of it is
// screens nobody has opened yet. The tour waits for its anchors, so a screen
// arriving a moment after the tab switch is fine there too.
//
// The sign-in screen too, with its animation libraries: a kept session never
// shows it. App renders it from its first render, behind the loading screen,
// so its code is on the way while the session check runs, and a signed-out
// launch keeps the loading screen up until it has arrived (see `authKnown`).
// One shared promise, so that wait and React's own load are the same load.
let signInLoad: Promise<typeof import("../screens/SignIn")> | null = null;
export const loadSignIn = () => (signInLoad ??= import("../screens/SignIn"));
export const SignIn = lazy(loadSignIn);
// Shown only after an update, so it loads with the entries it lists.
export const ChangelogModal = lazy(() => import("../components/ChangelogModal"));
export const EditCases = lazy(() => import("../screens/EditCases"));
export const ImportFile = lazy(() => import("../screens/ImportFile"));
export const ManualEntry = lazy(() => import("../screens/ManualEntry"));
export const CreateWorkItem = lazy(() => import("../screens/CreateWorkItem"));
export const PrPanel = lazy(() => import("../screens/PrPanel"));
export const RunTests = lazy(() => import("../screens/RunTests"));
export const ViewCases = lazy(() => import("../screens/ViewCases"));
export const CommandPalette = lazy(() => import("../components/CommandPalette"));
/** Compile-time dev gate: statically false in `tauri build`, so everything
 * behind it (and the dev/ module itself) is dead-code-eliminated from
 * released builds. Test mode opts out so vitest sees the plain app. */
export const DEV_TOOLS = import.meta.env.DEV && import.meta.env.MODE !== "test";

/** DevPanel must be a DYNAMIC import behind the gate: a static
 * `import DevPanel from "../dev/DevPanel"` bundles dev/demo's module-level
 * dataset into the release even though the render below is gated (the JSX
 * gets shaken, module side effects don't). Lazy inside the statically-false
 * branch drops the whole dev/ chunk from `tauri build`. */
export const DevPanel: ComponentType<{
  org: string;
  project: string;
  pbi: PbiHit | null;
  section: string;
  workMode: boolean;
  onShowSignIn: () => void;
}> = DEV_TOOLS ? lazy(() => import("../dev/DevPanel")) : () => null;
export const AiBridge = lazy(() => import("../screens/AiBridge"));
// Shown in development builds, and in a release build whose optional
// extras are unlocked (lib/extras); lazy, so nobody who never opens it
// loads it.
export const AutoRun = lazy(() => import("../screens/AutoRun"));
// Offered exactly where Auto Run is, and lazy for the same reason.
export const ApiTemplates = lazy(() => import("../screens/ApiTemplates"));
// The team's Playwright repo, offered exactly where Auto Run is.
export const Playwright = lazy(() => import("../screens/Playwright"));
// The report an assistant is working on, previewed; Rust refuses its commands while the module is off.
export const CrystalReports = lazy(() => import("../screens/CrystalReports"));
export const Settings = lazy(() => import("../screens/Settings"));
export const Suites = lazy(() => import("../screens/Suites"));
export const ManageCases = lazy(() => import("../screens/ManageCases"));
export const WorkBoard = lazy(() => import("../screens/WorkBoard"));
// The unattended run's window, with the step text renderer it shows cases
// in (lib/markdown and its sanitiser): loaded the first time a run's window
// opens, not at every launch. `RunWindowHost` below keeps it unrendered -
// and so unloaded - until then.
export const RunWindow = lazy(() => import("../screens/AutoRun/ReplayPane"));
