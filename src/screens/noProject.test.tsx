// What a screen says when there is no project to work in. Shown together
// there is no Project picker to point at, so the sentence names the ways out:
// a PBI in the bar, or a Home project in Settings. Switched off it is the
// picker's sentence, as it always was. (Auto Run's Setup rows are pinned in
// AutoRun/index.test.tsx.)

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import type { ReactElement } from "react";
import ApiTemplates from "./ApiTemplates";
import CreateWorkItem, { clearWorkItemDraft } from "./CreateWorkItem";
import ManageCases from "./ManageCases";
import Suites from "./Suites";
import WorkBoard from "./WorkBoard";
import { saveAllProjects } from "../lib/projectScope";

afterEach(() => {
  clearMocks();
  localStorage.clear();
  clearWorkItemDraft();
});

function mount(ui: ReactElement) {
  mockIPC((cmd) => {
    if (cmd === "plugin:event|listen") return 1;
    if (cmd === "plugin:event|unlisten") return null;
    return undefined;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={qc}>{ui}</QueryClientProvider>);
}

const COMBINED = "Pick a PBI in the bar above, or a Home project in Settings, ";
const OFF = "Pick an organization and project in the bar above ";

const SCREENS: Array<{ name: string; ui: (allProjects: boolean) => ReactElement; tail: string }> = [
  {
    name: "New Work Item",
    ui: (allProjects) => <CreateWorkItem org="acme" project="" allProjects={allProjects} />,
    tail: "first.",
  },
  { name: "Search Suites", ui: () => <Suites org="acme" project="" />, tail: "to browse test suites." },
  {
    name: "Suite Management",
    ui: () => <ManageCases org="acme" project="" pbi={null} />,
    tail: "to manage test cases.",
  },
  {
    name: "API Templates",
    ui: () => <ApiTemplates org="acme" project="" onOpenAiBridge={() => {}} />,
    tail: "first.",
  },
];

// The board is the exception while the projects show together: it reads every
// project, so only an organization is missing. (The combined board itself is
// covered in WorkBoard.test.tsx.)
test("Work Manager with no organization asks for one while the projects show together", () => {
  mount(<WorkBoard org="" project="" allProjects homeProject="" />);
  expect(screen.getByText("Pick an organization in the bar above first.")).toBeInTheDocument();
});

test("Work Manager with no project points at the Project picker once switched off", () => {
  saveAllProjects(false);
  mount(<WorkBoard org="acme" project="" allProjects={false} homeProject="" />);
  expect(screen.getByText(`${OFF}first.`)).toBeInTheDocument();
});

for (const s of SCREENS) {
  test(`${s.name} with no project points at a PBI or the Home project while the projects show together`, () => {
    mount(s.ui(true));
    expect(screen.getByText(`${COMBINED}${s.tail}`)).toBeInTheDocument();
  });

  test(`${s.name} with no project points at the Project picker once switched off`, () => {
    saveAllProjects(false);
    mount(s.ui(false));
    expect(screen.getByText(`${OFF}${s.tail}`)).toBeInTheDocument();
  });
}
