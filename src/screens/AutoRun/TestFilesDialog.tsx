// A project's Test files: the documents its Auto Test scripts and API
// templates upload into the application, by name. They live on this
// machine only, in the project's own folder (`src-tauri/src/test_files.rs`);
// Rust copies a picked file in, lists the folder and removes a file - a
// local delete, nothing else. Opened from Auto Test's Setup card and from
// the API Templates tab, which share the query key below, so one updates
// the other.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { useId, useMemo, useState, type ComponentType } from "react";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { MODAL_LARGE } from "./modalWidths";
import {
  IconAddFiles,
  IconBrowse,
  IconCancel,
  IconConfirm,
  IconFileArchive,
  IconFileData,
  IconFileDocument,
  IconFileImage,
  IconFileOther,
  IconFileSpreadsheet,
  IconRemove,
} from "../../lib/actionIcons";
import { fileName } from "../../lib/fileSync";
import { unwrapStr } from "../../lib/ipc";

/** The query key every view of a project's Test files shares. */
export const testFilesKey = (org: string, project: string) => ["test-files", org, project];

/** The list, for the dialog and the rows that report a count. Plain local
 * files read through Rust, so no persistent cache: a copy on disk could only
 * be staler than the folder. */
export function useTestFiles(org: string, project: string) {
  return useQuery({
    queryKey: testFilesKey(org, project),
    queryFn: async () => (await unwrapStr(commands.testFilesList(org, project))) ?? [],
    enabled: Boolean(org && project),
    retry: false,
  });
}

