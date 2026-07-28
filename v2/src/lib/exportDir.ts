// Where the last export was saved.
//
// The save dialog always asks - that has never been in question - but it
// asked from wherever the OS last happened to be, so a second export
// started somewhere unrelated to the first. Remembering the folder means
// the dialog opens where the work actually lives, with the filename
// pre-filled, and the user still chooses.

const KEY = "tcm-v2-export-dir";

function dirOf(path: string): string {
  const cut = Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/"));
  return cut > 0 ? path.slice(0, cut) : "";
}

export function rememberExportPath(path: string): void {
  const dir = dirOf(path);
  if (!dir) return;
  try {
    localStorage.setItem(KEY, dir);
  } catch {
    // storage unavailable -> the dialog just opens wherever it likes
  }
}

/** `defaultPath` for the save dialog: the remembered folder joined to the
 * suggested filename, or the bare filename when nothing is remembered. */
export function exportPathFor(fileName: string): string {
  let dir = "";
  try {
    dir = localStorage.getItem(KEY) ?? "";
  } catch {
    dir = "";
  }
  if (!dir) return fileName;
  const sep = dir.includes("\\") ? "\\" : "/";
  return `${dir}${sep}${fileName}`;
}
