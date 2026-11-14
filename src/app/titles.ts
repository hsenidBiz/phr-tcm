import type { Section } from "../components/Sidebar";

export const TITLES: Record<Section, string> = {
  manual: "Manual Entry",
  import: "Import Test Cases",
  edit: "Update Test Cases",
  view: "View Test Cases",
  run: "Run Tests",
  autorun: "Auto Run",
  suites: "Search Suites",
  manage: "Suite Management",
  ai: "AI Bridge",
  apitemplates: "API Templates",
  playwright: "Playwright",
  "crystal-reports": "Crystal Reports",
  settings: "Settings",
};

/** Status pill beside the heading - features shipped before they are done. */
export const TITLE_NOTES: Partial<Record<Section, string>> = {};
