// The tester's own test accounts. Entered here, kept in a plain file on
// this machine: the application under test is internal, and the point is
// that every tester runs the same scripts with their own logins. A script
// only ever names an account by its key.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { toast } from "../../lib/toast";
import { commands, type Account } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Checkbox } from "../../components/ui/checkbox";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { IconAdd, IconCancel, IconConfirm, IconRemove } from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

export default function AccountsDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const existing = useQuery({
    queryKey: ["autorun-accounts"],
    queryFn: () => unwrapStr(commands.autoRunListAccounts()),
    retry: false,
  });
  const [rows, setRows] = useState<Account[] | null>(null);
  const [show, setShow] = useState(false);
  const [problem, setProblem] = useState("");
  useEffect(() => {
    if (existing.data && rows === null) setRows(existing.data);
  }, [existing.data, rows]);

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
        {(rows ?? []).map((a, i) => {
          // Row fields are named by POSITION ("Key for account 1"), not by
          // the account's own key: naming them after the key would change
          // the field's accessible name out from under a screen reader
          // user mid-edit, right as the key is being typed.
          const pos = `account ${i + 1}`;
          return (
            <div key={i} className="grid grid-cols-[1fr_1fr_1fr_1fr_auto] items-center gap-2">
              <Input aria-label={`Key for ${pos}`} placeholder="hr.admin" value={a.key}
                onChange={(e) => edit(i, { key: e.target.value })} />
              <Input aria-label={`Name for ${pos}`} placeholder="HR Admin" value={a.label}
                onChange={(e) => edit(i, { label: e.target.value })} />
              <Input aria-label={`Username for ${pos}`} placeholder="Username" value={a.username}
                onChange={(e) => edit(i, { username: e.target.value })} />
              <Input aria-label={`Password for ${pos}`} placeholder="Password" value={a.password}
                type={show ? "text" : "password"} onChange={(e) => edit(i, { password: e.target.value })} />
              <Button size="sm" variant="ghost" aria-label={`Remove ${who(a, i)}`}
                onClick={() => setRows((r) => (r ?? []).filter((_, j) => j !== i))}>
                <IconRemove aria-hidden />
              </Button>
            </div>
          );
        })}
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
