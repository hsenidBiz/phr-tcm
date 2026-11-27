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
import { commands, type Action, type CaseScript } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Modal } from "../../components/ui/modal";
import { SearchInput } from "../../components/ui/search-input";
import { MODAL_LARGE } from "./modalWidths";
import {
  IconAddFiles,
  IconBrowse,
  IconCancel,
  IconCollapseAll,
  IconConfirm,
  IconExpandAll,
  IconFileArchive,
  IconFileData,
  IconFileDocument,
  IconFileImage,
  IconFileOther,
  IconFileSpreadsheet,
  IconHideDetails,
  IconRemove,
  IconShowDetails,
} from "../../lib/actionIcons";
import { fileName } from "../../lib/fileSync";
import { unwrapStr } from "../../lib/ipc";
import { plural } from "../../lib/plural";
import { scriptFiles } from "./CaseCard";

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

/** The text values a script hands to its components as inputs, nested
 * actions included: a file name passed that way is a use of the file. */
function componentInputs(script: CaseScript): string[] {
  const out: string[] = [];
  const walk = (list: readonly Action[]) => {
    for (const a of list) {
      if (a.kind === "use_component") {
        for (const v of Object.values(a.inputs ?? {})) if (typeof v === "string") out.push(v);
      }
      if (a.kind === "when_visible" || a.kind === "when_hidden" || a.kind === "repeat_while_visible") walk(a.then);
    }
  };
  for (const step of script.steps) walk(step.actions);
  return out;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** What Rust says when a file of that name is already there - asked about
 * like a clash the list already showed, should the list have been stale. */
const ALREADY_THERE = "is already in Test files";

export default function TestFilesDialog({
  org,
  project,
  scripts,
  onClose,
}: {
  org: string;
  project: string;
  /** The listed cases' scripts, as Auto Test's own queries hold them
   * (`["autorun-script", id]`): `undefined` for one not read yet or that
   * could not be. Left out where no cases are listed (API Templates), the
   * rows then say nothing about use. */
  scripts?: (CaseScript | null | undefined)[];
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
  const [query, setQuery] = useState("");
  // Groups the person opened or closed; a kind not here follows the default.
  const [folded, setFolded] = useState<Record<string, boolean>>({});

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
  const needle = query.trim().toLowerCase();
  const searching = needle !== "";
  const groups = useMemo(() => {
    const all = groupTestFiles(files.data ?? []);
    if (!needle) return all;
    return all
      .map((g) => ({
        ...g,
        files: g.files.filter((f) => f.name.toLowerCase().includes(needle)),
      }))
      .filter((g) => g.files.length > 0);
  }, [files.data, needle]);
  // Closed to start with, unless there is only one group to look at. A
  // search opens every group it matches in.
  const isOpen = (kind: string) => searching || (folded[kind] ?? groups.length === 1);
  const allOpen = groups.every((g) => isOpen(g.kind.name));
  const setAll = (open: boolean) => {
    setRemoving(null);
    setFolded(Object.fromEntries(groups.map((g) => [g.kind.name, open])));
  };

  // How many listed cases upload each file, by lower-cased name (Windows
  // names are not case-sensitive); a case that passes the name to a
  // component as an input counts too. Null unless every listed case's script
  // has been read: a count from some of them would be a wrong one.
  const usage = useMemo(() => {
    if (!scripts || scripts.length === 0 || scripts.some((sc) => sc === undefined)) return null;
    const counts = new Map<string, number>();
    for (const sc of scripts) {
      if (!sc) continue;
      const named = [...scriptFiles(sc).uploads, ...componentInputs(sc)];
      for (const f of new Set(named.map((u) => u.toLowerCase()))) {
        counts.set(f, (counts.get(f) ?? 0) + 1);
      }
    }
    return counts;
  }, [scripts]);
  const asking = clashes[0];

  return (
    <Modal onClose={onClose} labelledBy={headingId} className={`${MODAL_LARGE} flex flex-col gap-3 p-5`}>
      <h2 id={headingId} className="text-sm font-semibold text-text">
        Test files
      </h2>
      <p className="text-xs text-muted">Scripts and API templates upload these by name. They stay on this machine.</p>

      {files.isLoading && <p className="text-xs text-muted">Loading…</p>}
      {files.isError && <p className="text-xs text-danger">{files.error.message}</p>}
      {files.isSuccess && list.length === 0 && (
        <p className="text-sm text-muted">No test files yet - add the documents your tests upload.</p>
      )}
      {list.length > 0 && (
        <SearchInput
          aria-label="Search test files"
          placeholder="Search test files"
          value={query}
          onChange={(text) => {
            setRemoving(null);
            setQuery(text);
          }}
        />
      )}
      {list.length > 0 && searching && groups.length === 0 && (
        <p className="text-sm text-muted">No files match "{query.trim()}"</p>
      )}
      {groups.length > 0 && (
        // One labelled wrapper, then one labelled group per kind. Groups are
        // told apart by the gap between them, wider than the one between rows.
        <div role="group" aria-label="Test files" className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto">
          {groups.map(({ kind, files: inGroup }) => {
            const open = isOpen(kind.name);
            const listId = `${headingId}-${kind.name}`;
            return (
              <div key={kind.name} role="group" aria-label={kind.name}>
                <h3 className="text-xs text-muted">
                  <button
                    type="button"
                    aria-expanded={open}
                    aria-controls={listId}
                    disabled={searching}
                    onClick={() => {
                      setRemoving(null);
                      setFolded((f) => ({ ...f, [kind.name]: !open }));
                    }}
                    className="flex w-full items-center gap-1.5 rounded py-1 text-left hover:text-text focus-visible:outline-2 focus-visible:outline-accent disabled:cursor-default"
                  >
                    {open ? (
                      <IconHideDetails aria-hidden className="size-3.5 shrink-0" />
                    ) : (
                      <IconShowDetails aria-hidden className="size-3.5 shrink-0" />
                    )}
                    {kind.name} · {inGroup.length}
                  </button>
                </h3>
                <ul id={listId} hidden={!open} className="divide-y divide-border/60">
                  {inGroup.map((f) => {
                    const uses = usage?.get(f.name.toLowerCase()) ?? 0;
                    return (
                      <li key={f.name} className="flex flex-wrap items-center gap-x-3 gap-y-1 py-1.5">
                        <kind.icon aria-hidden className="size-4 shrink-0 text-faint" />
                        <span className="min-w-0 flex-1 basis-40 truncate text-sm text-text" title={f.name}>
                          {f.name}
                        </span>
                        {/* Only what the listed cases say: a file none of them uploads may
                              still be used by another PBI, suite or environment, an API
                              template, or a component, so it is never called unused. */}
                        {uses > 0 && <Badge title="Of the cases listed on Auto Test">{plural(uses, "case")}</Badge>}
                        <span className="w-20 shrink-0 text-right text-xs tabular-nums text-faint">
                          {fileSize(f.size)}
                        </span>
                        {removing === f.name ? (
                          <span
                            role="group"
                            aria-label={`Remove ${f.name}?`}
                            className="flex basis-full items-center justify-end gap-2"
                          >
                            <span className="min-w-0 break-all text-xs text-muted">
                              {uses
                                ? `${plural(uses, "listed case")} upload${uses === 1 ? "s" : ""} this file. Remove it anyway?`
                                : `Remove ${f.name}?`}
                            </span>
                            <Button size="sm" variant="ghost" disabled={busy} onClick={() => setRemoving(null)}>
                              <IconCancel aria-hidden />
                              Keep
                            </Button>
                            <Button size="sm" variant="danger" disabled={busy} onClick={() => void remove(f.name)}>
                              <IconRemove aria-hidden />
                              {uses ? "Remove file" : "Remove"}
                            </Button>
                          </span>
                        ) : (
                          <Button
                            size="sm"
                            variant="ghost"
                            className="px-2"
                            aria-label={`Remove ${f.name}`}
                            title={`Remove ${f.name}`}
                            disabled={busy}
                            onClick={() => setRemoving(f.name)}
                          >
                            <IconRemove aria-hidden />
                          </Button>
                        )}
                      </li>
                    );
                  })}
                </ul>
              </div>
            );
          })}
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

      <div className="flex flex-wrap items-center justify-between gap-2">
        {groups.length > 1 ? (
          <Button size="sm" variant="ghost" disabled={searching} onClick={() => setAll(!allOpen)}>
            {allOpen ? <IconCollapseAll aria-hidden /> : <IconExpandAll aria-hidden />}
            {allOpen ? "Collapse all" : "Expand all"}
          </Button>
        ) : (
          <span />
        )}
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
      </div>
    </Modal>
  );
}
