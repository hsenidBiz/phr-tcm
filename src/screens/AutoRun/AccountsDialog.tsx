// The tester's own test accounts. Entered here, kept in a plain file on
// this machine: the application under test is internal, and the point is
// that every tester runs the same scripts with their own logins. A script
// only ever names an account by its key.
//
// Under the accounts sit the logins the AI assistant PROPOSED for the
// active environment (it never supplies a password): the person ticks the
// ones to add and types a password for each, or leaves it empty to use the
// environment's default password. There is no event when the assistant
// writes a proposal, so the list is read again every time this dialog
// opens.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { toast } from "../../lib/toast";
import { commands, type Account, type AccountInput, type ProposedAccount } from "../../bindings";
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

  const proposals: ProposedAccount[] = proposed.data ?? [];

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
            <h3 className="text-xs font-semibold text-text">Proposed by the assistant ({proposals.length})</h3>
            <p className="text-xs text-muted">
              Logins the assistant found for this environment. It does not know their passwords: type one for
              each, or leave it empty to use the environment's default password.
            </p>
            {proposals.map((p) => (
              <div
                key={p.key}
                className="grid grid-cols-1 items-center gap-2 sm:grid-cols-2 lg:grid-cols-[1.5rem_1fr_1fr_1fr_1fr]"
              >
                <Checkbox
                  checked={picked.has(p.key)}
                  ariaLabel={`Add ${p.key}`}
                  onCheckedChange={(on) =>
                    setPicked((cur) => {
                      const next = new Set(cur);
                      if (on) next.add(p.key);
                      else next.delete(p.key);
                      return next;
                    })
                  }
                />
                <span className="id-mono break-all text-xs text-text">{p.key}</span>
                <span className="text-xs text-muted">
                  <span className="text-text">{p.label}</span>
                  {p.role ? <span className="ml-1 text-faint">({p.role})</span> : null}
                </span>
                <span className="break-all text-xs text-muted">{p.username}</span>
                <Input
                  aria-label={`Password for proposed ${p.key}`}
                  placeholder={hasDefaultPassword ? "default password" : "no default password set - type one"}
                  value={typed[p.key] ?? ""}
                  type={show ? "text" : "password"}
                  onChange={(e) => setTyped((cur) => ({ ...cur, [p.key]: e.target.value }))}
                />
              </div>
            ))}
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
                disabled={busy || picked.size === 0 || asking !== null || rows === null}
                onClick={addSelected}
              >
                <IconAdd aria-hidden />
                Add selected
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
