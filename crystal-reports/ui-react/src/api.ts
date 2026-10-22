/**
 * Thin wrappers over the Tauri commands in Out/tauri-commands/commands.rs.
 * Every call returns the refreshed IR — keep it in one React state and render from it.
 */
import { invoke } from "@tauri-apps/api/core";
import type { EditOp, Ir, LiveDb, NewReportOptions, PreviewOutput } from "./types";

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

/**
 * Live-data preview: your app's SQL Server connection string (ADO/OLE DB form) drives the
 * fetch — stored procedures run fresh, so the preview reflects the database as it stands.
 * Pass the parameter values the report prompts for. Rejects if no report is open, the
 * connection fails, or the PDF writer fails; the password never appears in an error.
 */
export async function previewReportLive(db: LiveDb): Promise<PreviewOutput> {
  return invoke<PreviewOutput>("preview_report_live", { db });
}
