import { useEffect, useRef } from "react";
import { commands, events } from "../bindings";
import { toast } from "../lib/toast";
import { fileName, loadWatches, saveWatches, upsertWatch } from "../lib/fileSync";
import { saveNote } from "../lib/caseNotes";
import { addWorkAlerts } from "../lib/workAlerts";
import { noteAssigned, noteInterruptedUploads, noteWaiting } from "../lib/notifications";
import { submitPhaseSnapshot } from "../lib/submitRun";
import { announce, summarize } from "../lib/assignedAlerts";
import { useWaitingItems } from "../lib/useWaitingItems";
import { logUi } from "../lib/uiLog";
import { isCaptureMode } from "../dev/capture";
import { detach } from "../lib/detach";

/**
 * Floor between two slowdown toasts. Azure DevOps can ask the app to slow
 * down repeatedly over one long import - `note_server_delay` on the Rust
 * side only re-emits for a hold that is new or longer, but that promise
 * does not reach the wire, so the frontend debounces on its own rather
 * than trust it. One every five minutes is plenty to make the user aware
 * without burying them.
 */
const SLOWDOWN_TOAST_MIN_GAP_MS = 5 * 60 * 1000;

/** Events Rust raises that need no screen to be open: saved report notes
 * and the Azure DevOps slowdown notice. */
