import { QueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef, useState, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { PbiHit } from "../bindings";
import { suspendCache } from "../lib/cache";
import { clearTourRepositories, setTourRepositories } from "../lib/workingDir";
import { clearTourExpanded, setTourExpanded } from "../lib/sidebarState";
import type { Section, WorkSection } from "../components/Sidebar";
import { isCaptureMode } from "../dev/capture";
import { installTourBackend, restoreTourBackend } from "../tour/tourBackend";
import { TOUR_ORG, TOUR_PBI, TOUR_PROJECT, TOUR_REPO_PATH } from "../tour/tourData";
import type { TourWhere } from "../tour/tourScript";
import { START_TOUR_EVENT, setTourRunning, tourDone } from "../tour/tourState";
import type { AppContext, CaseSelection } from "./types";

type TourArgs = {
  qc: QueryClient;
  /** Where the app is, read at the moment the tour starts. */
  ctx: MutableRefObject<AppContext>;
  setOrgRaw: Dispatch<SetStateAction<string>>;
  setHomeProjectRaw: Dispatch<SetStateAction<string>>;
  setPbiRaw: Dispatch<SetStateAction<PbiHit | null>>;
  setSection: Dispatch<SetStateAction<Section>>;
  setWorkMode: Dispatch<SetStateAction<boolean>>;
  setWorkSection: Dispatch<SetStateAction<WorkSection>>;
  setCaseSelection: Dispatch<SetStateAction<CaseSelection | null>>;
};

/**
 * The first-run walkthrough's state and its start/end/navigate callbacks.
 * While it is up the app runs on sample data, saves nothing, and cannot be
 * touched. Called ahead of every other effect in App, because several of
 * them gate on `tourOpen` and a hook cannot read a binding declared after
 * itself.
 */
export function useAppTour({
  qc,
  ctx,
  setOrgRaw,
  setHomeProjectRaw,
  setPbiRaw,
  setSection,
  setWorkMode,
  setWorkSection,
  setCaseSelection,
}: TourArgs) {
  const [tourOpen, setTourOpen] = useState(false);
  // A throw-away cache for the toured screens: the sample data never
  // mixes with the real one, and dies with the tour.
  const [tourQc, setTourQc] = useState<QueryClient | null>(null);
  // The destination the current stop is waiting for the user to walk to,
  // reported by the overlay - null while it is not waiting for anything.
  const [tourAwaited, setTourAwaited] = useState<TourWhere | null>(null);
  // ...and whether that stop is one the user is meant to ACT on (the
  // theme). The screens below stop being inert for exactly those stops, so
  // the real control inside the ring can be used; the overlay then
  // swallows clicks everywhere except that ring.
  const [tourActing, setTourActing] = useState(false);
  // Where the user was before the tour took over.
  const before = useRef<AppContext | null>(null);

  const startTour = useCallback(() => {
    if (before.current) return; // already running
    before.current = { ...ctx.current };
    suspendCache(true);
    setTourExpanded(true);
    installTourBackend();
    setTourRepositories([{ path: TOUR_REPO_PATH, enabled: true }], TOUR_REPO_PATH);
    setTourQc(
      new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } } }),
    );
    // The context bar is not remounted by the tour (only the screens
    // below it are, on their `key={section}`), and a mounted query keeps
    // the client it started on - so the org list it is already holding
    // stays the REAL one while everything around it turns to sample data.
    // Northwind is then not in the list and the picker reads blank, which
    // is exactly what the stop that rings it is pointing at. Dropping the
    // entry makes it ask again, and the ask now reaches the stand-in.
    // Dropped again when the tour ends, below, so the sample list does not
    // outlive it.
    qc.removeQueries({ queryKey: ["orgs"] });
    setOrgRaw(TOUR_ORG);
    setHomeProjectRaw(TOUR_PROJECT);
    setPbiRaw(TOUR_PBI);
    setTourRunning(true);
    setTourOpen(true);
  }, [qc]);

  const endTour = useCallback(() => {
    const back = before.current;
    before.current = null;
    setTourOpen(false);
    setTourRunning(false);
    setTourAwaited(null);
    setTourActing(false);
    restoreTourBackend();
    clearTourRepositories();
    clearTourExpanded();
    suspendCache(false);
    setTourQc(null);
    // The ungated `can-delete` permission query re-keys itself to whatever
    // org/project it is asked about, including the sample one - and it
    // lives on the REAL query client, not the throw-away one, so it would
    // otherwise sit there for the rest of the session. Dropping the whole
    // prefix rather than just the sample key is deliberate: the real
    // entry (if any) simply refetches next time it is needed.
    qc.removeQueries({ queryKey: ["can-delete"] });
    // Same for the org list the context bar was made to re-ask for at the
    // start: it is holding Northwind now, and Northwind is not a place
    // the user can work.
    qc.removeQueries({ queryKey: ["orgs"] });
    if (!back) return;
    setOrgRaw(back.org);
    setHomeProjectRaw(back.homeProject);
    setPbiRaw(back.pbi);
    setSection(back.section);
    setWorkMode(back.workMode);
    setWorkSection(back.workSection);
    setCaseSelection(back.caseSelection);
  }, [qc]);

  // Each stop says where it lives; take the app there.
  const tourNavigate = useCallback((where: TourWhere | undefined) => {
    if (!where) return;
    if (where.area === "cases") {
      setWorkMode(false);
      setCaseSelection(null);
      setSection(where.section);
    } else {
      setWorkMode(true);
      setWorkSection(where.workSection);
    }
  }, []);

  // A tour that is still up when the window goes away must not leave the
  // stand-ins installed for whatever mounts next.
  useEffect(
    () => () => {
      restoreTourBackend();
      clearTourRepositories();
      clearTourExpanded();
      suspendCache(false);
      setTourRunning(false);
    },
    [],
  );

  return {
    tourOpen,
    tourQc,
    tourAwaited,
    setTourAwaited,
    tourActing,
    setTourActing,
    startTour,
    endTour,
    tourNavigate,
  };
}

/**
 * The tour's tie to the session: opens once after the first sign-in, again
 * whenever Settings asks for it, and ends the moment the session does.
 */
export function useTourLifecycle(
  signedIn: boolean,
  tourOpen: boolean,
  startTour: () => void,
  endTour: () => void,
) {
  // First-run walkthrough: opens once after the first sign-in, and again
  // whenever Settings asks for it (see useAppTour for why `tourOpen` has to
  // exist before every effect that gates on it).
  useEffect(() => {
    // Capture mode: the guided tour must never auto-open over a shot.
    if (signedIn && !tourDone() && !isCaptureMode()) {
      // Asked again when it fires: a tour opened by hand and skipped inside
      // the 800 ms has marked itself seen, and must not open a second time.
      const t = setTimeout(() => {
        if (!tourDone()) startTour();
      }, 800);
      return () => clearTimeout(t);
    }
  }, [signedIn, startTour]);

  useEffect(() => {
    window.addEventListener(START_TOUR_EVENT, startTour);
    return () => window.removeEventListener(START_TOUR_EVENT, startTour);
  }, [startTour]);

  // A session that ends mid-tour (a dropped connection returning with an
  // expired token, a manual sign-out) must not leave the tour up: `UiTour`
  // only renders while `signedIn` too, so it would simply vanish - taking
  // the Skip button with it, while the shell stayed inert and the
  // stand-ins stayed installed. Ending the tour ourselves the moment
  // `signedIn` goes false hands everything back exactly as every other
  // exit path does.
  useEffect(() => {
    if (tourOpen && !signedIn) endTour();
  }, [tourOpen, signedIn, endTour]);
}
