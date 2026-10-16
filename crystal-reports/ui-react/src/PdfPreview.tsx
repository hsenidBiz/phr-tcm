/**
 * PdfPreview — the *render* preview: what the report actually looks like with data.
 *
 * Takes the PDF bytes from `previewReport()`, wraps them in a Blob URL, and shows them in an
 * iframe (WebView2/WKWebView both render PDFs natively — zoom/scroll come free). The URL is
 * revoked on change/unmount so the blob doesn't leak.
 *
 * Diagnostics from the render pipeline (an object kind the renderer doesn't draw, a formula
 * that failed over an empty row set) render as a collapsible strip — a degraded preview with
 * visible reasons beats a refused one. This is the counterpart to LayoutPreview (where things
 * are); the distinction between the two is the point.
 */
import { useEffect, useMemo, useState } from "react";
import type { PreviewOutput } from "./types";

interface Props {
  preview: PreviewOutput;
}

export function PdfPreview({ preview }: Props) {
  const url = useMemo(() => URL.createObjectURL(new Blob([new Uint8Array(preview.pdf)], { type: "application/pdf" })), [preview]);

  useEffect(() => {
    return () => URL.revokeObjectURL(url);
  }, [url]);

  const [showDiagnostics, setShowDiagnostics] = useState(false);

  return (
    <div className="flex h-full flex-col bg-slate-900">
      <div className="flex shrink-0 items-center gap-3 border-b border-slate-700 px-3 py-1.5 text-xs text-slate-300">
        <span>
          {preview.page_count} page{preview.page_count === 1 ? "" : "s"} ·{" "}
          {(preview.pdf.length / 1024).toFixed(0)} KB
        </span>
        {preview.diagnostics.length > 0 && (
          <button
            className="rounded border border-amber-600/60 px-2 py-0.5 text-amber-400 hover:bg-amber-900/30"
            onClick={() => setShowDiagnostics((v) => !v)}
          >
            {preview.diagnostics.length} render note
            {preview.diagnostics.length === 1 ? "" : "s"}
          </button>
        )}
      </div>
      {showDiagnostics && (
        <ul className="max-h-32 shrink-0 overflow-auto border-b border-slate-700 bg-amber-950/30 px-3 py-1.5 text-[11px] text-amber-300">
          {preview.diagnostics.map((d, i) => (
            <li key={i}>· {d}</li>
          ))}
        </ul>
      )}
      <iframe
        title="report preview"
        src={url}
        className="min-h-0 w-full flex-1 bg-slate-800"
      />
    </div>
  );
}