export function useGlobalBackendEvents() {
  // Comments typed in the "View in browser" report autosave here over the
  // loopback note listener - persist them into the local notes store.
  useEffect(() => {
    const un = events.caseNoteSaved.listen((e) =>
      saveNote(e.payload.org, e.payload.case_id, e.payload.text),
    );
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  // Azure DevOps throttles per USER, not per app - so once the account
  // trips the limit, ADO can be slowing down the person's browser tabs and
  // git operations too, with nothing telling them why. When the Rust pacer
  // starts holding requests for that reason, say so here rather than leave
  // it in the log panel nobody watches, and point at the one control that
  // can hand some of that speed back.
  //
  // `lastSlowdownToastAt` is local to this one listener - nothing outside
  // this effect needs it - so a ref (the same shape as `shownChangelogRef`
  // further down) is simpler than a module-level store like
  // sessionExpired.ts.
  const lastSlowdownToastAt = useRef(0);
  useEffect(() => {
    const un = events.slowdownRequested.listen((e) => {
      const now = Date.now();
      if (now - lastSlowdownToastAt.current < SLOWDOWN_TOAST_MIN_GAP_MS) return;
      lastSlowdownToastAt.current = now;
      toast.info("Azure DevOps asked this app to slow down", {
        description: `Requests are paused for ${e.payload.secs}s. If you're also working in Azure DevOps in your browser, the "Azure DevOps request rate" setting in Settings can hand back some speed.`,
        duration: 8_000,
      });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);
}

/**
 * Uploads that died before they finished (the app closed, the machine
 * slept) left a journal behind; each raises one bell notice that opens
 * its PBI. Once per account and organisation per session; an upload still
 * running right now is not interrupted and is skipped.
 */
export function useInterruptedUploadCheck(
  signedIn: boolean,
  org: string,
  tourOpen: boolean,
  journalAccount: string,
) {
  const journalsChecked = useRef<string | null>(null);
  useEffect(() => {
    if (!signedIn || !org || tourOpen || isCaptureMode()) return;
    const checkKey = `${journalAccount}|${org}`;
    if (journalsChecked.current === checkKey) return;
    journalsChecked.current = checkKey;
    void (async () => {
      const r = await commands.uploadJournalList(org);
      if (r.status === "error") {
        logUi(`upload journals: could not list (${r.error})`);
        return;
      }
      const running = submitPhaseSnapshot();
      noteInterruptedUploads(
        org,
        r.data.filter((j) => !(running && running.org === org && running.pbiId === j.pbi_id)),
      );
    })().catch((e) => logUi(`upload journals: could not list (${String(e)})`));
  }, [signedIn, org, tourOpen, journalAccount]);
}

/**
 * What the assistants left for a person to decide, for the Auto Run rail's
 * count and the bell. Auto Run's files belong to the home project, so that
 * is the project asked about while the projects show together.
 */
export function useWaitingNotices(org: string, autoRunProject: string, enabled: boolean, tourOpen: boolean) {
  const waiting = useWaitingItems(org, autoRunProject, enabled);
  useEffect(() => {
    if (!waiting || !org || tourOpen || isCaptureMode()) return;
    noteWaiting(org, autoRunProject, waiting);
  }, [waiting, org, autoRunProject, tourOpen]);
  return waiting;
}

/**
 * Background check for work items newly assigned to you. Rust polls and
 * emits; the choice of toast vs Windows notification is made here,
 * because "can the user see the app" is a frontend question.
 */
export function useAssignedWork(
  signedIn: boolean,
  org: string,
  bridgeProject: string,
  allProjects: boolean,
  tourOpen: boolean,
) {
  useEffect(() => {
    if (tourOpen) return;
    if (!signedIn || !org || !bridgeProject) return;
    // Together, one watch over every project seeded from the home project,
    // so it does not restart with every PBI picked; apart, the picked project.
    (allProjects
      ? commands.watchAssignedWorkAll(org, bridgeProject)
      : commands.watchAssignedWork(org, bridgeProject)
    ).catch(() => {});
  }, [signedIn, org, bridgeProject, allProjects, tourOpen]);

  useEffect(() => {
    const un = events.workAssigned.listen((e) => {
      const items = e.payload.items;
      if (items.length === 0) return;
      // Into the bell, where it stays until dismissed. Each item names its
      // own project; the home or picked one only fills in for one that names
      // none. The bell skips what it has raised before, which is what happens
      // when the all-projects switch is flipped and the other scope reports
      // an item again: only what it took in is announced.
      const added = new Set(noteAssigned(org, bridgeProject, items).map((n) => n.id));
      const news = items.filter((i) => added.has(`assigned:${i.id}`));
      if (news.length === 0) return;
      // A toast is gone in seconds - the Board badge is what remains
      // until the user actually looks at the board.
      addWorkAlerts(news.length);
      const { title, body } = summarize(news);
      announce(title, body);
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [org, bridgeProject]);
}

/**
 * `begin_test_case_writing` ends with the developer having said where
 * the finished JSON goes. Register that path as a watched file NOW, so
 * the assistant's first write folds into the queue on its own - the
 * second and later writes already did, and needing to import the first
 * one by hand was the only manual step left in the loop.
 *
 * Registration lives here rather than in ImportFile because only the
 * active screen is mounted: intake usually finishes while the developer
 * is on the AI Bridge tab, and a listener inside Import would never see
 * the event. ImportFile arms the OS watcher and checks the file once on
 * mount, so a file that lands while you are elsewhere is picked up when
 * you open the tab.
 */
export function useIntakeWatch(signedIn: boolean, org: string, pbiId: number | undefined, tourOpen: boolean) {
  useEffect(() => {
    // Otherwise this would drop the user's real subscription for the
    // tour's duration and re-subscribe as the sample org/PBI - and an
    // event arriving while that stand-in subscription is live would call
    // `saveWatches` with the sample scope. "No write" is the invariant,
    // not "no Azure DevOps call", so this is gated the same as its four
    // siblings even though it never reaches ADO itself.
    if (tourOpen) return;
    if (!signedIn || !org || !pbiId) return;
    let unlisten: (() => void) | undefined;
    let live = true;
    void events.intakeOutputPath
      .listen((e) => {
        const path = e.payload.path?.trim();
        if (!path) return;
        const list = loadWatches(org, pbiId);
        // Re-running begin with the same answers must not reset a file
        // already being followed - that would drop its snapshot and make
        // the next edit read as though the whole file were new.
        if (list.some((w) => w.path === path)) return;
        saveWatches(org, pbiId, upsertWatch(list, { path, stamp: "", snapshot: [] }));
        toast.info(`Watching ${fileName(path)} - it will import itself when written.`);
      })
      .then((f) => {
        if (live) unlisten = f;
        else detach(f);
      });
    return () => {
      live = false;
      detach(unlisten);
    };
  }, [signedIn, org, pbiId, tourOpen]);
}
