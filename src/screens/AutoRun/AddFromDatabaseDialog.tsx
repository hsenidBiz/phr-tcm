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

/** The longest search the app takes, in characters. */
const SEARCH_MAX = 128;

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
  /** The add worked but the dialog behind could not read the list again. */
  const [reopenNote, setReopenNote] = useState(false);

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
  // The whole list, whatever the search shows: what Select all ticks. Same
  // command with an empty search; with no search typed it is the same query.
  const full = useQuery({
    queryKey: ["db-logins", ""],
    queryFn: () => unwrapStr(commands.envListDbLogins("", dbReadAccessOn())),
    retry: false,
    staleTime: 0,
    gcTime: 0,
  });
  // Every ticked login counts and is added, shown by the search or not.
  const count = picked.size;
  const selectable = (full.data?.logins ?? []).filter((l) => !l.saved).map((l) => l.login);
  const selectableTicked = selectable.filter((l) => picked.has(l)).length;
  /** Select all met the 200 cap, so only the first 200 were ticked. */
  const [cutNote, setCutNote] = useState(false);

  const toggle = (login: string, on: boolean) =>
    setPicked((cur) => {
      const next = new Set(cur);
      if (on) next.add(login);
      else next.delete(login);
      return next;
    });

  const toggleAll = (on: boolean) => {
    setPicked(on ? new Set(selectable) : new Set());
    setCutNote(on && !!full.data?.cut_short);
  };

  const add = async () => {
    if (count === 0) return;
    setBusy(true);
    setProblem("");
    setSaid("");
    setReopenNote(false);
    try {
      const done = await unwrapStr(commands.envAddDbLogins([...picked], dbReadAccessOn()));
      setSaid(addedSentence(done));
      setPicked(new Set());
      setCutNote(false);
      // The add is done: a failure to read the accounts again is not a
      // problem with it, only a note that the list behind is behind.
      try {
        await onAdded();
      } catch {
        setReopenNote(true);
      }
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
      <SearchInput
        aria-label="Search logins"
        placeholder="Search logins"
        maxLength={SEARCH_MAX}
        value={text}
        onChange={setText}
      />
      <div className="flex items-center justify-between gap-2 text-xs text-muted">
        <label className="flex items-center gap-2">
          <Checkbox
            checked={selectable.length > 0 && selectableTicked === selectable.length}
            indeterminate={selectableTicked > 0 || picked.size > 0}
            disabled={selectable.length === 0 || busy}
            ariaLabel="Select all"
            onCheckedChange={toggleAll}
          />
          Select all
        </label>
        <span>{count} selected</span>
      </div>
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
      {cutNote && (
        <p className="text-xs text-muted">Selected the first 200 - add these, then Select all again for the rest</p>
      )}
      {said && (
        <p role="status" className="text-xs text-text">
          {said}
        </p>
      )}
      {reopenNote && <p className="text-xs text-muted">Reopen Accounts to see the new accounts</p>}
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
