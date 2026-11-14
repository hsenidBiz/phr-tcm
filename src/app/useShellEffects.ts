import { useEffect, type Dispatch, type SetStateAction } from "react";
import type { PbiHit } from "../bindings";
import { applyRateLevel } from "../lib/adoRate";
import { hydrateExtras, subscribeExtras } from "../lib/extras";
import {
  hydrateModules,
  realModulesSnapshot,
  sectionShown,
  shouldLeaveHidden,
  type Module,
} from "../lib/modules";
import { clearNewNav, pruneNewNav } from "../lib/newNav";
import { savePrefs } from "../lib/prefs";
import { initTheme } from "../lib/theme";
import { shortcutOrder, type Section } from "../components/Sidebar";
import { tourRunningSnapshot } from "../tour/tourState";

/** Theme, pacing and the Rust-side switches, set up once at launch. */
export function useStartupEffects() {
  // Also hourly, so a window left open past a season's end still gets
  // its restore without a relaunch.
  useEffect(() => {
    initTheme();
    const id = setInterval(() => initTheme(), 60 * 60 * 1000);
    return () => clearInterval(id);
  }, []);
  // The extras flag arrives from Rust a moment after launch, and can change
  // later: a held-back season follows it.
  useEffect(() => subscribeExtras(() => initTheme()), []);
  // Push the saved ADO pacing into the Rust limiter before anything fetches.
  useEffect(() => applyRateLevel(), []);

  // The hidden Extras switch and the modules both live in Rust; ask once,
  // so the Extras card and every module's screens follow them.
  useEffect(() => {
    void hydrateExtras();
    void hydrateModules();
  }, []);
}

/** Keeps the open tab consistent with which modules are on. */
export function useModuleGuards(
  section: Section,
  modules: ReadonlySet<Module>,
  modulesHydrated: boolean,
  workMode: boolean,
  tourOpen: boolean,
  setSection: Dispatch<SetStateAction<Section>>,
) {
  // If the open tab belongs to a module that is switched off (or gets
  // switched off while it is open), move off it rather than leave its
  // heading over a blank body. Gated on hydration: the set starts empty the
  // same way a fresh install reads, so without the gate a restart with a
  // saved Auto Run tab bounces straight to Manual Entry before
  // hydrateModules' answer comes back.
  useEffect(() => {
    if (shouldLeaveHidden(section, sectionShown(section, modules), modulesHydrated)) setSection("manual");
  }, [modules, section, modulesHydrated]);

  // A sidebar row marked "new" (lib/newNav) is done with once its screen
  // is open, however it was reached: a click, Ctrl+number, the palette, a
  // redirect. Not while the tour drives the tabs - its stops are not the
  // person's own visit.
  useEffect(() => {
    if (tourOpen || workMode || !sectionShown(section, modules)) return;
    clearNewNav(section);
  }, [section, workMode, tourOpen, modules]);

  // Marks only ever come from a person's own switch-on; this only ever
  // takes away the ones whose module is no longer on.
  useEffect(() => {
    if (modulesHydrated) pruneNewNav(realModulesSnapshot());
  }, [modules, modulesHydrated]);
}

/** Ctrl+1..9 = tabs, Ctrl+Shift+M = Work Manager (v1's binding). Ctrl+K
 * (palette) is registered in CommandPalette. */
export function useKeyboardShortcuts(
  setSection: Dispatch<SetStateAction<Section>>,
  setWorkMode: Dispatch<SetStateAction<boolean>>,
) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (tourRunningSnapshot()) return; // the tour drives, not the keyboard
      if (!e.ctrlKey && !e.metaKey) return;
      if (e.shiftKey && e.key.toLowerCase() === "m") {
        e.preventDefault();
        setWorkMode((w) => !w);
        return;
      }
      // Read at the moment of the press: the rows close up around a
      // module's hidden rows, and switching it on can add them while the
      // app runs. The palette's hints read the same list, so they cannot
      // drift.
      const order = shortcutOrder();
      const n = Number(e.key);
      if (n >= 1 && n <= order.length) {
        e.preventDefault();
        setSection(order[n - 1]);
        setWorkMode(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

/** One writer for all prefs so no path forgets to persist. Silent during
 * the tour: the sample scope is not the user's, and must not outlive it. */
export function usePersistPrefs(prefs: {
  org: string;
  homeProject: string;
  section: Section;
  pbi: PbiHit | null;
  workMode: boolean;
  tourOpen: boolean;
}) {
  const { org, homeProject, section, pbi, workMode, tourOpen } = prefs;
  useEffect(() => {
    if (tourOpen) return;
    // The home project, not the effective one: a PBI picked from another
    // project must not move where the person's own files live.
    savePrefs({ org, project: homeProject, section, pbi, workMode });
  }, [org, homeProject, section, pbi, workMode, tourOpen]);
}
