import { useMemo, useState } from "react";
import type { PbiHit } from "../bindings";
import { scopeOf, type SuiteScope } from "../lib/scope";
import { useAppSimpleMode } from "../lib/appSimpleMode";
import BridgeStatusBadge from "../components/BridgeStatusBadge";
import SpecsStatusPill from "../components/SpecsStatusPill";
import type { Section } from "../components/Sidebar";
import { askForFixturesView } from "../lib/apiTemplates";
import { sectionShown, type Module } from "../lib/modules";
import { saveAllProjects } from "../lib/projectScope";
import type { RunFolderTarget } from "../screens/RunTests";
import {
  AiBridge,
  ApiTemplates,
  AutoRun,
  CrystalReports,
  EditCases,
  ImportFile,
  ManageCases,
  ManualEntry,
  Playwright,
  RunTests,
  Settings,
  Suites,
  ViewCases,
} from "./lazyScreens";
import { TITLES, TITLE_NOTES } from "./titles";
import AutoRunModeSwitch from "../screens/AutoRun/ModeSwitch";
import AutoRunSettingsDialog from "../screens/AutoRun/AutoRunSettingsDialog";
import { Button } from "../components/ui/button";
import { IconSettings } from "../lib/actionIcons";
import type { CaseSelection } from "./types";

type ManageFocus = { planId: number; suiteId: number; project: string };

type Props = {
  section: Section;
  org: string;
  project: string;
  homeProject: string;
  allProjects: boolean;
  pbi: PbiHit | null;
  suite: SuiteScope | null;
  modules: ReadonlySet<Module>;
  caseSelection: CaseSelection | null;
  runFolder: RunFolderTarget | null;
  manageFocus: ManageFocus | null;
  settingsFill: boolean;
  startOnModules: boolean;
  setPbi: (p: PbiHit | null) => void;
  setSuite: (s: SuiteScope | null) => void;
  setCaseSelection: (s: CaseSelection | null) => void;
  setRunFolder: (f: RunFolderTarget | null) => void;
  setManageFocus: (f: ManageFocus | null) => void;
  setSection: (s: Section) => void;
  setWorkMode: (on: boolean) => void;
  setSettingsFill: (fill: boolean) => void;
  setStartOnModules: (on: boolean) => void;
  goToSection: (s: Section) => void;
  changeHomeProject: (p: string) => void;
};

