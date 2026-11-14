import type { WorkSection } from "../components/Sidebar";
import type { NotificationTarget } from "../lib/notifications";
import { CreateWorkItem, PrPanel, WorkBoard } from "./lazyScreens";

type Props = {
  workSection: WorkSection;
  org: string;
  project: string;
  homeProject: string;
  allProjects: boolean;
  workFocus: NotificationTarget | null;
  onFocusHandled: () => void;
};

/** Work Manager: the board, New Work Item or Pull Requests. */
export default function WorkScreens({
  workSection,
  org,
  project,
  homeProject,
  allProjects,
  workFocus,
  onFocusHandled,
}: Props) {
  return (
    // Same fade-up as the Test Case Manager tabs: key remounts
    // on section switch; the flex classes keep the board's height
    // chain intact (the wrapper sits inside a flex-col main).
    // 120ms fade chosen for snappiness (user request 2026-08-22).
    // `t-screen-in` lives in index.css.
    <div key={workSection} className="t-screen-in flex min-h-0 flex-1 flex-col">
      {workSection === "board" ? (
        <>
          <h1 className="mb-4 text-lg font-semibold">Board</h1>
          <div className="min-h-0 flex-1">
            <WorkBoard
              org={org}
              project={project}
              allProjects={allProjects}
              homeProject={homeProject}
              focusItem={workFocus?.kind === "work-item" ? workFocus.id : null}
              focusProject={workFocus?.kind === "work-item" ? workFocus.project : null}
              onFocusHandled={onFocusHandled}
            />
          </div>
        </>
      ) : workSection === "create" ? (
        <>
          <h1 className="mb-4 text-lg font-semibold">New Work Item</h1>
          <div className="min-h-0 flex-1 overflow-y-auto">
            {/* An item with no parent belongs to the home project; a
                parent from another project moves it there (the
                screen decides). */}
            <CreateWorkItem
              org={org}
              project={allProjects ? homeProject : project}
              allProjects={allProjects}
            />
          </div>
        </>
      ) : (
        <>
          <h1 className="mb-4 text-lg font-semibold">Pull Requests</h1>
          <div className="min-h-0 flex-1 overflow-y-auto">
            {/* Together the panel lists every project and the
                home project only fills in for what names none; a
                notification names its project in the focus. */}
            <PrPanel
              org={org}
              project={allProjects ? homeProject : project}
              allProjects={allProjects}
              focus={workFocus?.kind === "pr" ? workFocus : null}
              onFocusHandled={onFocusHandled}
            />
          </div>
        </>
      )}
    </div>
  );
}
