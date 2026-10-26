// The Overview's folder tree: what sits under the clone's suites folder, one
// level per folder (any depth), each feature beneath its folder with its
// spec and test case counts. Anything with something under it (a folder, or
// a feature that holds features) shows the totals of everything under it,
// and folds like a branch of the Runner's tree; each level hangs from the
// same guide line (RunnerTree's NESTED). A nested list, as the Runner's is:
// a screen reader hears each level, and the fold buttons are in the Tab
// order, each named by its folder's full path (two folders can share a
// name) with aria-expanded saying whether it is open.
//
// Given a clone, each folder row also has a right-click menu (Shift+F10 or
// the context-menu key on a focused row), as a code editor's file tree has:
// New folder puts a name box under the row, Rename turns the row's name into
// one. Rename asks Rust what it would break first (pw_repo_rename_folder_check)
// and lists that for confirming; the app never rewrites the repo's files.
//
// The empty area under the rows (or the hint, when the clone has no features
// yet) has the same menu with just New feature folder, which makes a folder
// at the top, directly under suites. That is the only way to a first folder
// or a top-level one, since the tree has no toolbar button for it.

import { useEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { commands, type RenameCheck, type RepoStructure } from "../../bindings";
import ContextMenu from "../../components/ContextMenu";
import { Input } from "../../components/ui/input";
import {
  IconAdd,
  IconHideDetails,
  IconRename,
  IconRepoFeature,
  IconRepoFolder,
  IconShowDetails,
} from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";
import RenameFolderDialog from "./RenameFolderDialog";
import { Hint } from "./parts";
import { NESTED } from "./RunnerTree";

type Feature = RepoStructure["features"][number];

/** What a feature that is the suites folder itself (no folders under
 * suites) is called, at the root. */
export const SUITES = "suites";

export type FolderNode = {
  /** The path under suites, `/`-joined ("" for suites itself). */
  id: string;
  name: string;
  /** Set when this folder is a feature. */
  feature: Feature | null;
  children: FolderNode[];
  /** Specs and test cases at or below this folder. */
  specs: number;
  cases: number;
};

/** The features, in their order, as folders nested under their parents. */
export function buildFolders(features: Feature[]): FolderNode[] {
  const root: FolderNode = { id: "", name: "", feature: null, children: [], specs: 0, cases: 0 };
  for (const f of features) {
    if (f.path.length === 0) {
      root.children.push({ id: "", name: SUITES, feature: f, children: [], specs: f.spec_count, cases: f.case_count });
      continue;
    }
    let at = root;
    f.path.forEach((name, i) => {
      const id = f.path.slice(0, i + 1).join("/");
      let next = at.children.find((c) => c.id === id);
      if (!next) {
        next = { id, name, feature: null, children: [], specs: 0, cases: 0 };
        at.children.push(next);
      }
      next.specs += f.spec_count;
      next.cases += f.case_count;
      at = next;
    });
    at.feature = f;
  }
  return root.children;
}

const plural = (n: number, one: string, many: string) => (n === 1 ? one : many);

/** A count in its column; a screen reader hears what it counts. */
function Count({ n, one, many, className }: { n: number; one: string; many: string; className: string }) {
  return (
    <span className={cn("w-14 shrink-0 text-right text-xs tabular-nums", className)}>
      {n}
      <span className="sr-only">{` ${plural(n, one, many)}`}</span>
    </span>
  );
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The folder a path sits in ("" at the top). */
const parentOf = (id: string) => id.slice(0, Math.max(0, id.lastIndexOf("/")));
const joinPath = (parent: string, name: string) => (parent ? `${parent}/${name}` : name);

/**
 * The inline name box of New folder and Rename: focused (with the current
 * name selected) on appearing. Enter hands the name on and Escape cancels;
 * leaving it empty or unchanged cancels too, while a name typed and left
 * stays.
 */
function NameBox({
  label,
  initial,
  hold,
  busy,
  onSubmit,
  onCancel,
}: {
  label: string;
  initial: string;
  /** A dialog over it has the focus: leaving the box is not a cancel. */
  hold: boolean;
  /** A check or rename is running: the box waits, and Enter does nothing. */
  busy: boolean;
  onSubmit: (name: string) => void;
  /** `viaKey`: Escape (the focus goes back to the row) rather than a blur. */
  onCancel: (viaKey: boolean) => void;
}) {
  const [value, setValue] = useState(initial);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    field.current?.focus();
    field.current?.select();
  }, []);
  // A box that waited takes the focus back, to be edited again after a refusal.
  useEffect(() => {
    if (!busy) field.current?.focus();
  }, [busy]);
  return (
    <Input
      ref={field}
      aria-label={label}
      className="h-6 min-w-0 flex-1 px-1.5 py-0 text-sm"
      value={value}
      disabled={busy}
      spellCheck={false}
      onChange={(e) => setValue(e.target.value)}
      // The field keeps its own copy and paste menu.
      onContextMenu={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === "Enter") {
          e.preventDefault();
          if (!busy) onSubmit(value.trim());
        } else if (e.key === "Escape") {
          e.preventDefault();
          if (!busy) onCancel(true);
        }
      }}
      onBlur={() => {
        const v = value.trim();
        if (!hold && !busy && (v === "" || v === initial)) onCancel(false);
      }}
    />
  );
}

