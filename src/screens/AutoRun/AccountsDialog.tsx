// The tester's own test accounts. Entered here, kept in a plain file on
// this machine: the application under test is internal, and the point is
// that every tester runs the same scripts with their own logins. A script
// only ever names an account by its key.
//
// Under the accounts sit the logins the AI assistant PROPOSED for the
// active environment, in a searchable list: the person ticks the ones to
// add, and opens a password field only for a login they want to set one
// for. Left empty, a login uses the password the assistant read from the
// database with it, when it proposed one (in a test environment only; it
// stays in Rust - this screen only learns that there is one), and otherwise
// the environment's default password. A login with neither shows its field
// open, since it cannot be added without one. There is no event when the
// assistant writes a proposal, so the list is read again every time this
// dialog opens.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { toast } from "../../lib/toast";
import { commands, type Account, type AccountInput, type ProposedAccount } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { IconAdd, IconCancel, IconClear, IconConfirm, IconRemove } from "../../lib/actionIcons";
import { activeEnvironment, envKeys, useEnvironments } from "../../lib/environments";
import { unwrapStr } from "../../lib/ipc";

/** The small word over each field, shown only while a row is stacked. */
const fieldLabel = "block text-xs font-medium text-muted lg:hidden";

const PROPOSALS_KEY = envKeys.proposals;

/** The accounts just added or replaced, put into the rows on screen: each
 * one's saved version takes the place of the row with its key, or joins
 * the end. Rows the person has been editing and left alone stay exactly as
 * they are. */
function mergeAdded(local: Account[], saved: Account[], keys: string[]): Account[] {
  const out = [...local];
  for (const key of keys) {
    const fresh = saved.find((a) => a.key === key);
    if (!fresh) continue;
    const at = out.findIndex((a) => a.key === key);
    if (at >= 0) out[at] = fresh;
    else out.push(fresh);
  }
  return out;
}

