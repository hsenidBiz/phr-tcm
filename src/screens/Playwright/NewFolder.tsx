// New folder: makes a feature folder in the clone, with test-cases/ and
// specs/ each holding a .gitkeep so git keeps it (Rust's pw_repo::new_folder).
// In Export to Playwright, under Areas (the Overview makes folders from the
// tree's right-click menu instead); the caller reads the clone's folders
// again once one is made, so it is listed and offered at once.

import { useEffect, useMemo, useRef, useState } from "react";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import Combobox from "../../components/ui/combobox";
import { IconAdd, IconCancel, IconConfirm } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

export default function NewFolder({
  clone,
  paths,
  disabled = false,
  onCreated,
}: {
  clone: string;
  /** The clone's feature paths, offered as you type a new one beside them. */
  paths: string[];
  disabled?: boolean;
  onCreated: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [path, setPath] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [made, setMade] = useState("");
  const field = useRef<HTMLDivElement>(null);
  // Where the clone's features sit, once each, with the slash a new name follows.
  const parents = useMemo(() => [...new Set(paths.map((p) => `${p.split("/").slice(0, -1).join("/")}/`))], [paths]);

  // The keyboard starts in the path field, as the plain input it replaces did.
  useEffect(() => {
    if (open) field.current?.querySelector<HTMLElement>('[role="combobox"]')?.focus();
  }, [open]);

  const close = () => {
    setOpen(false);
    setPath("");
    setError("");
  };

  const create = async () => {
    setBusy(true);
    setError("");
    try {
      const done = await unwrapStr(commands.pwRepoCreateFolder(clone, path));
      setMade(done.path);
      close();
      onCreated(done.path);
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          variant="outline"
          disabled={disabled || !clone}
          title="Make a feature folder in the clone, ready for specs and test cases"
          onClick={() => {
            setMade("");
            setOpen(true);
          }}
        >
          <IconAdd aria-hidden />
          New folder
        </Button>
        {made && <span className="text-xs text-success">{`Created suites/${made}`}</span>}
      </div>
    );
  }
  return (
    <div className="space-y-1.5" role="group" aria-label="New folder">
      <div className="flex items-center gap-2">
        <div
          ref={field}
          className="min-w-0 flex-1"
          onKeyDown={(e) => {
            // Only the field itself: its list answers its own Enter and Escape.
            if ((e.target as HTMLElement).getAttribute("role") !== "combobox") return;
            if (e.key === "Enter" && path.trim()) {
              // The path was chosen in the list; Enter on the field makes the folder.
              e.preventDefault();
              if (!busy) void create();
            }
            if (e.key === "Escape") {
              // Only this form: a dialog behind it is not closed.
              e.stopPropagation();
              close();
            }
          }}
        >
          <Combobox
            ariaLabel="New folder path"
            placeholder="sl/admin/performance/new-feature"
            allowCustom
            seedQuery
            options={parents}
            triggerClassName="py-1.5 font-mono text-xs"
            value={path}
            onChange={setPath}
          />
        </div>
        <Button size="sm" disabled={busy || disabled || !path.trim()} onClick={() => void create()}>
          <IconConfirm aria-hidden />
          Create
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={close}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
      </div>
      <p className="text-xs text-muted">The folder's path under suites. It gets test-cases and specs folders, kept in git.</p>
      {error && <p className="text-xs text-danger">{error}</p>}
    </div>
  );
}
