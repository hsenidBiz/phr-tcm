// The Crystal Reports tab: the report an AI assistant is working on - a
// header card (its file, where its data came from, its pages, and a button
// that opens it in Crystal Reports), its notes in plain words, and its
// pages drawn in the app by pdf.js.
//
// The assistant changes the report through the AI Bridge; this screen only
// looks. Rust tells it when the report changed (`crystalReportChanged`), and
// it reads the open report and its last PDF again; the newest read wins.
// The PDF crosses the IPC boundary as base64 and is handed to pdf.js as
// bytes - no blob: URL and no frame (see ./PageView).

import { useCallback, useEffect, useRef, useState } from "react";
import { commands, events, type CurrentReport } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { IconOpenInBrowser } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { PageView } from "./PageView";
import { PreviewNotes } from "./PreviewNotes";

/** What the screen is showing. `null` until the first read comes back. */
type View =
  | { kind: "empty" }
  | { kind: "error"; message: string }
  | {
      kind: "report";
      /** Which read this is, so a late failure from an older PDF is not pinned on this one. */
      read: number;
      report: CurrentReport;
      /** The PDF's bytes, or null when there is nothing to draw. */
      pdf: Uint8Array | null;
      /** Why pdf.js could not open or draw the PDF, once it has said so. */
      drawError: string | null;
    };

const NOTHING_OPEN = "No report open. Ask your AI assistant to open one.";
const FILE_GONE = "The report file is no longer there.";
const NOT_DRAWN = "The preview could not be drawn.";

/** The PDF in `base64` (no data-URL prefix) as bytes. */
function pdfBytes(base64: string): Uint8Array {
  const text = atob(base64);
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
  return bytes;
}

/** A report path as its file name and the folder it is in (either separator). */
function splitPath(path: string): { name: string; folder: string } {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut < 0 ? { name: path, folder: "" } : { name: path.slice(cut + 1), folder: path.slice(0, cut) };
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const pagesLabel = (n: number) => (n === 1 ? "1 page" : `${n} pages`);

export default function CrystalReports() {
  const [view, setView] = useState<View | null>(null);
  const [opening, setOpening] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  // Which read is the newest, so a slow read that comes back after a newer
  // one cannot put an old PDF back.
  const newest = useRef(0);

  const load = useCallback(async () => {
    const mine = ++newest.current;
    try {
      const report = await unwrapStr(commands.crystalCurrent());
      const pdf = report ? await unwrapStr(commands.crystalPreviewPdf()) : null;
      if (mine !== newest.current) return;
      if (!report) {
        setView({ kind: "empty" });
        return;
      }
      // A failed render is an empty PDF with its reason in the notes; a file
      // that is gone has nothing worth drawing either.
      const drawable = pdf && report.preview.page_count > 0 && !report.missing;
      setView({ kind: "report", read: mine, report, pdf: drawable ? pdfBytes(pdf) : null, drawError: null });
    } catch (e) {
      if (mine !== newest.current) return;
      setView({ kind: "error", message: message(e) });
    }
  }, []);

  useEffect(() => {
    void load();
    const un = events.crystalReportChanged.listen(() => void load());
    return () => {
      newest.current++;
      un.then((f) => f()).catch(() => {});
    };
  }, [load]);

  const drawFailed = useCallback((read: number, why: string) => {
    setView((v) => (v?.kind === "report" && v.read === read ? { ...v, drawError: why } : v));
  }, []);

  const openInDesigner = async () => {
    setOpening(true);
    setOpenError(null);
    try {
      await unwrapStr(commands.crystalOpenInDesigner());
    } catch (e) {
      setOpenError(message(e));
    } finally {
      setOpening(false);
    }
  };

  if (!view) return <p className="text-sm text-muted">Loading the report…</p>;
  if (view.kind === "error") {
    return (
      <p role="alert" className="text-sm text-danger">
        {view.message}
      </p>
    );
  }
  if (view.kind === "empty") return <p className="text-sm text-muted">{NOTHING_OPEN}</p>;

  const { preview, missing } = view.report;
  const { name, folder } = splitPath(preview.path);
  const drawn = view.pdf !== null && view.drawError === null;
  const notes = view.drawError ? [...preview.diagnostics, view.drawError] : preview.diagnostics;
  const read = view.read;
  return (
    <section aria-label="Report" className="flex min-h-0 flex-1 flex-col gap-3 pb-6">
      <header className="flex shrink-0 flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-surface px-4 py-3">
        <div className="min-w-0 flex-1" title={preview.path}>
          <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
            <span className="truncate text-sm font-semibold text-text">{name}</span>
            {!missing && (
              <>
                <Badge
                  className={cn(
                    "whitespace-nowrap text-[11px] font-medium",
                    preview.live ? "bg-info/15 text-info" : "bg-surface-2 text-muted",
                  )}
                >
                  {preview.live ? `Live data from ${preview.live}` : "Saved data"}
                </Badge>
                {preview.page_count > 0 && (
                  <span className="whitespace-nowrap text-xs text-muted">{pagesLabel(preview.page_count)}</span>
                )}
              </>
            )}
          </div>
          {folder && <div className="truncate text-xs text-muted">{folder}</div>}
        </div>
        {!missing && (
          <div className="flex flex-col items-end gap-1">
            <Button size="sm" variant="outline" disabled={opening} onClick={() => void openInDesigner()}>
              <IconOpenInBrowser aria-hidden />
              Open in Crystal Reports
            </Button>
            {openError && (
              <p role="alert" className="max-w-md text-right text-xs text-danger">
                {openError}
              </p>
            )}
          </div>
        )}
      </header>
      {missing && <p className="text-sm text-danger">{FILE_GONE}</p>}
      {!missing && !drawn && <p className="text-sm text-text">{NOT_DRAWN}</p>}
      {!missing && <PreviewNotes lines={notes} live={preview.live} otherOpenAtFirst={!drawn} />}
      {!missing && drawn && view.pdf && (
        <PageView pdf={view.pdf} pageCount={preview.page_count} onFailed={(why) => drawFailed(read, why)} />
      )}
    </section>
  );
}
