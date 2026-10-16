# ui-react — editor components for the Tauri + Tailwind V4 front end

Zero-dependency React + TS components (React ≥ 18, `@tauri-apps/api` v2). Tailwind utility
classes inline — works with Tailwind V4 out of the box (the slate/sky palette ships in the
default theme).

## Files

| file | what |
|---|---|
| `src/types.ts` | TS types for the `crpt-ir/1` IR + edit ops + preview output, and twips/points/inches converters |
| `src/api.ts` | `invoke` wrappers for the backend commands (`Out/tauri-commands`) |
| `src/ReportTree.tsx` | areas → sections → objects navigator |
| `src/LayoutPreview.tsx` | SVG/page canvas of the design surface, click-to-select, zoom |
| `src/PdfPreview.tsx` | the *render* preview: PDF bytes → Blob URL → iframe, with the render diagnostics strip |
| `src/PropertyInspector.tsx` | edit forms: text, position/size, font/color, section height, page setup — collects a pending op batch |
| `src/ReportEditor.tsx` | composition root (open / apply / save, single-IR state) |

## Usage

Copy `src/` into your app, render `<ReportEditor />` inside your authenticated shell, and wire
the open button to `@tauri-apps/plugin-dialog`:

```tsx
import { open } from "@tauri-apps/plugin-dialog";
import { ReportEditor } from "./crpt/ReportEditor";

<ReportEditor onOpen={async () => {
  const f = await open({ filters: [{ name: "Crystal Reports", extensions: ["rpt"] }] });
  return typeof f === "string" ? f : null;
}} />
```

(Adjust the `open` flow to your wiring — `ReportEditor` currently takes a demo path.)

## Data-flow contract

- The backend is the source of truth: `apply_edits` returns the **refreshed IR**; the UI replaces
  its state with it. Never patch the IR client-side.
- Object ids (`o12`) are stable across edit batches; after a batch, use the ids from the returned
  IR (they are the same unless a future op type adds/removes records).
- Pending edits in the inspector are keyed per field — editing the same field again replaces the
  pending op; **Apply** sends the whole batch, and a refusal (e.g. multi-run text, missing
  DEVMODE) rejects with a user-showable message while earlier ops in the batch stand.

## v1 scope notes

- Text editing is single-run only; objects with embedded field references are shown read-only
  (the IR's `text.editable` flag is `false` for them).
- Orientation/paper-size edits require the report to carry a page-DEVMODE record (saved with a
  printer attached); the backend refuses with an explanation otherwise.
- `LayoutPreview` is a *layout* preview (positions/fonts/colors from the IR), not a data render.
  `PdfPreview` is that other half — the real render: call `previewReport()` after a save/apply
  and hand the result to `<PdfPreview preview={…} />`. It renders whatever the report carries:
  saved data drives the detail rows; a report saved without data shows its static bands, with
  the pipeline's fidelity warnings in a collapsible strip.
- `previewReportLive(db)` is the fresh-data variant: pass your app's SQL Server connection
  string (ADO/OLE DB form) and the report's parameter values; the report's stored procedures
  execute live, so the preview reflects the database as it stands. The connection string is
  parsed once and never logged; failures reject with a message that never contains the
  password.