/** A size as a person says it - the same wording Rust's run reports use. */
export function fileSize(bytes: number): string {
  if (bytes === 1) return "1 byte";
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** The kinds the list is grouped by, in the order they are shown, with the
 * extensions (lower case) that decide a file's kind. Anything else, and a
 * name with no extension, is Other. */
const KINDS: {
  name: string;
  extensions: string[];
  icon: ComponentType<{ className?: string; "aria-hidden"?: boolean }>;
}[] = [
  { name: "Spreadsheets", extensions: ["xlsx", "xls", "xlsm", "csv", "tsv", "ods"], icon: IconFileSpreadsheet },
  { name: "Documents", extensions: ["pdf", "doc", "docx", "txt", "rtf", "odt", "md"], icon: IconFileDocument },
  {
    name: "Images",
    extensions: ["png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "tif", "tiff"],
    icon: IconFileImage,
  },
  { name: "Data", extensions: ["json", "xml", "yaml", "yml"], icon: IconFileData },
  { name: "Archives", extensions: ["zip", "7z", "rar", "gz", "tar"], icon: IconFileArchive },
  { name: "Other", extensions: [], icon: IconFileOther },
];

/** The index into KINDS for a file name, by its extension (any case). */
export function fileKindIndex(name: string): number {
  const dot = name.lastIndexOf(".");
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : "";
  const i = KINDS.findIndex((k) => k.extensions.includes(ext));
  return i === -1 ? KINDS.length - 1 : i;
}

/** The files grouped by kind in KINDS' order, each group sorted by name
 * without regard to case; a kind with no files is left out. */
export function groupTestFiles<T extends { name: string }>(files: T[]) {
  return KINDS.map((kind, i) => ({
    kind,
    files: files
      .filter((f) => fileKindIndex(f.name) === i)
      .sort((a, b) => {
        const x = a.name.toLowerCase();
        const y = b.name.toLowerCase();
        return x < y ? -1 : x > y ? 1 : 0;
      }),
  })).filter((g) => g.files.length > 0);
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** What Rust says when a file of that name is already there - asked about
 * like a clash the list already showed, should the list have been stale. */
const ALREADY_THERE = "is already in Test files";

export default function TestFilesDialog({
  org,
  project,
  onClose,
}: {
  org: string;
  project: string;
  onClose: () => void;
}) {
  const headingId = useId();
  const qc = useQueryClient();
  const files = useTestFiles(org, project);
  const [problems, setProblems] = useState<string[]>([]);
  // Picked files whose name is already in Test files, asked about one at a
  // time.
  const [clashes, setClashes] = useState<{ path: string; name: string }[]>([]);
  // The file whose Remove is waiting for its confirm.
  const [removing, setRemoving] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = () => qc.invalidateQueries({ queryKey: testFilesKey(org, project) });

  const addFiles = async () => {
    setProblems([]);
    let picked: string | string[] | null;
    try {
      picked = await open({ multiple: true });
    } catch (e) {
      setProblems([message(e)]);
      return;
    }
    const paths = picked == null ? [] : Array.isArray(picked) ? picked : [picked];
    if (paths.length === 0) return;
    setBusy(true);
    const have = new Set((files.data ?? []).map((f) => f.name.toLowerCase()));
    const asking: { path: string; name: string }[] = [];
    const failed: string[] = [];
    for (const path of paths) {
      const name = fileName(path);
      if (have.has(name.toLowerCase())) {
        asking.push({ path, name });
        continue;
      }
      try {
        await unwrapStr(commands.testFilesAdd(org, project, path, false));
        have.add(name.toLowerCase());
      } catch (e) {
        if (message(e).includes(ALREADY_THERE)) asking.push({ path, name });
        else failed.push(message(e));
      }
    }
    setBusy(false);
    setProblems(failed);
    setClashes(asking);
    await refresh();
  };

  // The person's answer to the first clash waiting.
  const answer = async (replace: boolean) => {
    const [first, ...rest] = clashes;
    if (!first) return;
    if (replace) {
      setBusy(true);
      try {
        await unwrapStr(commands.testFilesAdd(org, project, first.path, true));
      } catch (e) {
        setProblems((p) => [...p, message(e)]);
      }
      setBusy(false);
      await refresh();
    }
    setClashes(rest);
  };

  const remove = async (name: string) => {
    setProblems([]);
    setBusy(true);
    try {
      await unwrapStr(commands.testFilesRemove(org, project, name));
    } catch (e) {
      setProblems([message(e)]);
    }
    setBusy(false);
    setRemoving(null);
    await refresh();
  };

  const openFolder = async () => {
    setProblems([]);
    try {
      await unwrapStr(commands.testFilesOpenFolder(org, project));
    } catch (e) {
      setProblems([message(e)]);
    }
  };

  const list = files.data ?? [];
  const groups = useMemo(() => groupTestFiles(files.data ?? []), [files.data]);
  const asking = clashes[0];

  return (
    <Modal onClose={onClose} labelledBy={headingId} className={`${MODAL_LARGE} flex flex-col gap-3 p-5`}>
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Test files
      </h2>
      <p className="text-xs text-muted">
        Scripts and API templates upload these by name. They stay on this machine.
      </p>

      {files.isLoading && <p className="text-xs text-muted">Loading…</p>}
      {files.isError && <p className="text-xs text-danger">{files.error.message}</p>}
      {files.isSuccess && list.length === 0 && (
        <p className="text-sm text-muted">No test files yet - add the documents your tests upload.</p>
      )}
      {list.length > 0 && (
        // One labelled wrapper, then one labelled group per kind. Groups are
        // told apart by the gap between them, wider than the one between rows.
        <div role="group" aria-label="Test files" className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto">
          {groups.map(({ kind, files: inGroup }) => (
            <div key={kind.name} role="group" aria-label={kind.name}>
              <h3 className="pb-1 text-xs text-muted">
                {kind.name} · {inGroup.length}
              </h3>
              <ul className="divide-y divide-border/60">
                {inGroup.map((f) => (
                  <li key={f.name} className="flex items-center gap-3 py-2">
                    <kind.icon aria-hidden className="size-4 shrink-0 text-faint" />
                    <span className="min-w-0 flex-1 truncate text-sm text-text" title={f.name}>
                      {f.name}
                    </span>
                    <span className="w-20 shrink-0 text-right text-xs tabular-nums text-faint">
                      {fileSize(f.size)}
                    </span>
                    {removing === f.name ? (
                      <span role="group" aria-label={`Remove ${f.name}?`} className="flex shrink-0 items-center gap-2">
                        <span className="text-xs text-muted">Remove {f.name}?</span>
                        <Button size="sm" variant="ghost" disabled={busy} onClick={() => setRemoving(null)}>
                          <IconCancel aria-hidden />
                          Keep
                        </Button>
                        <Button size="sm" variant="danger" disabled={busy} onClick={() => void remove(f.name)}>
                          <IconRemove aria-hidden />
                          Remove
                        </Button>
                      </span>
                    ) : (
                      <Button
                        size="sm"
                        variant="ghost"
                        aria-label={`Remove ${f.name}`}
                        disabled={busy}
                        onClick={() => setRemoving(f.name)}
                      >
                        <IconRemove aria-hidden />
                        Remove
                      </Button>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      )}

      {asking && (
        <div
          role="group"
          aria-label={`Replace ${asking.name}?`}
          className="flex flex-wrap items-center gap-2 rounded-md border border-warning/40 bg-warning/10 p-3"
        >
          <span className="min-w-0 flex-1 text-sm text-text">
            Replace {asking.name}? A file of that name is already in Test files.
          </span>
          <Button size="sm" variant="ghost" disabled={busy} onClick={() => void answer(false)}>
            <IconCancel aria-hidden />
            Keep the old one
          </Button>
          <Button size="sm" disabled={busy} onClick={() => void answer(true)}>
            <IconConfirm aria-hidden />
            Replace
          </Button>
        </div>
      )}

      {problems.map((p, i) => (
        <p key={`${i}-${p}`} className="text-xs text-danger">
          {p}
        </p>
      ))}

      <div className="flex flex-wrap justify-end gap-2">
        <Button size="sm" variant="outline" onClick={() => void openFolder()}>
          <IconBrowse aria-hidden />
          Open folder
        </Button>
        <Button size="sm" variant="outline" disabled={busy || Boolean(asking)} onClick={() => void addFiles()}>
          <IconAddFiles aria-hidden />
          Add files
        </Button>
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}