export default function AccountsDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const existing = useQuery({
    queryKey: ["autorun-accounts"],
    queryFn: () => unwrapStr(commands.autoRunListAccounts()),
    retry: false,
  });
  // Read afresh on every open (nothing tells this screen when the assistant
  // writes one): always fetched again when the dialog mounts.
  const proposed = useQuery({
    queryKey: PROPOSALS_KEY,
    queryFn: async () => (await unwrapStr(commands.envProposals())) ?? [],
    retry: false,
    staleTime: 0,
    refetchOnMount: "always",
  });
  const hasDefaultPassword = activeEnvironment(useEnvironments().data)?.has_default_password ?? false;
  const [rows, setRows] = useState<Account[] | null>(null);
  const [show, setShow] = useState(false);
  const [problem, setProblem] = useState("");
  useEffect(() => {
    if (existing.data && rows === null) setRows(existing.data);
  }, [existing.data, rows]);

  // The proposals: which are ticked, the password typed for each, the
  // sentence the app answered a refused add with, and the keys that are
  // already accounts and wait on a Replace / Keep answer.
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [typed, setTyped] = useState<Record<string, string>>({});
  const [proposalProblem, setProposalProblem] = useState("");
  const [busy, setBusy] = useState(false);
  const [asking, setAsking] = useState<{
    picks: AccountInput[];
    keys: string[];
    answers: Record<string, boolean>;
  } | null>(null);

  // What the search box holds, and the logins whose password field the
  // person opened with Set password.
  const [query, setQuery] = useState("");
  const [opened, setOpened] = useState<Set<string>>(new Set());
  // A field opened by Set password takes the focus once, after it renders.
  // Not autoFocus: that would also fire when a search brings a row with an
  // open field back, pulling the focus out of the search box mid-word.
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const fields = useRef(new Map<string, HTMLInputElement>());
  useEffect(() => {
    if (focusKey === null) return;
    fields.current.get(focusKey)?.focus();
    setFocusKey(null);
  }, [focusKey]);

  const proposals: ProposedAccount[] = proposed.data ?? [];
  const needle = query.trim().toLowerCase();
  const shown = needle
    ? proposals.filter((p) =>
        [p.key, p.label, p.role ?? "", p.username].some((f) => f.toLowerCase().includes(needle)),
      )
    : proposals;
  const pickedCount = proposals.filter((p) => picked.has(p.key)).length;
  const shownPicked = shown.filter((p) => picked.has(p.key)).length;

  /** Ticks or clears `keys`, leaving every other pick as it is, so a pick
   * made under one search stays through the next. */
  const pick = (keys: string[], on: boolean) =>
    setPicked((cur) => {
      const next = new Set(cur);
      for (const k of keys) {
        if (on) next.add(k);
        else next.delete(k);
      }
      return next;
    });

  /** Where an empty field takes the password from: the database, the
   * environment's default, or nowhere (the login needs one typed). */
  const fallback = (p: ProposedAccount) =>
    p.has_password ? "database" : hasDefaultPassword ? "default" : "none";

  /** Drops the typed password and closes the field, so the login goes back
   * to its database or default password. */
  const unset = (key: string) => {
    setTyped((cur) => Object.fromEntries(Object.entries(cur).filter(([k]) => k !== key)));
    setOpened((cur) => {
      const next = new Set(cur);
      next.delete(key);
      return next;
    });
  };

  /** Reads what the app saved and puts `keys` into the rows. */
  const takeAdded = async (keys: string[]) => {
    const saved = await unwrapStr(commands.autoRunListAccounts());
    qc.setQueryData(["autorun-accounts"], saved);
    setRows((r) => mergeAdded(r ?? saved, saved, keys));
    await qc.invalidateQueries({ queryKey: PROPOSALS_KEY });
  };

  const addSelected = async () => {
    const picks: AccountInput[] = proposals
      .filter((p) => picked.has(p.key))
      .map((p) => ({ key: p.key, label: p.label, username: p.username, password: typed[p.key] ?? "" }));
    if (picks.length === 0) return;
    setProposalProblem("");
    setBusy(true);
    try {
      // Answers the keys that are already accounts and need a Replace
      // answer; the rest are written. A pick with no password and no
      // default refuses the WHOLE call, in a sentence naming the key.
      const confirm = await unwrapStr(commands.envAddProposals(picks, []));
      const added = picks.map((p) => p.key).filter((k) => !confirm.includes(k));
      await takeAdded(added);
      // What went in is no longer a pick; a key waiting on its answer stays
      // ticked, with its typed password.
      setPicked((cur) => new Set([...cur].filter((k) => !added.includes(k))));
      setTyped((cur) => Object.fromEntries(Object.entries(cur).filter(([k]) => !added.includes(k))));
      if (confirm.length > 0) {
        setAsking({ picks: picks.filter((p) => confirm.includes(p.key)), keys: confirm, answers: {} });
      }
    } catch (e) {
      // Nothing was written: the ticks and the typed passwords stay for
      // the person to finish.
      setProposalProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  /** One Replace / Keep answer. When the last key is answered, the
   * confirmed ones are sent with `replace` naming exactly them. */
  const answer = async (key: string, replace: boolean) => {
    if (!asking) return;
    const answers = { ...asking.answers, [key]: replace };
    if (!asking.keys.every((k) => k in answers)) {
      setAsking({ ...asking, answers });
      return;
    }
    const yes = asking.keys.filter((k) => answers[k]);
    const kept = asking.keys.filter((k) => !answers[k]);
    setAsking(null);
    setPicked((cur) => new Set([...cur].filter((k) => !kept.includes(k))));
    if (yes.length === 0) return;
    setProposalProblem("");
    setBusy(true);
    try {
      await unwrapStr(
        commands.envAddProposals(
          asking.picks.filter((p) => yes.includes(p.key)),
          yes,
        ),
      );
      await takeAdded(yes);
      setPicked((cur) => new Set([...cur].filter((k) => !yes.includes(k))));
    } catch (e) {
      setProposalProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const dismiss = async () => {
    setProposalProblem("");
    setBusy(true);
    try {
      await unwrapStr(commands.envDismissProposals());
      setPicked(new Set());
      setTyped({});
      setOpened(new Set());
      setAsking(null);
      await qc.invalidateQueries({ queryKey: PROPOSALS_KEY });
    } catch (e) {
      setProposalProblem(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const save = useMutation({
    mutationFn: () => unwrapStr(commands.autoRunSaveAccounts(rows ?? [])),
    onSuccess: (dropped) => {
      qc.invalidateQueries({ queryKey: ["autorun-accounts"] });
      toast.success(
        dropped.length > 0
          ? `Accounts saved. Saved sessions dropped for: ${dropped.join(", ")}.`
          : "Accounts saved.",
      );
      onClose();
    },
    onError: (e) => setProblem(e instanceof Error ? e.message : String(e)),
  });

  const edit = (i: number, patch: Partial<Account>) =>
    setRows((r) => (r ?? []).map((a, j) => (j === i ? { ...a, ...patch } : a)));
  // Used only for the Remove button, where naming the account by its own
  // key ("Remove admin") reads better than "Remove account 1". The four
  // row fields below are named by POSITION instead - see the comment
  // there for why.
  const who = (a: Account, i: number) => a.key.trim() || `account ${i + 1}`;
  const blocked = existing.isLoading || existing.isError || rows === null;

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-3xl flex-col gap-3 p-5">
      <div>
        <h2 className="text-sm font-semibold text-text">Accounts</h2>
        <p className="mt-1 text-xs text-muted">
          Your own test accounts, kept on this machine. A script names an account by its key, so the same
          script works for every tester with their own login.
        </p>
      </div>
      {existing.isError && <p className="text-xs text-danger">{existing.error.message}</p>}
      <div className="min-h-0 flex-1 space-y-2 overflow-auto">
        {rows?.length === 0 && <p className="text-xs text-muted">No accounts yet.</p>}
        {/* Column names for the one-line layout. Hidden from assistive
            tech: every field already carries its own name. */}
        {(rows?.length ?? 0) > 0 && (
          <div
            aria-hidden
            className="hidden grid-cols-[1fr_1fr_1fr_1fr_2.5rem] gap-2 text-xs font-medium text-muted lg:grid"
          >
            <span>Key</span>
            <span>Name</span>
            <span>Username</span>
            <span>Password</span>
            <span />
          </div>
        )}
        {(rows ?? []).map((a, i) => {
          // Row fields are named by POSITION ("Key for account 1"), not by
          // the account's own key: naming them after the key would change
          // the field's accessible name out from under a screen reader
          // user mid-edit, right as the key is being typed.
          const pos = `account ${i + 1}`;
          return (
            // One column on a narrow window, two on a medium one, the full
            // row of four plus Remove only where there is room for it -
            // four fixed columns squashed every field to a sliver.
            <div
              key={i}
              className="grid grid-cols-1 items-center gap-2 border-b border-border/60 pb-2 last:border-b-0 sm:grid-cols-2 lg:grid-cols-[1fr_1fr_1fr_1fr_2.5rem] lg:border-b-0 lg:pb-0"
            >
              {/* A visible word over each field while the row is stacked
                  (below lg), where a placeholder alone vanishes the moment
                  the field has a value. On one line the header row above
                  names the columns instead. The aria-label still wins as
                  the accessible name, and it starts with the same word. */}
              <label className="space-y-1">
                <span className={fieldLabel}>Key</span>
                <Input aria-label={`Key for ${pos}`} placeholder="hr.admin" value={a.key}
                  onChange={(e) => edit(i, { key: e.target.value })} />
              </label>
              <label className="space-y-1">
                <span className={fieldLabel}>Name</span>
                <Input aria-label={`Name for ${pos}`} placeholder="HR Admin" value={a.label}
                  onChange={(e) => edit(i, { label: e.target.value })} />
              </label>
              <label className="space-y-1">
                <span className={fieldLabel}>Username</span>
                <Input aria-label={`Username for ${pos}`} placeholder="Username" value={a.username}
                  onChange={(e) => edit(i, { username: e.target.value })} />
              </label>
              <label className="space-y-1">
                <span className={fieldLabel}>Password</span>
                <Input aria-label={`Password for ${pos}`} placeholder="Password" value={a.password}
                  type={show ? "text" : "password"} onChange={(e) => edit(i, { password: e.target.value })} />
              </label>
              <Button size="sm" variant="ghost" aria-label={`Remove ${who(a, i)}`}
                className="justify-self-end sm:col-span-2 lg:col-span-1"
                onClick={() => setRows((r) => (r ?? []).filter((_, j) => j !== i))}>
                <IconRemove aria-hidden />
              </Button>
            </div>
          );
        })}
        {proposals.length > 0 && (
          <section className="space-y-2 border-t border-border pt-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h3 className="text-xs font-semibold text-text">Proposed by the assistant ({proposals.length})</h3>
              <Input
                aria-label="Search proposed logins"
                placeholder="Search logins"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                className="h-8 w-48 py-1 text-xs"
              />
            </div>
            <p className="text-xs text-muted">Each login uses its password from the database unless you set one.</p>
            <div className="flex items-center justify-between gap-2 text-xs text-muted">
              <label className="flex items-center gap-2">
                <Checkbox
                  checked={shown.length > 0 && shownPicked === shown.length}
                  indeterminate={shownPicked > 0}
                  disabled={shown.length === 0}
                  ariaLabel="Select all shown"
                  onCheckedChange={(on) => pick(shown.map((p) => p.key), on)}
                />
                Select all shown
              </label>
              <span>{pickedCount} selected</span>
            </div>
            <div className="max-h-[22rem] overflow-y-auto rounded-md border border-border">
              {shown.length === 0 ? (
                <p className="p-3 text-xs text-muted">No logins match "{query.trim()}".</p>
              ) : (
                <ul className="divide-y divide-border">
                  {shown.map((p) => {
                    const from = fallback(p);
                    const value = typed[p.key] ?? "";
                    // A login with nothing to fall back on keeps its field
                    // open: it cannot be added without a password.
                    const fieldOpen = from === "none" || opened.has(p.key) || value !== "";
                    const details = [p.label, p.role, p.username].filter((d): d is string => !!d);
                    return (
                      <li key={p.key} className="space-y-1.5 px-2 py-1.5">
                        <div className="flex items-center gap-2">
                          <Checkbox
                            checked={picked.has(p.key)}
                            ariaLabel={`Add ${p.key}`}
                            onCheckedChange={(on) => pick([p.key], on)}
                          />
                          <div className="min-w-0 flex-1 text-xs">
                            <span className="id-mono break-all text-text">{p.key}</span>
                            {details.map((d, i) => (
                              <span key={i} className="text-muted">
                                {i === 0 ? " " : " - "}
                                <span className="break-all">{d}</span>
                              </span>
                            ))}
                          </div>
                          {value !== "" ? (
                            <Badge className="shrink-0 bg-accent/15 text-accent">Password set</Badge>
                          ) : from === "database" ? (
                            <Badge className="shrink-0 bg-success/15 text-success">From database</Badge>
                          ) : from === "default" ? (
                            <Badge className="shrink-0">Default password</Badge>
                          ) : (
                            <Badge className="shrink-0 bg-warning/15 text-warning">Needs a password</Badge>
                          )}
                          {!fieldOpen && (
                            <Button
                              size="sm"
                              variant="ghost"
                              aria-label={`Set password for ${p.key}`}
                              className="shrink-0 px-2 py-1"
                              onClick={() => {
                                setOpened((cur) => new Set(cur).add(p.key));
                                setFocusKey(p.key);
                              }}
                            >
                              Set password
                            </Button>
                          )}
                        </div>
                        {fieldOpen && (
                          <div className="flex items-center gap-2 pl-6">
                            <Input
                              aria-label={`Password for proposed ${p.key}`}
                              placeholder="Password"
                              value={value}
                              type={show ? "text" : "password"}
                              ref={(el) => {
                                if (el) fields.current.set(p.key, el);
                                else fields.current.delete(p.key);
                              }}
                              onChange={(e) => setTyped((cur) => ({ ...cur, [p.key]: e.target.value }))}
                              className="h-8 min-w-0 flex-1 py-1 text-xs"
                            />
                            {from !== "none" && (
                              <Button
                                size="sm"
                                variant="ghost"
                                aria-label={`${from === "database" ? "Use database password" : "Use default password"} for ${p.key}`}
                                className="shrink-0 px-2 py-1"
                                onClick={() => unset(p.key)}
                              >
                                {from === "database" ? "Use database password" : "Use default password"}
                              </Button>
                            )}
                          </div>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
            {asking && (
              <div className="space-y-1 rounded-md border border-border bg-surface-2 p-2">
                {asking.keys.map((key) => (
                  <div key={key} className="flex flex-wrap items-center gap-2 text-xs">
                    <p className="text-text">Replace {key}?</p>
                    {key in asking.answers ? (
                      <span className="text-faint">{asking.answers[key] ? "will be replaced" : "kept as it is"}</span>
                    ) : (
                      <>
                        <Button size="sm" variant="outline" aria-label={`Replace ${key}`} onClick={() => answer(key, true)}>
                          <IconConfirm aria-hidden />
                          Replace
                        </Button>
                        <Button size="sm" variant="ghost" aria-label={`Keep ${key}`} onClick={() => answer(key, false)}>
                          <IconCancel aria-hidden />
                          Keep
                        </Button>
                      </>
                    )}
                  </div>
                ))}
              </div>
            )}
            {proposalProblem && <p className="text-xs text-danger">{proposalProblem}</p>}
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={busy || pickedCount === 0 || asking !== null || rows === null}
                onClick={addSelected}
              >
                <IconAdd aria-hidden />
                Add selected ({pickedCount})
              </Button>
              <Button size="sm" variant="ghost" disabled={busy} onClick={dismiss}>
                <IconClear aria-hidden />
                Dismiss
              </Button>
            </div>
          </section>
        )}
      </div>
      <div className="flex items-center gap-3">
        <Button size="sm" variant="outline" disabled={blocked}
          onClick={() => setRows((r) => [...(r ?? []), { key: "", label: "", username: "", password: "" }])}>
          <IconAdd aria-hidden />
          Add account
        </Button>
        <label className="flex items-center gap-2 text-xs text-muted">
          <Checkbox checked={show} onCheckedChange={setShow} ariaLabel="Show passwords" />
          Show passwords
        </label>
      </div>
      {problem && <p className="text-xs text-danger">{problem}</p>}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Cancel
        </Button>
        <Button size="sm" disabled={blocked || save.isPending} onClick={() => { setProblem(""); save.mutate(); }}>
          <IconConfirm aria-hidden />
          {save.isPending ? "Saving" : "Save accounts"}
        </Button>
      </div>
    </Modal>
  );
}
