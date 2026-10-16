// The Crystal Reports tab: the PDF preview of the report an AI assistant is
// working on, with a button that opens the file in Crystal Reports itself.
//
// The assistant changes the report through the AI Bridge; this screen only
// looks. Rust tells it when the report changed (`crystalReportChanged`), and
// it reads the open report and its last PDF again. The PDF crosses the IPC
// boundary as base64 and is shown from a blob: URL, which is let go when the
// next one replaces it and when the screen goes (the CSP allows blob: frames).

import { useCallback, useEffect, useRef, useState } from "react";
import { commands, events, type CurrentReport } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconOpenInBrowser } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

/** What the screen is showing. `null` until the first read comes back. */
type View =
  | { kind: "empty" }
  | { kind: "error"; message: string }
  | { kind: "report"; report: CurrentReport; url: string | null };

const NOTHING_OPEN = "No report open. Ask your AI assistant to open one.";
const FILE_GONE = "The report file is no longer there.";
const NOT_DRAWN = "The preview could not be drawn.";

/** The PDF in `base64` (no data-URL prefix) as a Blob the iframe can load. */
function pdfBlob(base64: string): Blob {
  const text = atob(base64);
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
  return new Blob([bytes], { type: "application/pdf" });
}

/** A report path as its file name and the folder it is in (either separator). */
function splitPath(path: string): { name: string; folder: string } {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut < 0 ? { name: path, folder: "" } : { name: path.slice(cut + 1), folder: path.slice(0, cut) };
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function CrystalReports() {
  const [view, setView] = useState<View | null>(null);
  const [opening, setOpening] = useState(false);
  const [openError, setOpenError] = useState<string | null>(null);
  // The blob: URL the iframe is showing, and which read is the newest, so a
  // slow read that comes back after a newer one cannot put an old PDF back.
  const urlRef = useRef<string | null>(null);
  const newest = useRef(0);

  const releaseUrl = useCallback(() => {
    if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    urlRef.current = null;
  }, []);

  const load = useCallback(async () => {
    const mine = ++newest.current;
    try {
      const report = await unwrapStr(commands.crystalCurrent());
      const pdf = report ? await unwrapStr(commands.crystalPreviewPdf()) : null;
      if (mine !== newest.current) return;
      releaseUrl();
      if (!report) {
        setView({ kind: "empty" });
        return;
      }
      // A failed render is an empty PDF with its reason in the notes; a file
      // that is gone has nothing worth drawing either.
      if (pdf && report.preview.page_count > 0 && !report.missing) {
        urlRef.current = URL.createObjectURL(pdfBlob(pdf));
      }
      setView({ kind: "report", report, url: urlRef.current });
    } catch (e) {
      if (mine !== newest.current) return;
      releaseUrl();
      setView({ kind: "error", message: message(e) });
    }
  }, [releaseUrl]);

  useEffect(() => {
    void load();
    const un = events.crystalReportChanged.listen(() => void load());
    return () => {
      newest.current++;
      un.then((f) => f()).catch(() => {});
      releaseUrl();
    };
  }, [load, releaseUrl]);

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
  const drawn = view.url !== null;
  return (
    <section aria-label="Report" className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 space-y-1">
          <div className="min-w-0" title={preview.path}>
            <div className="truncate text-sm font-medium text-text">{name}</div>
            {folder && <div className="truncate text-xs text-muted">{folder}</div>}
          </div>
          {!missing && (
            <div className="flex flex-wrap gap-x-3 text-xs text-muted">
              <span>{preview.live ? `Live data from ${preview.live}` : "Saved data"}</span>
              {preview.page_count > 0 && (
                <span>{preview.page_count === 1 ? "1 page" : `${preview.page_count} pages`}</span>
              )}
            </div>
          )}
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
      </div>
      {missing && <p className="text-sm text-danger">{FILE_GONE}</p>}
      {!missing && !drawn && <p className="text-sm text-text">{NOT_DRAWN}</p>}
      {!missing && preview.diagnostics.length > 0 && (
        <ul aria-label="Preview notes" className="list-disc space-y-0.5 pl-5 text-xs text-muted">
          {preview.diagnostics.map((line, i) => (
            <li key={i}>{line}</li>
          ))}
        </ul>
      )}
      {drawn && (
        <iframe
          title="Report preview"
          src={view.url ?? undefined}
          className="min-h-[480px] w-full flex-1 rounded-lg border border-border bg-surface"
        />
      )}
    </section>
  );
}
