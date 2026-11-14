import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import { commands } from "../bindings";
import { toast } from "../lib/toast";
import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";
import { unwrap } from "../lib/ipc";
import { takeAllProjectsNotice } from "../lib/projectScope";
import { seedCombinedRecents } from "../components/PbiPicker";
import { isCaptureMode } from "../dev/capture";
import type { AppContext } from "./types";

type Args = {
  signedIn: boolean;
  org: string;
  tourOpen: boolean;
  allProjects: boolean;
  homeProject: string;
  /** Whether a project was already chosen when the app launched. */
  hadProject: boolean;
  ctx: MutableRefObject<AppContext>;
  setHomeProjectRaw: Dispatch<SetStateAction<string>>;
  setProject: (p: string) => void;
};

/**
 * The organization's projects, and the home project nobody has to pick.
 * Returns the project list query.
 */
export function useHomeProject({
  signedIn,
  org,
  tourOpen,
  allProjects,
  homeProject,
  hadProject,
  ctx,
  setHomeProjectRaw,
  setProject,
}: Args) {
  // The organization's projects: the same key and options as the context
  // bar's and the palette's, so it is one request. Not over the walkthrough,
  // whose sample scope has its own list.
  const projects = useQuery({
    queryKey: ["projects", org],
    ...persistentQuery({
      key: cacheKeys.projects(org),
      fetcher: () => unwrap(commands.listProjects(org)),
      ...CACHE.reference,
    }),
    enabled: signedIn && Boolean(org) && !tourOpen,
  });

  // A home project nobody has to pick. An organization with exactly one
  // project: that project, in both modes (the selector hides). Projects shown
  // together with several and no home yet: the one project that already has
  // Auto Run files (asked once per organization, and only when exactly one
  // does); otherwise the first PBI picked decides (setPbi).
  const askedAutorunHome = useRef<string | null>(null);
  useEffect(() => {
    if (tourOpen || !signedIn || !org || !projects.data) return;
    const names = projects.data.map((p) => p.name);
    if (names.length === 1) {
      if (homeProject !== names[0]) {
        if (allProjects) setHomeProjectRaw(names[0]);
        else setProject(names[0]);
      }
      return;
    }
    if (!allProjects || homeProject || names.length < 2) return;
    if (askedAutorunHome.current === org) return;
    askedAutorunHome.current = org;
    const askedOrg = org;
    commands
      .projectsWithAutorunFiles(org, names)
      .then((r) => {
        // The answer belongs to the organization it was asked about: one
        // that lands after the person moved on is dropped.
        if (ctx.current.org !== askedOrg) return;
        // An answer that is not exactly one project (or none at all, as
        // from a stand-in) decides nothing. A home chosen meanwhile wins.
        if (r?.status === "ok" && Array.isArray(r.data) && r.data.length === 1) {
          const only = r.data[0];
          setHomeProjectRaw((h) => h || only);
        }
      })
      .catch(() => {});
    // setProject is App's own and changes identity every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tourOpen, signedIn, org, projects.data, homeProject, allProjects]);

  // What a person saved as the home project's recent PBIs is copied to the
  // organization-wide list the first time the projects show together (the
  // per-project list is left exactly as it was).
  useEffect(() => {
    if (tourOpen || !signedIn || !allProjects || !org || !homeProject) return;
    seedCombinedRecents(org, homeProject);
  }, [tourOpen, signedIn, allProjects, org, homeProject]);

  // An existing install is told once that its projects now show together.
  // A fresh install (nothing chosen yet) never is, and neither is one whose
  // organization has a single project: there is nothing to show together, so
  // the notice would only puzzle (its key is still spent). It waits for the
  // project list to know which. Not over a documentation capture, which must
  // never carry a toast.
  //
  // The wording says only what holds from here on: later releases widen what
  // shows together, and this notice is never shown again to correct it.
  const hadProjectRef = useRef(hadProject);
  useEffect(() => {
    if (tourOpen || !signedIn || isCaptureMode() || !projects.data) return;
    if (takeAllProjectsNotice(hadProjectRef.current) && projects.data.length > 1) {
      toast.info("Projects now show together", {
        description:
          "PBIs from every project now show in one search, and each PBI brings its own project, so the Project picker is gone. Auto Run, API Templates and Playwright stay on your home project, which you can change in Settings. To work in one project at a time, turn off Treat all projects as one in Settings, under General.",
        duration: 12_000,
      });
    }
  }, [tourOpen, signedIn, projects.data]);

  return projects;
}
