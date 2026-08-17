import { useMutation } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import {
  forgetRecentImport,
  loadRecentImports,
  recordRecentImport,
} from "../lib/recentImports";
import { commands, events, type PbiHit, type SharedQueue } from "../bindings";
import PickPbiEmpty from "../components/PickPbiEmpty";
import GeneralComments from "../components/GeneralComments";
import QueueSection from "../components/QueueSection";
import { appIsInView, osNotify } from "../lib/assignedAlerts";
import SyncReport from "../components/SyncReport";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Modal } from "../components/ui/modal";
import {
  fileName,
  loadWatches,
  saveWatches,
  syncFromFile,
  syncNotification,
  patchWatch,
  upsertWatch,
  withoutFileCases,
  type SyncChange,
  type WatchedFile,
} from "../lib/fileSync";
import { useQueue } from "../hooks/useQueue";
import {
  IconCancel,
  IconConfirm,
  IconImport,
  IconNext,
  IconRemove,
  IconStopWatching,
} from "../lib/actionIcons";

export default function ImportFile({
  org,
  project,
  pbi,
  onPickPbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  onPickPbi?: (pbi: PbiHit) => void;
}) {
  const { queue, setQueue } = useQueue(org, pbi?.id ?? null);
  const [warnings, setWarnings] = useState<string[]>([]);
  /** Recently imported JSON files, offered back when the queue is empty. */
  const [recents, setRecents] = useState(loadRecentImports);
  const [shareLink, setShareLink] = useState("");
  // A fetched draft whose PBI differs from the current selection: the
  // queue is stored PER PBI, so loading it here would hide the cases the
  // moment the user switches. Ask first.
  const [choice, setChoice] = useState<SharedQueue | null>(null);
  // Cases waiting for a specific PBI to become current. Switching PBI is
  // async (App owns it, useQueue re-keys on the new id), so the load is
  // deferred until `pbi` actually matches - writing immediately would put
  // them in the OLD PBI's queue, which is the bug this fixes.
  const [pendingFor, setPendingFor] = useState<{
    pbiId: number;
    data: SharedQueue;
    extraWarnings: string[];
  } | null>(null);

  // Every JSON file this queue was imported from, so an assistant editing
  // any of them flows straight through. Persisted per PBI, so leaving the
  // tab (or the app) doesn't stop the watches.
  const [watches, setWatchesState] = useState<WatchedFile[]>([]);
  const [report, setReport] = useState<{
    changes: SyncChange[];
    file: string;
    warnings: number;
  } | null>(null);
  // The watches the user asked to drop, pending the "and their cases?"
  // answer. A list rather than one file so Stop and Remove all go through
  // the same confirmation - the question is identical, only the count
  // differs, and two dialogs would be two things to keep in step.
  const [dropping, setDropping] = useState<WatchedFile[] | null>(null);

  // Keyed on the id, never the object: a new PbiHit identity on every
  // render would re-arm the effects below and could cancel an in-flight
  // re-parse forever.
  const pbiId = pbi?.id ?? null;
  const setWatches = useCallback(
    (next: WatchedFile[] | ((prev: WatchedFile[]) => WatchedFile[])) => {
      setWatchesState((prev) => {
        const list = typeof next === "function" ? next(prev) : next;
        if (pbiId != null) saveWatches(org, pbiId, list);
        return list;
      });
    },
    [org, pbiId],
  );

  // Scope switch -> that PBI's watches, and drop a report about the old one.
  useEffect(() => {
    setWatchesState(pbiId != null ? loadWatches(org, pbiId) : []);
    setReport(null);
    setDropping(null);
  }, [org, pbiId]);

  // The reconcile reads the queue but must not re-run when it changes -
  // only a new file fingerprint should trigger it.
  const queueRef = useRef(queue);
  queueRef.current = queue;

  // Newest fingerprint per path, as reported by the OS watcher. Nothing
  // polls - untouched files cost nothing at all.
  const [detected, setDetected] = useState<Record<string, string>>({});
  // A stable key so the arming effect re-runs when the SET of watched
  // paths changes, but not when a fingerprint or snapshot does.
  const watchedPaths = JSON.stringify(watches.map((w) => w.path));

  useEffect(() => {
    const paths: string[] = JSON.parse(watchedPaths);
    if (paths.length === 0) return;
    let live = true;
    let unlisten: (() => void) | undefined;

    void (async () => {
      for (const path of paths) {
        const r = await commands.watchFile(path);
        if (!live) return;
        if (r.status === "error") {
          toast.error(`Could not watch ${fileName(path)}: ${r.error}`);
          setWatches((prev) => prev.filter((w) => w.path !== path));
          continue;
        }
        // The watcher only reports changes from now on, so check once for
        // an edit made while the app was closed or this tab was elsewhere.
        const current = await commands.fileStamp(path);
        if (live && current) setDetected((d) => ({ ...d, [path]: current }));
      }
    })();

    void events.watchedFileChanged
      .listen((e) => {
        setDetected((d) => ({ ...d, [e.payload.path]: e.payload.stamp }));
      })
      .then((f) => {
        if (live) unlisten = f;
        else f();
      });

    return () => {
      live = false;
      // Teardown must never throw or reject: this also runs when the app
      // is shutting down, where the IPC bridge may already be gone.
      // `unlisten` resolves asynchronously despite its void signature.
      void (async () => {
        try {
          await unlisten?.();
        } catch {
          /* already detached */
        }
      })();
      void commands.unwatchAllFiles().catch(() => {});
    };
    // Only the SET of paths re-arms the watchers; a new fingerprint must not.
  }, [watchedPaths, setWatches]);

  // One reconcile per file whose fingerprint moved. Serialized through a
  // ref-guard so two files saved at once can't interleave their queue
  // writes and lose one of them.
  const syncing = useRef(false);
  useEffect(() => {
    const stale = watches.find((w) => detected[w.path] && detected[w.path] !== w.stamp);
    if (!stale || syncing.current) return;

    syncing.current = true;
    let cancelled = false;
    const fresh = detected[stale.path];
    void (async () => {
      try {
        const r = await commands.parseImportFile(stale.path);
        if (cancelled) return;
        if (r.status === "error") {
          // Now invalid JSON. Adopt the fingerprint anyway, so this same
          // broken content isn't re-parsed and re-toasted on the next event.
          setWatches((prev) =>
            prev.map((w) => (w.path === stale.path ? { ...w, stamp: fresh } : w)),
          );
          toast.error(`${fileName(stale.path)} could not be read: ${r.error}`);
          return;
        }
        const synced = syncFromFile(queueRef.current, stale.snapshot, r.data.cases);
        // The set-wide comment lives in the same file, so an edit can have
        // moved it as well - re-read rather than let the panel go stale.
        const comment = await commands.readGeneralComment(stale.path);
        if (cancelled) return;
        setWatches((prev) =>
          patchWatch(prev, stale.path, {
            stamp: fresh,
            snapshot: synced.snapshot,
            comment,
          }),
        );
        setWarnings(r.data.warnings);
        if (synced.changes.length > 0) setQueue(synced.queue);
        // Report a save that produced only warnings too - a draft that
        // stopped being valid is exactly what someone needs to hear about.
        if (synced.changes.length > 0 || r.data.warnings.length > 0) {
          setReport({
            changes: synced.changes,
            file: fileName(stale.path),
            warnings: r.data.warnings.length,
          });
          // The whole point of watching a file is that an assistant can
          // edit it while you are somewhere else. If the app is behind
          // another window the report panel is not feedback at all, so
          // the OS says it instead - and stays quiet when you are looking.
          if (!appIsInView()) {
            const n = syncNotification(fileName(stale.path), synced.changes);
            void osNotify(n.title, n.body);
          }
        }
      } finally {
        syncing.current = false;
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [detected, watches, setWatches, setQueue]);

  // A general comment typed in the browser view. The file is already
  // written; this keeps the app's panel and its fingerprint in step.
  useEffect(() => {
    const un = events.draftGeneralCommentSaved.listen((e) => {
      setWatches((prev) =>
        patchWatch(prev, e.payload.path, { comment: e.payload.text, stamp: e.payload.stamp }),
      );
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [setWatches]);

  // A per-case comment typed there lands in the same file, so the watch's
  // fingerprint has to move too - the watcher stays quiet about the app's
  // own write, so nothing else would tell us.
  useEffect(() => {
    const un = events.draftCommentSaved.listen((e) => {
      if (!e.payload.path) return;
      setWatches((prev) => patchWatch(prev, e.payload.path, { stamp: e.payload.stamp }));
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, [setWatches]);

  const dismissReport = useCallback(() => setReport(null), []);

  /** Stop following these files, optionally taking their cases with them.
   *
   * The cases of ALL the dropped files are considered together against the
   * ones that remain, so a case two dropped files share still goes, and a
   * case a surviving file also contains still stays. */
  const dropWatch = useCallback(
    (targets: WatchedFile[], alsoRemoveCases: boolean) => {
      const dropped = new Set(targets.map((t) => t.path));
      for (const t of targets) void commands.unwatchFile(t.path).catch(() => {});
      const others = watches.filter((w) => !dropped.has(w.path));
      setWatches(others);
      if (alsoRemoveCases) {
        const owned = targets.flatMap((t) => t.snapshot);
        setQueue((q) =>
          withoutFileCases(
            q,
            owned,
            others.map((w) => w.snapshot),
          ),
        );
      }
      setDropping(null);
    },
    [watches, setWatches, setQueue],
  );

  // Rows the last sync touched, so the queue itself shows where the change
  // landed rather than only naming it in the banner.
  const flash = useMemo(() => {
    if (!report) return undefined;
    const m: Record<string, "added" | "changed"> = {};
    for (const c of report.changes) {
      if (c.kind !== "removed") m[c.key] = c.kind;
    }
    return m;
  }, [report]);

  useEffect(() => {
    if (!pendingFor || pbi?.id !== pendingFor.pbiId) return;
    const { pbiId: forPbi, data, extraWarnings } = pendingFor;
    setQueue((q) => [...q, ...data.cases]);
    setWarnings([...extraWarnings, ...data.warnings]);
    setPendingFor(null);
    // A share link has no file behind it, so the post-submit id write-back
    // had nowhere to land - the created ids lived only in Azure DevOps and
    // a re-import of the same drafts silently created every case again.
    // Materialize the share as a real local draft and follow it like any
    // imported file, so the whole stamping machinery just works.
    void commands.materializeSharedDraft(forPbi, data.cases).then((r) => {
      if (!r || (r.status === "ok" && !r.data)) return; // defensive: mocked/absent backend
      if (r.status === "error") {
        toast.warning(
          `Imported, but a local copy could not be saved: ${r.error}. ` +
            `Without that file the app can't remember the ids a submit creates - ` +
            `use Export JSON after submitting so you keep them.`,
          { duration: 15000 },
        );
        return;
      }
      setWatches((prev) =>
        upsertWatch(prev, { path: r.data.path, stamp: r.data.stamp, snapshot: data.cases }),
      );
      toast.info(`Saved a local copy and watching it for changes: ${fileName(r.data.path)}`);
    });
    toast.success(
      `Imported ${data.cases.length} shared case${data.cases.length === 1 ? "" : "s"} for review.`,
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingFor, pbi?.id, setQueue]);

  // A pasted share link (see ado_share.rs): one-time use - a successful
  // import revokes it, so a second paste tells the user it's spent.
  const importShared = useMutation({
    mutationFn: async () => {
      const r = await commands.fetchSharedQueue(shareLink.trim());
      if (r.status === "error") throw new Error(r.error);
      return r.data;
    },
    onSuccess: (data) => {
      setShareLink("");
      if (pbi && data.pbi_id !== pbi.id) {
        setChoice(data); // ask which PBI's queue to load into
        return;
      }
      setPendingFor({ pbiId: data.pbi_id, data, extraWarnings: [] });
    },
    onError: (e) => toast.error(`Shared import failed: ${e.message}`),
  });

  const importFile = useMutation({
    // `givenPath` set: a Recent JSON Imports row - same importer, no file
    // dialog. Absent: the Import button's normal picker flow.
    mutationFn: async (givenPath: string | undefined) => {
      // JSON is the import format (the AI round-trip file exports produce).
      const path =
        givenPath ??
        (await open({
          multiple: false,
          filters: [{ name: "Test cases (JSON)", extensions: ["json"] }],
        }));
      if (typeof path !== "string") return null;
      const r = await commands.parseImportFile(path);
      if (r.status === "error") throw new Error(r.error);
      return {
        path,
        stamp: await commands.fileStamp(path),
        data: r.data,
        // Whatever the file already says about the set as a whole - very
        // often written by whoever generated it.
        comment: await commands.readGeneralComment(path),
      };
    },
    onSuccess: (res) => {
      if (!res) return;
      const { path, stamp, data, comment } = res;
      setQueue((q) => [...q, ...data.cases]);
      setWarnings(data.warnings);
      setReport(null);
      setRecents(recordRecentImport(path));
      // From here on, edits to this file land in the queue by themselves.
      // Re-importing the same file replaces its entry rather than adding a
      // second watch on it.
      if (stamp)
        setWatches((prev) => upsertWatch(prev, { path, stamp, snapshot: data.cases, comment }));
      toast.success(
        `Imported ${data.cases.length} case${data.cases.length === 1 ? "" : "s"}` +
          (data.warnings.length ? ` with ${data.warnings.length} warning(s)` : ""),
      );
    },
    // A recent whose file is gone (or unreadable) is not worth offering
    // again - the row disappears with the explanation, rather than failing
    // identically on every future click.
    onError: (e, givenPath) => {
      if (typeof givenPath === "string") {
        setRecents(forgetRecentImport(givenPath));
        toast.error(`Could not reopen ${fileName(givenPath)}: ${e.message}. Removed from recent imports.`);
        return;
      }
      toast.error(`Import failed: ${e.message}`);
    },
  });

  if (!org || !project || !pbi) {
    return (
      <PickPbiEmpty
        message="Pick an organization, project and PBI in the bar above, then import a JSON file of test cases."
        org={org}
        project={project}
        onPickPbi={onPickPbi}
      />
    );
  }

  return (
    <div className="space-y-4">
      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">Import test cases</h2>
        <p className="text-sm text-muted">
          Import your test cases from a JSON file. Imported files are watched -
          edit the file and the changes flow into the queue automatically.
          Cases that carry an id update that exact work item; cases without
          one are created new.
        </p>
        <div className="flex gap-2">
          <Button disabled={importFile.isPending} onClick={() => importFile.mutate(undefined)}>
            <IconImport aria-hidden />
            {importFile.isPending ? "Importing" : "Import JSON"}
          </Button>
        </div>
        {watches.length > 0 && (
          <div className="space-y-1 rounded-md border border-border/60 bg-surface-2 px-2.5 py-1.5 text-xs">
            <div className="flex items-center gap-2">
              <span className="relative flex h-2 w-2 shrink-0" aria-hidden>
                <span className="absolute inline-flex h-full w-full rounded-full bg-accent opacity-60 motion-safe:animate-ping" />
                <span className="relative inline-flex h-2 w-2 rounded-full bg-accent" />
              </span>
              <span className="text-muted">
                {watches.length === 1
                  ? "Watching 1 file — edits are applied to the queue automatically."
                  : `Watching ${watches.length} files — edits are applied to the queue automatically.`}
              </span>
              {watches.length > 1 && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="ml-auto"
                  aria-label="Stop watching every file"
                  onClick={() => setDropping(watches)}
                >
                  <IconStopWatching aria-hidden />
                  Remove all
                </Button>
              )}
            </div>
            {/* One row per file once there is a choice to make. A single
                file needs no list - its name goes on the line above. */}
            <ul className="space-y-0.5">
              {watches.map((w) => (
                <li key={w.path} className="flex items-center gap-2">
                  <span
                    className="id-mono min-w-0 flex-1 truncate text-text"
                    title={w.path}
                  >
                    {fileName(w.path)}
                  </span>
                  <span className="shrink-0 text-faint">
                    {w.snapshot.length} case{w.snapshot.length === 1 ? "" : "s"}
                  </span>
                  <Button
                    variant="ghost"
                    size="sm"
                    aria-label={`Stop watching ${fileName(w.path)}`}
                    onClick={() => setDropping([w])}
                  >
                    <IconStopWatching aria-hidden />
                    Stop
                  </Button>
                </li>
              ))}
            </ul>
          </div>
        )}
        {watches.length > 0 && (
          <GeneralComments
            watches={watches}
            onSaved={(path, text, stamp) =>
              setWatches((prev) => patchWatch(prev, path, { comment: text, stamp }))
            }
          />
        )}
        {report && (
          <SyncReport
            changes={report.changes}
            fileName={report.file}
            warnings={report.warnings}
            onDismiss={dismissReport}
          />
        )}
        <div className="space-y-1 border-t border-border/60 pt-3">
          <p className="text-xs text-muted">
            Paste a share link a teammate sent you. Links are one-time use -
            once imported, the link expires.
          </p>
          <div className="flex gap-2">
            <Input
              aria-label="Share link"
              placeholder="tcm-share:…"
              className="id-mono flex-1 py-1.5 text-xs"
              value={shareLink}
              onChange={(e) => setShareLink(e.target.value)}
            />
            <Button
              variant="outline"
              disabled={!shareLink.trim() || importShared.isPending}
              onClick={() => importShared.mutate()}
            >
              <IconImport aria-hidden />
              {importShared.isPending ? "Fetching" : "Import shared"}
            </Button>
          </div>
        </div>
        {warnings.length > 0 && (
          <ul className="max-h-32 space-y-0.5 overflow-y-auto text-xs text-warning">
            {warnings.map((w, i) => (
              <li key={i}>{w}</li>
            ))}
          </ul>
        )}
      </section>

      <QueueSection
        org={org}
        project={project}
        pbiId={pbi.id}
        queue={queue}
        setQueue={setQueue}
        flash={flash}
        watches={watches}
        recentImports={recents}
        onOpenRecent={(path) => importFile.mutate(path)}
        onForgetRecent={(path) => setRecents(forgetRecentImport(path))}
        // Called both by Remove all and by a submit that emptied the queue.
        // Either way the import is finished, and everything that existed to
        // service it goes with it: the file watches (which would otherwise
        // let a later save refill a queue already dealt with), the change
        // report describing edits that have now been written, and the
        // per-file general comments, which were notes for that review.
        onQueueCleared={() => {
          if (watches.length > 0) dropWatch(watches, false);
          setReport(null);
        }}
        // A bulk change was written into a watched file: move the watch's
        // fingerprint and snapshot forward so the watcher stays silent
        // about our own write, and ownership keeps matching the new titles.
        onWatchPatched={(path, fields) =>
          setWatches((prev) => patchWatch(prev, path, fields))
        }
      />

      {dropping && (
        <Modal onClose={() => setDropping(null)} className="w-full max-w-md p-4">
          <h2 className="text-sm font-semibold text-text">
            {dropping.length === 1
              ? `Stop watching ${fileName(dropping[0].path)}?`
              : `Stop watching all ${dropping.length} files?`}
          </h2>
          <p className="mt-2 text-sm text-muted">
            {(() => {
              const n = dropping.reduce((sum, w) => sum + w.snapshot.length, 0);
              const files = dropping.length === 1 ? "this file" : "these files";
              return n === 1
                ? `Edits to ${files} will no longer flow into the queue. The 1 case it added is still queued - keep it, or remove it too?`
                : `Edits to ${files} will no longer flow into the queue. The ${n} cases ${dropping.length === 1 ? "it" : "they"} added are still queued - keep them, or remove them too?`;
            })()}
          </p>
          {dropping.length > 1 && (
            <ul className="mt-2 space-y-0.5 text-xs text-faint">
              {dropping.map((w) => (
                <li key={w.path} className="id-mono truncate" title={w.path}>
                  {fileName(w.path)}
                </li>
              ))}
            </ul>
          )}
          <p className="mt-2 text-xs text-faint">
            Cases you typed by hand, or that another watched file also contains, are
            never removed.
          </p>
          <div className="mt-4 flex flex-wrap justify-end gap-2">
            <Button variant="outline" size="sm" onClick={() => setDropping(null)}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button variant="outline" size="sm" onClick={() => dropWatch(dropping, false)}>
              <IconConfirm aria-hidden />
              Keep the cases
            </Button>
            <Button variant="danger" size="sm" onClick={() => dropWatch(dropping, true)}>
              <IconRemove aria-hidden />
              Remove them too
            </Button>
          </div>
        </Modal>
      )}

      {choice && (
        <Modal onClose={() => setChoice(null)} className="w-full max-w-md p-4">
          <h2 className="text-sm font-semibold text-text">This draft is for a different PBI</h2>
          <p className="mt-2 text-sm text-muted">
            It was shared for{" "}
            <span className="text-text">
              #{choice.pbi_id}
              {choice.pbi_title ? ` ${choice.pbi_title}` : ""}
            </span>
            , but you have <span className="text-text">#{pbi.id} {pbi.title}</span> selected.
          </p>
          <p className="mt-2 text-xs text-faint">
            Queued cases are kept per PBI, so they will appear under whichever you choose.
          </p>
          <div className="mt-4 flex flex-wrap justify-end gap-2">
            <Button variant="outline" size="sm" onClick={() => setChoice(null)}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                setPendingFor({
                  pbiId: pbi.id,
                  data: choice,
                  extraWarnings: [
                    `This draft was shared for PBI #${choice.pbi_id}, but you loaded it under #${pbi.id} - check before creating.`,
                  ],
                });
                setChoice(null);
              }}
            >
              <IconConfirm aria-hidden />
              Stay on #{pbi.id}
            </Button>
            <Button
              size="sm"
              disabled={!onPickPbi}
              title={onPickPbi ? undefined : "Switching is unavailable here"}
              onClick={() => {
                setPendingFor({ pbiId: choice.pbi_id, data: choice, extraWarnings: [] });
                onPickPbi?.({
                  id: choice.pbi_id,
                  title: choice.pbi_title,
                  work_item_type: choice.pbi_work_item_type,
                });
                setChoice(null);
              }}
            >
              <IconNext aria-hidden />
              Switch to #{choice.pbi_id}
            </Button>
          </div>
        </Modal>
      )}
    </div>
  );
}