/** The Test Case Manager tabs: the heading and whichever screen `section` names. */
export default function CaseScreens({
  section,
  org,
  project,
  homeProject,
  allProjects,
  pbi,
  suite,
  modules,
  caseSelection,
  runFolder,
  manageFocus,
  settingsFill,
  startOnModules,
  setPbi,
  setSuite,
  setCaseSelection,
  setRunFolder,
  setManageFocus,
  setSection,
  setWorkMode,
  setSettingsFill,
  setStartOnModules,
  goToSection,
  changeHomeProject,
}: Props) {
  const simple = useAppSimpleMode().on;
  const [autoRunSettingsOpen, setAutoRunSettingsOpen] = useState(false);
  // One object per pick, so Auto Test's effects keyed on it do not re-run on every App render.
  const scope = useMemo(() => scopeOf(pbi, suite), [pbi, suite]);
  return (
    // key={section} remounts the wrapper on tab switch, so every
    // screen fades up briefly (120ms) instead of snapping in.
    // 120ms fade chosen for snappiness (user request 2026-08-22).
    <div key={section} className={
      section === "playwright" || section === "crystal-reports" || (section === "settings" && settingsFill)
        ? "t-screen-in flex min-h-0 flex-1 flex-col"
        : "t-screen-in"
    }>
      <div className="mb-2 flex items-center gap-2">
        <h1 className="text-lg font-semibold">{TITLES[section]}</h1>
        {section === "ai" && <BridgeStatusBadge />}
        {section === "playwright" && <SpecsStatusPill />}
        {/* Auto Test's Basic/Advanced switch rides in the title row, at the right.
            App Basic mode holds Auto Test in Basic, so the switch says nothing there. */}
        {/* The Settings button shows in Basic mode too; only the switch is left out of it. */}
        {section === "autorun" && modules.has("auto_run") && (
          <div className="ml-auto flex items-center gap-2">
            <Button size="sm" variant="outline" onClick={() => setAutoRunSettingsOpen(true)}>
              <IconSettings aria-hidden />
              Settings
            </Button>
            {!simple && <AutoRunModeSwitch />}
          </div>
        )}
        {autoRunSettingsOpen && <AutoRunSettingsDialog onClose={() => setAutoRunSettingsOpen(false)} />}
        {TITLE_NOTES[section] && (
          <span className="rounded-full bg-warning/15 px-2 py-0.5 text-[11px] font-medium text-warning">
            <span className="label-trim">{TITLE_NOTES[section]}</span>
          </span>
        )}
      </div>
      {section === "manual" && (
        <ManualEntry org={org} project={project} pbi={pbi} onPickPbi={setPbi} />
      )}
      {section === "import" && (
        <ImportFile org={org} project={project} pbi={pbi} onPickPbi={setPbi} />
      )}
      {section === "edit" && (
        <EditCases
          org={org}
          // Cases handed over from Search Suites are edited in the
          // project of the plan they came from.
          project={caseSelection?.project ?? project}
          pbi={pbi}
          caseSelection={caseSelection}
          onClearSelection={() => setCaseSelection(null)}
          onPickPbi={setPbi}
        />
      )}
      {section === "view" && (
        <ViewCases org={org} project={project} pbi={pbi} onPickPbi={setPbi} />
      )}
      {section === "run" && (
        <RunTests
          org={org}
          project={project}
          pbi={pbi}
          folder={runFolder}
          onPickPbi={setPbi}
          onCloseFolder={() => {
            // Back to the PBI's Run Tests, or to Test plans with
            // no PBI to go back to.
            if (pbi) setRunFolder(null);
            else goToSection("suites");
          }}
        />
      )}
      {section === "suites" && (
        <Suites
          org={org}
          project={project}
          allProjects={allProjects}
          onOpenPbi={(p, target) => {
            setPbi({ id: p.id, title: p.title, work_item_type: "Product Backlog Item", project: p.project || project });
            goToSection(target === "edit" ? "edit" : "run");
          }}
          onEditCases={(label, caseIds, casesProject) => {
            setCaseSelection({ label, caseIds, project: casesProject || project });
            setSection("edit");
            setWorkMode(false);
          }}
          onRunFolder={(f) => {
            goToSection("run");
            setRunFolder({ ...f, project: f.project || project });
          }}
          // Only where Auto Test is on: the menu item has nowhere to go otherwise.
          onAutoTest={
            modules.has("auto_run")
              ? (s) => {
                  setSuite({ planId: s.planId, suiteId: s.suiteId, title: s.title, planName: s.planName, project: s.project || project });
                  goToSection("autorun");
                }
              : undefined
          }
          onManageSuite={(planId, suiteId, planProject) => {
            setManageFocus({ planId, suiteId, project: planProject || project });
            goToSection("manage");
          }}
          onSetCurrentPbi={(p) =>
            setPbi({ id: p.id, title: p.title, work_item_type: "Product Backlog Item", project: p.project || project })
          }
        />
      )}
      {modules.has("auto_run") && section === "autorun" && (
        <AutoRun
          org={org}
          // Auto Test's files belong to the application under test
          // (the home project); only the results sent to Azure
          // DevOps follow the PBI.
          project={allProjects ? homeProject : project}
          adoProject={project}
          scope={scope}
          onOpenPlaywright={() => goToSection("playwright")}
          onOpenFixtures={
            sectionShown("apitemplates", modules, simple)
              ? () => {
                  askForFixturesView();
                  goToSection("apitemplates");
                }
              : undefined
          }
          onOpenModules={
            simple
              ? undefined
              : () => {
                  goToSection("settings");
                  setStartOnModules(true);
                }
          }
        />
      )}
      {section === "manage" && (
        <ManageCases
          org={org}
          project={project}
          allProjects={allProjects}
          pbi={pbi}
          focus={manageFocus}
          onFocusHandled={() => setManageFocus(null)}
        />
      )}
      {section === "ai" && <AiBridge />}
      {modules.has("api_templates") && section === "apitemplates" && (
        <ApiTemplates
          org={org}
          project={allProjects ? homeProject : project}
          onOpenAiBridge={() => goToSection("ai")}
        />
      )}
      {modules.has("playwright") && section === "playwright" && (
        <Playwright org={org} project={allProjects ? homeProject : project} />
      )}
      {modules.has("crystal_reports") && section === "crystal-reports" && <CrystalReports />}
      {section === "settings" && (
        <Settings
          org={org}
          project={project}
          homeProject={homeProject}
          onHomeProjectChange={changeHomeProject}
          onAllProjectsChange={saveAllProjects}
          onFillChange={setSettingsFill}
          startOnModules={startOnModules}
        />
      )}
    </div>
  );
}