export default function FeatureTree({
  features,
  clone = "",
  onChanged,
}: {
  features: Feature[];
  /** The clone the folders are in. Without it the rows have no menu. */
  clone?: string;
  /** A folder was made or renamed: read the clone again. */
  onChanged?: () => void;
}) {
  const [folded, setFolded] = useState<Set<string>>(() => new Set());
  /** `id` "" is the top, under suites. */
  const [menu, setMenu] = useState<{ id: string; x: number; y: number; row: HTMLElement | null } | null>(null);
  /** The folder a new one is being named under ("" is the top, under
   * suites), and the one being renamed. */
  const [creating, setCreating] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<RenameCheck | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const editable = clone !== "";
  const tree = useRef<HTMLDivElement>(null);
  /** The row to give the focus to once it is on screen (a rename's new id
   * only appears when the caller has read the clone again). */
  const pendingFocus = useRef<string | null>(null);
  const focusRow = (id: string) => {
    pendingFocus.current = id;
    setTimeout(() => {
      if (pendingFocus.current === id) pendingFocus.current = null;
    }, 3000);
  };
  useEffect(() => {
    const id = pendingFocus.current;
    if (id === null) return;
    const el = [...(tree.current?.querySelectorAll<HTMLElement>("[data-folder]") ?? [])].find(
      (e) => e.dataset.folder === id,
    );
    if (el) {
      pendingFocus.current = null;
      el.focus();
    }
  });

  const toggle = (id: string) =>
    setFolded((f) => {
      const next = new Set(f);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const unfold = (id: string) =>
    setFolded((f) => {
      if (!f.has(id)) return f;
      const next = new Set(f);
      next.delete(id);
      return next;
    });
  const stopEditing = () => {
    setCreating(null);
    setRenaming(null);
    setConfirm(null);
    setError("");
  };

  const create = async (parent: string, name: string) => {
    if (busy) return;
    if (!name) return stopEditing();
    setBusy(true);
    setError("");
    try {
      await unwrapStr(commands.pwRepoCreateFolder(clone, joinPath(parent, name)));
      focusRow(joinPath(parent, name));
      stopEditing();
      onChanged?.();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  /** Renames for real. The folds follow the folder to its new path. */
  const rename = async (check: RenameCheck, confirmed: boolean) => {
    if (busy && confirmed) return;
    setBusy(true);
    setError("");
    try {
      const done = await unwrapStr(commands.pwRepoRenameFolder(clone, check.from, check.to, confirmed));
      // The folder moved, but some of the app's own settings could not follow:
      // the sentence says which, and outlives the box and the dialog.
      if (done.warning) toast.warning(done.warning, { duration: 10_000 });
      focusRow(check.to);
      setFolded((f) => {
        const next = new Set<string>();
        for (const id of f) {
          const inside = id === check.from || id.startsWith(`${check.from}/`);
          next.add(inside ? check.to + id.slice(check.from.length) : id);
        }
        return next;
      });
      stopEditing();
      onChanged?.();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  /** Enter in the rename box: ask what it would break, and go ahead at once
   * when it breaks nothing. */
  const askRename = async (from: string, name: string, current: string) => {
    if (busy) return;
    if (!name || name === current) return stopEditing();
    if (/[\\/]/.test(name)) {
      setError("A name, not a path.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      const check = await unwrapStr(commands.pwRepoRenameFolderCheck(clone, from, joinPath(parentOf(from), name)));
      if (check.breaks.length > 0) {
        setConfirm(check);
        setBusy(false);
        return;
      }
      setBusy(false);
      await rename(check, false);
    } catch (e) {
      setError(message(e));
      setBusy(false);
    }
  };

  const row = (n: FolderNode) => {
    const shut = folded.has(n.id);
    const isFeature = n.feature !== null;
    /** A feature on its own shows its counts; anything with something under
     * it shows the totals, faint. */
    const leaf = isFeature && n.children.length === 0;
    const Glyph = isFeature ? IconRepoFeature : IconRepoFolder;
    // The suites folder itself (a feature with no folders above it) is not
    // a folder the menu can act on.
    const menuRow = editable && n.id !== "";
    const isRenaming = renaming === n.id && menuRow;
    const isCreating = creating === n.id && menuRow;
    const openMenu = (x: number, y: number, row: HTMLElement) => setMenu({ id: n.id, x, y, row });
    /** Escape in a name box: back to the row it came from. */
    const cancelBox = (viaKey: boolean) => {
      if (viaKey) focusRow(n.id);
      stopEditing();
    };
    const menuProps = menuRow
      ? {
          role: "group",
          tabIndex: 0,
          "data-folder": n.id,
          "aria-label": `Folder ${n.id}`,
          "aria-keyshortcuts": "Shift+F10",
          onContextMenu: (e: MouseEvent<HTMLDivElement>) => {
            e.preventDefault();
            openMenu(e.clientX, e.clientY, e.currentTarget);
          },
          onKeyDown: (e: KeyboardEvent<HTMLDivElement>) => {
            if (e.key !== "ContextMenu" && !(e.shiftKey && e.key === "F10")) return;
            e.preventDefault();
            const r = e.currentTarget.getBoundingClientRect();
            openMenu(r.left + 28, r.bottom - 4, e.currentTarget);
          },
        }
      : {};
    return (
      <li key={n.id}>
        <div
          {...menuProps}
          className={cn(
            "flex items-center gap-1.5 rounded px-1 py-0.5 hover:bg-surface-2",
            menuRow && "focus-visible:bg-surface-2 focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent",
          )}
        >
          {n.children.length > 0 ? (
            <button
              type="button"
              className="flex size-5 shrink-0 items-center justify-center text-muted transition-colors hover:text-accent [&_svg]:size-3.5"
              aria-label={n.id}
              aria-expanded={!shut}
              title={shut ? "Unfold" : "Fold"}
              onClick={() => toggle(n.id)}
            >
              {shut ? <IconShowDetails aria-hidden /> : <IconHideDetails aria-hidden />}
            </button>
          ) : (
            <span className="w-5 shrink-0" aria-hidden />
          )}
          <Glyph aria-hidden className={cn("size-4 shrink-0", isFeature ? "text-accent" : "text-muted")} />
          {isRenaming ? (
            <NameBox
              label={`New name for ${n.id}`}
              initial={n.name}
              hold={confirm !== null}
              busy={busy}
              onSubmit={(name) => void askRename(n.id, name, n.name)}
              onCancel={cancelBox}
            />
          ) : (
            <span
              className={cn("min-w-0 flex-1 truncate text-sm text-text", isFeature && "font-medium")}
              title={n.id || SUITES}
            >
              {n.name}
            </span>
          )}
          <Count n={n.specs} one="spec" many="specs" className={leaf ? "text-text" : "text-faint"} />
          <Count
            n={n.cases}
            one="test case"
            many="test cases"
            className={leaf && n.cases ? "text-text" : "text-faint"}
          />
        </div>
        {isRenaming && error && !confirm && <p className="ml-7 text-xs text-danger">{error}</p>}
        {((n.children.length > 0 && !shut) || isCreating) && (
          <ul className={NESTED}>
            {!shut && n.children.map(row)}
            {isCreating && (
              <li>
                <div className="flex items-center gap-1.5 rounded px-1 py-0.5">
                  <span className="w-5 shrink-0" aria-hidden />
                  <IconRepoFolder aria-hidden className="size-4 shrink-0 text-muted" />
                  <NameBox
                    label={`New folder name under ${n.id}`}
                    initial=""
                    hold={false}
                    busy={busy}
                    onSubmit={(name) => void create(n.id, name)}
                    onCancel={cancelBox}
                  />
                </div>
                {error && <p className="ml-7 text-xs text-danger">{error}</p>}
              </li>
            )}
          </ul>
        )}
      </li>
    );
  };

  /** The empty area under the rows: the menu for the top, by right-click or
   * the keyboard. Without a clone it is just the space. */
  const topProps = editable
    ? {
        role: "group",
        tabIndex: 0,
        "data-folder-top": "",
        "aria-label": "Top of suites",
        "aria-keyshortcuts": "Shift+F10",
        onContextMenu: (e: MouseEvent<HTMLDivElement>) => {
          e.preventDefault();
          setMenu({ id: "", x: e.clientX, y: e.clientY, row: e.currentTarget });
        },
        onKeyDown: (e: KeyboardEvent<HTMLDivElement>) => {
          if (e.target !== e.currentTarget) return;
          if (e.key !== "ContextMenu" && !(e.shiftKey && e.key === "F10")) return;
          e.preventDefault();
          const r = e.currentTarget.getBoundingClientRect();
          setMenu({ id: "", x: r.left + 28, y: r.top + 24, row: e.currentTarget });
        },
      }
    : {};
  const isCreatingTop = creating === "" && editable;
  /** Escape in the top-level name box: back to the empty area. */
  const cancelTopBox = (viaKey: boolean) => {
    if (viaKey) tree.current?.querySelector<HTMLElement>("[data-folder-top]")?.focus();
    stopEditing();
  };

  return (
    <div ref={tree} className="overflow-hidden rounded-lg border border-border bg-surface">
      {features.length > 0 && (
        <>
          <div aria-hidden className="flex items-center gap-1.5 border-b border-border bg-surface-2 px-3 py-2 text-xs font-medium text-muted">
            <span className="min-w-0 flex-1">Folder</span>
            <span className="w-14 shrink-0 text-right">Specs</span>
            <span className="w-14 shrink-0 text-right">Cases</span>
          </div>
          <ul aria-label="Features" className="p-2">
            {buildFolders(features).map(row)}
          </ul>
        </>
      )}
      <div
        {...topProps}
        className={cn(
          "min-h-10 px-3 py-2",
          editable && "focus-visible:bg-surface-2 focus-visible:outline focus-visible:outline-1 focus-visible:outline-accent",
        )}
      >
        {features.length === 0 && <Hint>No features under suites yet.</Hint>}
        {isCreatingTop && (
          <div className="mt-1">
            <div className="flex items-center gap-1.5 rounded px-1 py-0.5">
              <span className="w-5 shrink-0" aria-hidden />
              <IconRepoFolder aria-hidden className="size-4 shrink-0 text-muted" />
              <NameBox
                label="New folder name under suites"
                initial=""
                hold={false}
                busy={busy}
                onSubmit={(name) => void create("", name)}
                onCancel={cancelTopBox}
              />
            </div>
            {error && <p className="ml-7 text-xs text-danger">{error}</p>}
          </div>
        )}
      </div>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={`Actions for ${menu.id || SUITES}`}
          returnTo={menu.row}
          onClose={() => setMenu(null)}
          actions={[
            {
              label: "New feature folder",
              title: "Makes a feature folder, with specs and test-cases folders inside it.",
              icon: <IconAdd aria-hidden />,
              onSelect: () => {
                setRenaming(null);
                setError("");
                unfold(menu.id);
                setCreating(menu.id);
              },
            },
            // The top is not a folder: nothing there to rename.
            ...(menu.id === ""
              ? []
              : [
                  {
                    label: "Rename",
                    icon: <IconRename aria-hidden />,
                    onSelect: () => {
                      setCreating(null);
                      setError("");
                      setRenaming(menu.id);
                    },
                  },
                ]),
          ]}
        />
      )}
      {confirm && (
        <RenameFolderDialog
          check={confirm}
          busy={busy}
          error={error}
          onConfirm={() => void rename(confirm, true)}
          onCancel={() => {
            if (busy) return;
            setConfirm(null);
            setError("");
          }}
        />
      )}
    </div>
  );
}
