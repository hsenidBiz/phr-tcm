import { useRef, useState, type Dispatch, type SetStateAction } from "react";
import { commands, type PbiHit } from "../bindings";
import type { Section } from "../components/Sidebar";
import { unwrap } from "../lib/ipc";
import { logUi } from "../lib/uiLog";
import type { RunFolderTarget } from "../screens/RunTests";
import type { CaseSelection } from "./types";

type Args = {
  section: Section;
  workMode: boolean;
  tourOpen: boolean;
  org: string;
  project: string;
  allProjects: boolean;
  setSection: Dispatch<SetStateAction<Section>>;
  setWorkMode: Dispatch<SetStateAction<boolean>>;
  setCaseSelection: Dispatch<SetStateAction<CaseSelection | null>>;
  setRunFolder: Dispatch<SetStateAction<RunFolderTarget | null>>;
  setProject: (p: string) => void;
  setPbi: (p: PbiHit | null) => void;
};

/** Moving between the tabs: the rail, the Settings toggle and a bell notice about a PBI. */
export function useAppNavigation({
  section,
  workMode,
  tourOpen,
  org,
  project,
  allProjects,
  setSection,
  setWorkMode,
  setCaseSelection,
  setRunFolder,
  setProject,
  setPbi,
}: Args) {
  // Settings opens on Modules when the "Playwright is needed" dialog sent
  // the person there; any other navigation clears it.
  const [startOnModules, setStartOnModules] = useState(false);
  const goToSection = (s: Section) => {
    logUi(`nav: ${s}`);
    setSection(s);
    setStartOnModules(false);
    setWorkMode(false); // any tab click exits Work Manager mode
    setCaseSelection(null); // direct navigation returns Edit to PBI mode
    setRunFolder(null); // ...and Run Tests to the PBI
  };
  // A bell notice about a PBI: open it, then land on `then`. The hit comes
  // from the search so the title is the real one. Projects shown together:
  // the PBI's own project decides where every later write goes, so the id is
  // looked up across the organization and the PBI opens only under the
  // project the hit names; no hit, no switch. One project at a time, a PBI
  // the search does not return still opens under a plain title in the
  // project that was searched. The values read after the search are the
  // current ones: if the organization, the project or the scope moved while
  // it ran, the notice is no longer what the person is looking at.
  const latestScope = useRef({ org, project, allProjects });
  latestScope.current = { org, project, allProjects };
  const openPbiById = async (id: number, pbiProject: string, then: Section) => {
    const started = latestScope.current;
    let hit: PbiHit | null = null;
    try {
      if (started.allProjects) {
        const found = await unwrap(commands.searchPbisAll(started.org, String(id)));
        const f = found.find((h) => h.id === id);
        hit = f?.project ? f : null;
      } else {
        const found = await unwrap(commands.searchPbis(started.org, pbiProject, String(id)));
        const f = found.find((h) => h.id === id);
        hit = f
          ? { ...f, project: f.project || pbiProject }
          : { id, title: `PBI ${id}`, work_item_type: "Product Backlog Item", project: pbiProject };
      }
    } catch (e) {
      logUi(`nav: PBI ${id} search failed: ${String(e)}`);
      if (!started.allProjects) {
        hit = { id, title: `PBI ${id}`, work_item_type: "Product Backlog Item", project: pbiProject };
      }
    }
    const now = latestScope.current;
    if (now.org !== started.org || now.allProjects !== started.allProjects) {
      logUi(`nav: PBI ${id} not opened, the organization or project scope changed meanwhile`);
      return;
    }
    if (!hit) {
      logUi(`nav: PBI ${id} not opened, no project found for it`);
      return;
    }
    if (!now.allProjects && hit.project && hit.project !== now.project) setProject(hit.project);
    setPbi(hit);
    goToSection(then);
  };

  // Settings acts as a toggle: opening it remembers where you were (tab or
  // Work Manager); clicking the gear again returns you there.
  const beforeSettings = useRef<{ section: Section; workMode: boolean } | null>(null);
  const toggleSettings = () => {
    if (section === "settings" && !workMode) {
      const back = beforeSettings.current;
      beforeSettings.current = null;
      setSection(back?.section ?? "manual");
      setWorkMode(back?.workMode ?? false);
      setCaseSelection(null);
      return;
    }
    // Not while the tour is up: its Settings stops ask for this same
    // click, and recording the sample tab it happened to be on would
    // overwrite where the user's OWN visit to Settings goes back to. The
    // tour puts the app back where it found it when it ends, so the
    // earlier answer is still the right one.
    if (!tourOpen) beforeSettings.current = { section, workMode };
    goToSection("settings");
  };

  return { startOnModules, setStartOnModules, goToSection, openPbiById, toggleSettings };
}
