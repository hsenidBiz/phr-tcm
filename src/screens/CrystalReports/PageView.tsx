// The report's pages, drawn by pdf.js onto one canvas each, with a small
// toolbar to size them: zoom out / in, Fit width (the default, kept as the
// view is resized) and 100%.
//
// pdf.js is loaded on first use (./pdf, its own chunk). Each new PDF
// destroys the document before it - and with it its worker and any drawing
// still under way - and a document or drawing that finishes after a newer
// one began is ignored, so a slow old preview can never replace a new one.
// A PDF pdf.js cannot open or draw is reported through `onFailed`.

import { useEffect, useRef, useState } from "react";
import { Button } from "../../components/ui/button";
import { IconFitWidth, IconZoomIn, IconZoomOut } from "../../lib/actionIcons";
import type { PDFDocumentLoadingTask, PDFPageProxy, RenderTask } from "./pdf";

/** PDF points to CSS pixels: 100% is the page at its printed size. */
const CSS_UNITS = 96 / 72;
/** The page area's padding (p-6) on each side, which Fit width leaves clear. */
const GUTTER = 24;
const ZOOMS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4];
const MIN_ZOOM = ZOOMS[0];
const MAX_ZOOM = ZOOMS[ZOOMS.length - 1];

type Loaded = {
  /** Which load this is: drawing errors count only for the current one. */
  token: object;
  pages: PDFPageProxy[];
  /** Each page's size in CSS pixels at 100%. */
  sizes: { w: number; h: number }[];
};

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const cancelled = (e: unknown) => e instanceof Error && e.name === "RenderingCancelledException";
const clamp = (z: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z));

export function PageView({
  pdf,
  pageCount,
  onFailed,
}: {
  pdf: Uint8Array;
  pageCount: number;
  onFailed: (why: string) => void;
}) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [fit, setFit] = useState(true);
  const [zoom, setZoom] = useState(1);
  const [width, setWidth] = useState(0);
  const scroller = useRef<HTMLDivElement>(null);
  const canvases = useRef<(HTMLCanvasElement | null)[]>([]);
  const active = useRef<object | null>(null);
  const failed = useRef(onFailed);
  useEffect(() => {
    failed.current = onFailed;
  }, [onFailed]);

  // Open the PDF. The cleanup destroys the document (or stops it opening).
  useEffect(() => {
    const token = {};
    active.current = token;
    let live = true;
    let task: PDFDocumentLoadingTask | null = null;
    void (async () => {
      try {
        const { openPdf } = await import("./pdf");
        if (!live) return;
        task = openPdf(pdf);
        const doc = await task.promise;
        const pages = await Promise.all(Array.from({ length: doc.numPages }, (_, i) => doc.getPage(i + 1)));
        if (!live) return;
        const sizes = pages.map((p) => {
          const v = p.getViewport({ scale: CSS_UNITS });
          return { w: v.width, h: v.height };
        });
        setLoaded({ token, pages, sizes });
      } catch (e) {
        if (live) failed.current(message(e));
      }
    })();
    return () => {
      live = false;
      void task?.destroy();
    };
  }, [pdf]);

  // The page area's width, for Fit width; kept as the window is resized.
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const measure = () => setWidth(el.clientWidth);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const widest = loaded ? Math.max(0, ...loaded.sizes.map((s) => s.w)) : 0;
  // Whole percent, so a resize by a pixel or two does not redraw every page.
  const fitZoom = widest > 0 && width > 2 * GUTTER ? clamp(Math.floor(((width - 2 * GUTTER) / widest) * 100) / 100) : 1;
  const scale = fit ? fitZoom : zoom;

  // Draw every page at the current scale, one after another, sharp at the
  // screen's pixel density. A new scale or document cancels what is left.
  useEffect(() => {
    if (!loaded) return;
    let live = true;
    const tasks: RenderTask[] = [];
    const dpr = window.devicePixelRatio || 1;
    void (async () => {
      for (const [i, page] of loaded.pages.entries()) {
        const canvas = canvases.current[i];
        if (!live) return;
        if (!canvas) continue;
        const viewport = page.getViewport({ scale: scale * CSS_UNITS * dpr });
        canvas.width = Math.round(viewport.width);
        canvas.height = Math.round(viewport.height);
        const task = page.render({ canvas, viewport });
        tasks.push(task);
        try {
          await task.promise;
        } catch (e) {
          if (live && !cancelled(e) && active.current === loaded.token) failed.current(message(e));
          return;
        }
      }
    })();
    return () => {
      live = false;
      for (const t of tasks) t.cancel();
    };
  }, [loaded, scale]);

  const zoomTo = (z: number) => {
    setFit(false);
    setZoom(clamp(z));
  };
  const zoomIn = () => zoomTo(ZOOMS.find((z) => z > scale + 0.001) ?? MAX_ZOOM);
  const zoomOut = () => zoomTo([...ZOOMS].reverse().find((z) => z < scale - 0.001) ?? MIN_ZOOM);
  const count = loaded?.pages.length ?? 0;

  return (
    <section
      aria-label="Pages"
      className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border border-border bg-surface"
    >
      <div role="toolbar" aria-label="Page view" className="flex flex-wrap items-center gap-1 border-b border-border px-2 py-1.5">
        <Button size="sm" variant="ghost" className="px-2" aria-label="Zoom out" disabled={scale <= MIN_ZOOM} onClick={zoomOut}>
          <IconZoomOut aria-hidden />
        </Button>
        <span role="status" aria-label="Zoom" className="w-12 text-center text-xs tabular-nums text-text">
          {Math.round(scale * 100)}%
        </span>
        <Button size="sm" variant="ghost" className="px-2" aria-label="Zoom in" disabled={scale >= MAX_ZOOM} onClick={zoomIn}>
          <IconZoomIn aria-hidden />
        </Button>
        <Button size="sm" variant={fit ? "pill" : "ghost"} aria-pressed={fit} onClick={() => setFit(true)}>
          <IconFitWidth aria-hidden />
          Fit width
        </Button>
        <Button size="sm" variant={!fit && zoom === 1 ? "pill" : "ghost"} aria-pressed={!fit && zoom === 1} onClick={() => zoomTo(1)}>
          100%
        </Button>
        <span className="ml-auto pr-1 text-xs text-muted">{pageCount === 1 ? "1 page" : `${pageCount} pages`}</span>
      </div>
      <div ref={scroller} className="min-h-0 flex-1 overflow-auto bg-surface-2">
        {/* w-max min-w-full: centred while the pages are narrower than the
            view, and scrollable to their left edge once they are wider. */}
        <div className="flex w-max min-w-full flex-col items-center gap-4 p-6">
          {!loaded && <p className="text-sm text-muted">Drawing the pages…</p>}
          {loaded?.sizes.map((s, i) => (
            <canvas
              key={i}
              ref={(el) => {
                canvases.current[i] = el;
              }}
              role="img"
              aria-label={`Page ${i + 1} of ${count}`}
              className="block shrink-0 rounded-sm shadow-md ring-1 ring-border"
              style={{ width: Math.round(s.w * scale), height: Math.round(s.h * scale) }}
            />
          ))}
        </div>
      </div>
    </section>
  );
}
