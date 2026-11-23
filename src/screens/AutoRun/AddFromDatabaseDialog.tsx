/**
 * "Add accounts from the database": the logins in the environment's account
 * table, searchable, with a tick for each. Add selected makes an account of
 * every ticked login; the app reads each password itself, so none is ever
 * shown here or sent to this screen.
 */
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { commands, type AddedCounts } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Modal } from "../../components/ui/modal";
import { SearchInput } from "../../components/ui/search-input";
import { IconAdd, IconCancel } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { dbReadAccessOn } from "../../lib/mcpTools";

/** How long the search box waits after the last keystroke. */
const SEARCH_DELAY_MS = 300;

/** What Add selected did, in words: "2 accounts added, 1 already saved. Not
 * added: a.b (the database holds only a hash)". Counts and logins only. */
export function addedSentence(c: AddedCounts): string {
  const parts = [`${c.added} ${c.added === 1 ? "account" : "accounts"} added`];
  if (c.already_saved > 0) parts.push(`${c.already_saved} already saved`);
  let out = parts.join(", ") + ".";
  if (c.not_added.length > 0) {
    out += ` Not added: ${c.not_added.map((n) => `${n.login} (${n.reason})`).join("; ")}`;
  }
  return out;
}

export default function AddFromDatabaseDialog({
  onClose,
  onAdded,
}: {
  onClose: () => void;
  /** Called after an add, so the dialog behind reads its accounts again. */
  onAdded: () => Promise<void> | void;
}) {
  const qc = useQueryClient();
  const [text, setText] = useState("");
  const [search, setSearch] = useState("");
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState("");
  const [problem, setProblem] = useState("");

  useEffect(() => {
    const t = setTimeout(() => setSearch(text.trim()), SEARCH_DELAY_MS);
    return () => clearTimeout(t);
  }, [text]);

  const list = useQuery({
    queryKey: ["db-logins", search],
    queryFn: () => unwrapStr(commands.envListDbLogins(search, dbReadAccessOn())),
    retry: false,
    staleTime: 0,
    gcTime: 0,
  });
  const logins = list.data?.logins ?? [];
  const count = [...picked].length;

  const toggle = (login: string, on: boolean) =>
    setPicked((cur) => {
      const next = new Set(cur);
      if (on) next.add(login);
      else next.delete(login);
      return next;
    });

  const add = async () => {
    if (count === 0) return;
    setBusy(true);
    setProblem("");
    setSaid("");
    try {
      const done = await unwrapStr(commands.envAddDbLogins([...picked], dbReadAccessOn()));
      setSaid(addedSentence(done));
      setPicked(new Set());
      await onAdded();
      await qc.invalidateQueries({ queryKey: ["db-logins"] });
    } catch (e) {
      setProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal onClose={onClose} label="Add accounts from the database" className="flex w-full max-w-lg flex-col gap-3 p-5">
      <div>
        <h2 className="text-sm font-semibold text-text">Add accounts from the database</h2>
        <p className="mt-1 text-xs text-muted">
          The logins in this environment's account table. Each ticked login becomes an account, with its password read
          from the table.
        </p>
      </div>
      <SearchInput aria-label="Search logins" placeholder="Search logins" value={text} onChange={setText} />
      <div className="max-h-72 min-h-24 overflow-auto rounded-md border border-border">
        {list.isLoading && <p className="p-3 text-xs text-muted">Loading logins...</p>}
        {list.isError && <p className="p-3 text-xs text-danger">{list.error.message}</p>}
        {list.data && logins.length === 0 && <p className="p-3 text-xs text-muted">No logins found.</p>}
        <ul className="divide-y divide-border">
          {logins.map((l) => (
            <li key={l.login}>
              <label className="flex items-center gap-3 px-3 py-2 text-sm text-text">
                <Checkbox
                  checked={l.saved || picked.has(l.login)}
                  disabled={l.saved || busy}
                  ariaLabel={`Add ${l.login}`}
                  onCheckedChange={(on) => toggle(l.login, on)}
                />
                <span className="min-w-0 flex-1 truncate">{l.login}</span>
                {l.saved && <span className="text-xs text-muted">Saved</span>}
              </label>
            </li>
          ))}
        </ul>
      </div>
      {list.data?.cut_short && (
        <p className="text-xs text-muted">Showing the first 200 - type to narrow the list</p>
      )}
      {said && (
        <p role="status" className="text-xs text-text">
          {said}
        </p>
      )}
      {problem && <p className="text-xs text-danger">{problem}</p>}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="outline" disabled={busy || count === 0} onClick={add}>
          <IconAdd aria-hidden />
          Add selected ({count})
        </Button>
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}
