import type { PbiHit } from "../bindings";
import type { Section, WorkSection } from "../components/Sidebar";

/** Suite-browser handoff: edit an arbitrary set of cases (not persisted).
 * The cases belong to their plan's project, which may not be the effective
 * one: Update Test Cases deletes, edits and tags them there. */
export type CaseSelection = {
  label: string;
  caseIds: number[];
  project: string;
};

/** Where the app is, read inside stable callbacks (see App). */
export type AppContext = {
  section: Section;
  org: string;
  homeProject: string;
  pbi: PbiHit | null;
  workMode: boolean;
  workSection: WorkSection;
  caseSelection: CaseSelection | null;
};
