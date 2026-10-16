// pdf.js, set up the one way this app uses it. The page view imports this
// module dynamically, so pdf.js is its own chunk and only loads when a
// report is shown - never at startup.
//
// The worker is the file pdf.js ships, emitted by Vite into the app's own
// assets and served from 'self': the CSP has no worker-src, so workers fall
// back to script-src 'self', which a same-origin URL satisfies. A remote or
// blob: worker would be refused - which is why workerSrc is always set here
// (left unset, pdf.js would try to build one itself).

import { GlobalWorkerOptions, getDocument } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

GlobalWorkerOptions.workerSrc = workerUrl;

/** Opens a PDF from its bytes. pdf.js takes the buffer it is given, so the caller's array is copied. */
export function openPdf(data: Uint8Array) {
  // useWasm: false - pdf.js must never compile code from a PDF. Since 6.0
  // it no longer evals (the old `isEvalSupported` option is gone); instead
  // it compiles a PDF's PostScript functions, and loads its image and
  // colour decoders, as WebAssembly. script-src has no 'wasm-unsafe-eval',
  // so that would only be a CSP violation before its JavaScript fallback;
  // this goes straight to the fallback. No wasmUrl is given either, so
  // nothing is fetched for JPEG 2000 / JBIG2 images, which the report
  // renderer never writes.
  return getDocument({ data: data.slice(), useWasm: false });
}

export type { PDFDocumentLoadingTask, PDFPageProxy, RenderTask } from "pdfjs-dist";
