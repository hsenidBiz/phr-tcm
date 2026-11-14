import { useMutation } from "@tanstack/react-query";
import { useEffect, useState, type Dispatch, type SetStateAction } from "react";
import { commands, events, type TestCase } from "../../bindings";
import { toast } from "../../lib/toast";
import { fileName, keysFor, ownerPaths, type WatchedFile } from "../../lib/fileSync";
import { copyText } from "../../lib/clipboard";
import { pagePalette } from "../../lib/reportTheme";

/** Share-for-review, the browser report, and keeping an open report in step with the queue. */
export function useQueueReport({
  org,
  project,
  pbiId,
  queue,
  setQueue,
  watches,
  areaGrouped,
}: {
  org: string;
  project: string;
  pbiId: number;
  queue: TestCase[];
  setQueue: Dispatch<SetStateAction<TestCase[]>>;
  watches: WatchedFile[];
  areaGrouped: boolean;
}) {
  // Whether the browser report has been opened this session - see the
  // re-export effect below.
  const [reportOpen, setReportOpen] = useState(false);

  // Share-for-review: the draft travels through ADO as a PBI attachment
  // (one-time-use link; nothing is created in ADO). The link lands on the
  // clipboard, ready for Teams.
  const share = useMutation({
    mutationFn: async () => {
      const r = await commands.shareQueue(org, project, pbiId, queue);
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: (link) => {
      copyText(link)
        .then(() => toast.success("Share link copied - send it to your reviewer."))
        .catch(() => toast.success(`Share link ready: ${link}`));
    },
    onError: (e) => toast.error(`Could not share: ${e.message}`),
  });

  // v1's "View": render to a temp file and open the browser - no download.
  //
  // The draft page, not the one used for cases that already exist in Azure
  // DevOps: here every case gets a comment box (a draft has no work item id
  // to key an app-side note by), the text is the case's own `comment` -
  // the same field the card below edits - and it is written back into the
  // JSON file the case came from.
  const viewHtml = useMutation({
    mutationFn: async () => {
      const r = await commands.viewDraftHtml(
        queue,
        `PBI #${pbiId}`,
        ownerPaths(queue, watches),
        // Each row's key and this PBI ride on the page, so a comment typed
        // there comes back to exactly that row - and to nothing when the
        // page is for a PBI no longer on screen.
        keysFor(queue),
        pbiId,
        watches.map((w) => ({
          path: w.path,
          label: fileName(w.path),
          comment: w.comment ?? "",
          specs: w.specs ?? [],
        })),
        // Read at click time so the page opens in the theme in front of
        // the user; it carries both schemes and its own switch.
        pagePalette(),
        // The page groups its cases the way the Queue is showing them.
        areaGrouped,
      );
      if (r.status === "error") throw new Error(r.error);
    },
    onSuccess: () => setReportOpen(true),
    onError: (e) => toast.error(`Could not open the report: ${e.message}`),
  });

  // Keep an already-open report in step with the queue.
  //
  // The page is a file on disk: nothing pushes to it, so the app rewrites
  // the same file and bumps a revision the page polls; the page then
  // pulls the fresh content over the loopback listener and swaps itself
  // in place - no reload, no lost scroll position.
  //
  // REFRESH, never VIEW: this used to call the same command as the
  // button, and the open_path at the end of that meant every comment
  // save and every queue change opened ANOTHER browser tab on a file the
  // reviewer already had open.
  //
  // Only after they have opened it once: re-exporting for a report
  // nobody asked for would write a temp file on every keystroke.
  useEffect(() => {
    if (!reportOpen || queue.length === 0) return;
    // Debounced, because a rename or a bulk edit lands as a burst of
    // queue updates and each one would otherwise rewrite the file.
    const t = window.setTimeout(() => {
      void commands
        .refreshDraftHtml(
          queue,
          `PBI #${pbiId}`,
          ownerPaths(queue, watches),
          // Each row's key and this PBI ride on the page, so a comment typed
          // there comes back to exactly that row - and to nothing when the
          // page is for a PBI no longer on screen.
          keysFor(queue),
          pbiId,
          watches.map((w) => ({
            path: w.path,
            label: fileName(w.path),
            comment: w.comment ?? "",
            specs: w.specs ?? [],
          })),
          pagePalette(),
          areaGrouped,
        )
        // Silent on failure: this is a background refresh of something
        // the developer is not necessarily looking at, and the report
        // they have is still readable.
        .catch(() => {});
    }, 800);
    return () => window.clearTimeout(t);
    // watches/pbiId are read, not depended on: a watch comment changing
    // is not a reason to rewrite the page. Switching Group by area is: the
    // open page regroups to match.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queue, reportOpen, areaGrouped]);

  // A comment typed in that page comes back here, so the card and the
  // browser tab never disagree. The file is already written by the time
  // this fires - this is only the app catching up.
  useEffect(() => {
    const un = events.draftCommentSaved.listen((e) => {
      const { key, pbi_id, text } = e.payload;
      // A page for a PBI this screen is not showing: its rows are not here.
      if (pbi_id !== pbiId || !key) return;
      // The full occurrence key: two rows sharing a title are two rows.
      setQueue((q) => {
        const keys = keysFor(q);
        return q.map((c, i) => (keys[i] === key ? { ...c, comment: text } : c));
      });
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [setQueue, pbiId]);

  return { share, viewHtml };
}
