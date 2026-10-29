/**
 * Thin wrappers over the Tauri commands in Out/tauri-commands/commands.rs.
 * Every call returns the refreshed IR — keep it in one React state and render from it.
 */
import { invoke } from "@tauri-apps/api/core";
import type { EditOp, HitsOutput, HitBox, Ir, LiveDb, NewReportOptions, ParameterInfo, PdfExportOptions, PreviewOutput } from "./types";

export async function openReport(path: string): Promise<Ir> {
  return invoke<Ir>("open_report", { path });
}

export async function getIr(): Promise<Ir> {
  return invoke<Ir>("get_ir");
}

/** Apply a batch of edits; resolves to the refreshed IR, rejects with the refusal message. */
export async function applyEdits(ops: EditOp[]): Promise<Ir> {
  return invoke<Ir>("apply_edits", { ops });
}

/** Save to the opened path (or pass an explicit one). */
export async function saveReport(path?: string): Promise<void> {
  return invoke<void>("save_report", { path: path ?? null });
}

/** Apply a VibeyReports LayoutPlan JSON to the open report via the apply_plan command. */
export async function applyPlan(planJson: string): Promise<Ir> {
  return invoke<Ir>("apply_plan", { planJson });
}

/** Create a blank .rpt from scratch (no donor file); returns its bytes. */
export async function newReport(opts: NewReportOptions): Promise<number[]> {
  return invoke<number[]>("new_report", { opts });
}

/**
 * Render the open report (unsaved edits included) to a PDF preview. Saved data drives the
 * detail rows when the report carries it; a report saved without data renders its static
 * bands. Rejects only when no report is open or the writer fails — fidelity problems come
 * back in `diagnostics` instead.
 */
export async function previewReport(): Promise<PreviewOutput> {
  return invoke<PreviewOutput>("preview_report");
}

/** Page-1-only one-page PDF of the open report — the list-view thumbnail. */
export async function previewThumbnail(): Promise<PreviewOutput> {
  return invoke<PreviewOutput>("preview_thumbnail");
}

/** Open a report held in memory (e.g. straight from newReport) — no temp files. */
export async function openReportBytes(bytes: number[]): Promise<Ir> {
  return invoke<Ir>("open_report_bytes", { bytes });
}

/** Step back one accepted edit batch; resolves to the refreshed IR. */
export async function undoReport(): Promise<Ir> {
  return invoke<Ir>("undo_report");
}

/** Re-apply the last undone edit; resolves to the refreshed IR. */
export async function redoReport(): Promise<Ir> {
  return invoke<Ir>("redo_report");
}

/** Find & replace across the main report and every subreport (one undoable batch). */
export async function findReplace(find: string, replace: string, caseInsensitive = false): Promise<FindReplaceResult> {
  return invoke<FindReplaceResult>("find_replace", { find, replace, caseInsensitive });
}

/** The render-geometry bridge: every page's selectable hit boxes (overlay for click-to-select / drag-to-edit). */
export async function previewHits(): Promise<HitsOutput> {
  return invoke<HitsOutput>("preview_hits");
}

/** The topmost hit box at a printable-relative point on a page (server-side click lookup). */
export async function hitTest(page: number, x: number, y: number): Promise<HitBox | null> {
  return invoke<HitBox | null>("hit_test", { page, x, y });
}

/** "What changed": JSON-path diff lines over two reports' decoded models (empty = identical). */
export async function diffReports(a: number[], b: number[]): Promise<string[]> {
  return invoke<string[]>("diff_reports", { a, b });
}

/** The named embedded subreport's OWN IR — the object scope for `subreport`-scoped plan ops. */
export async function getSubreportIr(name: string): Promise<Ir> {
  return invoke<Ir>("get_subreport_ir", { name });
}

/** The open report's declared parameters — the prompt form for previews. */
export async function getParameters(): Promise<ParameterInfo[]> {
  return invoke<ParameterInfo[]>("get_parameters");
}

/** "Export PDF": the preview render with title / PDF-A level-b / tagging options. */
export async function exportPdf(opts: PdfExportOptions): Promise<PreviewOutput> {
  return invoke<PreviewOutput>("export_pdf", { opts });
}

/**
 * Live-data preview: your app's SQL Server connection string (ADO/OLE DB form) drives the
 * fetch — stored procedures run fresh, so the preview reflects the database as it stands.
 * Pass the parameter values the report prompts for. Rejects if no report is open, the
 * connection fails, or the PDF writer fails; the password never appears in an error.
 */
export async function previewReportLive(db: LiveDb): Promise<PreviewOutput> {
  return invoke<PreviewOutput>("preview_report_live", { db });
}
