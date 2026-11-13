// Saving the files a Test Runner result carries.
//
// Two sources, one dialog: a file attached THIS session is already in the
// webview as base64 and Rust just writes it; a file already uploaded to the
// result is streamed by Rust from Azure DevOps straight to the chosen path,
// so its bytes (a screen recording can be hundreds of MB) never cross IPC.
import { save } from "@tauri-apps/plugin-dialog";
import { commands, type ResultAttachment, type RunAttachment } from "../bindings";
import { exportPathFor, rememberExportPath } from "./exportDir";
import { unwrap, unwrapStr } from "./ipc";
import { toast } from "./toast";

/** Save dialog seeded from the last export folder; remembers the folder.
 * null = cancelled. */
export async function pickSavePath(fileName: string): Promise<string | null> {
  const path = await save({ defaultPath: exportPathFor(fileName) });
  if (!path) return null;
  rememberExportPath(path);
  return path;
}

const reason = (e: unknown) => (e instanceof Error ? e.message : String(e));

async function saveTo(fileName: string, write: (path: string) => Promise<unknown>): Promise<boolean> {
  try {
    const path = await pickSavePath(fileName);
    if (!path) return false;
    await write(path);
    toast.success(`Saved ${fileName}`);
    return true;
  } catch (e) {
    toast.error(`Could not save ${fileName}: ${reason(e)}`);
    return false;
  }
}

/** A file attached this session: b64 -> Rust writes it. true when saved. */
export function downloadSessionFile(att: RunAttachment): Promise<boolean> {
  return saveTo(att.file_name, (path) => unwrapStr(commands.saveAttachmentB64(path, att.b64)));
}

/** An uploaded file: Rust streams it from Azure DevOps to the chosen path.
 * true when saved. */
export function downloadResultFile(
  org: string,
  project: string,
  runId: number,
  resultId: number,
  att: ResultAttachment,
): Promise<boolean> {
  return saveTo(att.file_name, (path) =>
    unwrap(commands.saveResultAttachment(org, project, runId, resultId, att.id, att.size_bytes, path)),
  );
}

export type ReadReport = { read: RunAttachment[]; failed: { name: string; why: string }[] };

const baseName = (path: string) => path.split(/[\/]/).filter(Boolean).pop() ?? path;

/** Reads each path through read_file_b64, one after another (several large
 * files at once would hold all their base64 in memory together); one
 * failure never stops the rest. */
export async function readFiles(paths: string[]): Promise<ReadReport> {
  const report: ReadReport = { read: [], failed: [] };
  for (const path of paths) {
    try {
      report.read.push(await unwrapStr(commands.readFileB64(path)));
    } catch (e) {
      report.failed.push({ name: baseName(path), why: reason(e) });
    }
  }
  return report;
}
