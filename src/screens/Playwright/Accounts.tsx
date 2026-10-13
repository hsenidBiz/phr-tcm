// The Playwright tab's Accounts: the repo's test accounts, listed and changed
// through its own `npm run users` CLI (pw_repo::users), so the repo's checks
// apply: a rename rewrites every reference, a referenced account cannot be
// deleted, a changed username or password clears its saved session.
//
// Every change is previewed first (the CLI without --apply), and Apply is
// offered only under that preview. Editing the form again drops the preview.
//
// Passwords: the list carries none. Show asks Rust for one
// (`pw_users_password`) and holds it in this component's state only; Hide,
// any change, or leaving the screen drops it.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { commands, type AccountChange, type AccountField, type ChangeOutcome, type RepoAccount } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Select } from "../../components/ui/select";
import {
  IconAdd,
  IconCancel,
  IconConfirm,
  IconEdit,
  IconHideDetails,
  IconRefresh,
  IconRemove,
  IconRename,
  IconReview,
  IconShowDetails,
} from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";

const sectionTitle = "text-xs font-semibold uppercase tracking-wide text-muted";
const fieldLabel = "flex flex-col gap-1 text-xs text-muted";
const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** The query key of the clone's accounts; the folder is part of it. */
export const ACCOUNTS_KEY = "pw-accounts";

/** The fields Edit can change, in the CLI's order. */
const FIELDS: { field: AccountField; label: string }[] = [
  { field: "Username", label: "Username" },
  { field: "Password", label: "Password" },
  { field: "EmployeeNo", label: "Employee number" },
  { field: "DisplayName", label: "Display name" },
  { field: "Title", label: "Title" },
  { field: "Supervisor", label: "Supervisor" },
];

/** The form open, if any. */
type Form =
  | { mode: "add"; key: string; username: string; password: string; employee_no: string; display_name: string; title: string; supervisor: string }
  | { mode: "edit"; key: string; field: AccountField; value: string }
  | { mode: "rename"; key: string; to: string }
  | { mode: "delete"; key: string };

const EMPTY_ADD: Form = { mode: "add", key: "", username: "", password: "", employee_no: "", display_name: "", title: "", supervisor: "" };

function changeOf(form: Form): AccountChange {
  switch (form.mode) {
    case "add": {
      const { mode: _mode, ...fields } = form;
      return { kind: "Add", ...fields };
    }
    case "edit":
      return { kind: "Set", key: form.key, field: form.field, value: form.value };
    case "rename":
      return { kind: "Rename", from: form.key, to: form.to };
    case "delete":
      return { kind: "Remove", key: form.key };
  }
}

function heading(form: Form): string {
  switch (form.mode) {
    case "add":
      return "Add an account";
    case "edit":
      return `Edit ${form.key}`;
    case "rename":
      return `Rename ${form.key}`;
    case "delete":
      return `Delete ${form.key}`;
  }
}

function references(a: RepoAccount): string {
  if (a.specs === 0 && a.other_refs === 0) return "Not used";
  const parts = [a.specs ? plural(a.specs, "spec") : "", a.other_refs ? plural(a.other_refs, "other reference") : ""];
  return parts.filter(Boolean).join(", ");
}

function Field({ label, value, onChange, type = "text" }: { label: string; value: string; onChange: (v: string) => void; type?: string }) {
  return (
    <label className={fieldLabel}>
      {label}
      <Input type={type} value={value} autoComplete="off" spellCheck={false} onChange={(e) => onChange(e.target.value)} />
    </label>
  );
}

export default function Accounts({ clone, onChanged }: { clone: string; onChanged?: () => void }) {
  const qc = useQueryClient();
  const accounts = useQuery({
    queryKey: [ACCOUNTS_KEY, clone],
    queryFn: () => unwrapStr(commands.pwUsersList(clone)),
    // A failed listing is a repo command that failed: List again retries.
    retry: false,
  });
  const [form, setForm] = useState<Form | null>(null);
  /** The form as it is now, for a preview that answers after an edit. */
  const latest = useRef<Form | null>(null);
  latest.current = form;
  /** The CLI's preview of the form's change, which Apply writes. */
  const [preview, setPreview] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [done, setDone] = useState<string[]>([]);
  /** Passwords shown, by key. Never kept anywhere else. */
  const [shown, setShown] = useState<Record<string, string>>({});

  const list = accounts.data ?? [];

  const open = (next: Form | null) => {
    setForm(next);
    setPreview(null);
    setError("");
    setDone([]);
  };

  /** A form edit: the preview no longer describes it. */
  const edit = (next: Form) => {
    setForm(next);
    setPreview(null);
  };

  const send = async (apply: boolean): Promise<ChangeOutcome | null> => {
    if (!form) return null;
    setBusy(true);
    setError("");
    try {
      return await unwrapStr(commands.pwUsersChange(clone, changeOf(form), apply));
    } catch (e) {
      setError(message(e));
      return null;
    } finally {
      setBusy(false);
    }
  };

  const showPreview = async () => {
    const asked = form;
    const r = await send(false);
    // Only a preview of the form as it still is: Apply writes that.
    if (r && latest.current === asked) setPreview(r.output);
  };

  const apply = async () => {
    const r = await send(true);
    if (!r) return;
    setForm(null);
    setPreview(null);
    setShown({});
    setDone(r.output);
    if (r.list_error) setError(r.list_error);
    if (r.accounts) qc.setQueryData([ACCOUNTS_KEY, clone], r.accounts);
    onChanged?.();
  };

  const togglePassword = async (key: string) => {
    if (key in shown) {
      setShown(({ [key]: _gone, ...rest }) => rest);
      return;
    }
    setError("");
    try {
      const password = await unwrapStr(commands.pwUsersPassword(clone, key));
      setShown((s) => ({ ...s, [key]: password }));
    } catch (e) {
      setError(message(e));
    }
  };

  const active = busy || accounts.isFetching;

  return (
    <section className="space-y-3" aria-label="Accounts">
      <h3 className={sectionTitle}>Accounts</h3>

      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={active} onClick={() => open(EMPTY_ADD)}>
          <IconAdd aria-hidden />
          Add account
        </Button>
        <Button size="sm" variant="outline" disabled={active} onClick={() => void accounts.refetch()}>
          <IconRefresh aria-hidden />
          List again
        </Button>
      </div>

      {error && <p className="whitespace-pre-wrap text-xs text-danger">{error}</p>}
      {done.length > 0 && (
        <pre aria-label="Applied" className="overflow-x-auto rounded border border-border bg-surface p-2 text-xs text-text">
          {done.join("\n")}
        </pre>
      )}

      {form && (
        <div className="space-y-2 rounded border border-border bg-surface p-3" role="group" aria-label={heading(form)}>
          <h4 className={sectionTitle}>{heading(form)}</h4>
          {form.mode === "add" && (
            <div className="grid gap-2 sm:grid-cols-2">
              <Field label="Key" value={form.key} onChange={(key) => edit({ ...form, key })} />
              <Field label="Username" value={form.username} onChange={(username) => edit({ ...form, username })} />
              <Field label="Password" type="password" value={form.password} onChange={(password) => edit({ ...form, password })} />
              <Field label="Employee number" value={form.employee_no} onChange={(employee_no) => edit({ ...form, employee_no })} />
              <Field label="Display name" value={form.display_name} onChange={(display_name) => edit({ ...form, display_name })} />
              <Field label="Title" value={form.title} onChange={(title) => edit({ ...form, title })} />
              <Field label="Supervisor" value={form.supervisor} onChange={(supervisor) => edit({ ...form, supervisor })} />
            </div>
          )}
          {form.mode === "edit" && (
            <div className="flex flex-wrap items-end gap-2">
              <Select aria-label="Field" value={form.field} onChange={(e) => edit({ ...form, field: e.target.value as AccountField, value: "" })}>
                {FIELDS.map((f) => (
                  <option key={f.field} value={f.field}>
                    {f.label}
                  </option>
                ))}
              </Select>
              <Field
                label="New value"
                type={form.field === "Password" ? "password" : "text"}
                value={form.value}
                onChange={(value) => edit({ ...form, value })}
              />
              <p className="text-xs text-muted">Leave it empty to clear an optional field.</p>
            </div>
          )}
          {form.mode === "rename" && (
            <>
              <Field label="New key" value={form.to} onChange={(to) => edit({ ...form, to })} />
              <p className="text-xs text-muted">Every spec and test case that names it is rewritten too.</p>
            </>
          )}
          {form.mode === "delete" && <p className="text-xs text-muted">The repo refuses while anything still uses the account.</p>}

          {preview && (
            <div className="space-y-1">
              <p className="text-xs text-muted">What npm run users will do:</p>
              <pre aria-label="Preview" className="overflow-x-auto rounded border border-border bg-surface p-2 text-xs text-text">
                {preview.join("\n")}
              </pre>
            </div>
          )}

          <div className="flex flex-wrap items-center gap-2">
            {preview ? (
              <Button size="sm" disabled={busy} onClick={() => void apply()}>
                <IconConfirm aria-hidden />
                Apply
              </Button>
            ) : (
              <Button size="sm" disabled={busy} onClick={() => void showPreview()}>
                <IconReview aria-hidden />
                Preview
              </Button>
            )}
            <Button size="sm" variant="outline" disabled={busy} onClick={() => open(null)}>
              <IconCancel aria-hidden />
              Cancel
            </Button>
          </div>
        </div>
      )}

      {accounts.isLoading && <p className="text-xs text-muted">Listing the accounts…</p>}
      {accounts.isError && <p className="whitespace-pre-wrap text-xs text-danger">{accounts.error.message}</p>}
      {accounts.isSuccess && list.length === 0 && <p className="text-xs text-muted">No accounts in the repo yet.</p>}
      <ul className="space-y-2">
        {list.map((a) => (
          <li key={a.key} className="space-y-1 rounded border border-border p-2">
            <div className="flex flex-wrap items-baseline gap-x-3">
              <span className="text-sm text-text">{a.key}</span>
              <span className="text-xs text-muted">{references(a)}</span>
            </div>
            <p className="text-xs text-muted">
              {[a.username, a.display_name, a.employee_no, a.title, a.supervisor ? `reports to ${a.supervisor}` : ""].filter(Boolean).join(" · ")}
            </p>
            {a.key in shown && (
              <p className="text-xs text-text">
                {"Password: "}
                <code aria-label={`Password of ${a.key}`}>{shown[a.key]}</code>
              </p>
            )}
            <div className="flex flex-wrap items-center gap-2">
              <Button size="sm" variant="outline" aria-label={`${a.key in shown ? "Hide" : "Show"} password of ${a.key}`} onClick={() => void togglePassword(a.key)}>
                {a.key in shown ? <IconHideDetails aria-hidden /> : <IconShowDetails aria-hidden />}
                {a.key in shown ? "Hide password" : "Show password"}
              </Button>
              <Button size="sm" variant="outline" aria-label={`Edit ${a.key}`} disabled={active} onClick={() => open({ mode: "edit", key: a.key, field: "Username", value: "" })}>
                <IconEdit aria-hidden />
                Edit
              </Button>
              <Button size="sm" variant="outline" aria-label={`Rename ${a.key}`} disabled={active} onClick={() => open({ mode: "rename", key: a.key, to: a.key })}>
                <IconRename aria-hidden />
                Rename
              </Button>
              <Button size="sm" variant="outline" aria-label={`Delete ${a.key}`} disabled={active} onClick={() => open({ mode: "delete", key: a.key })}>
                <IconRemove aria-hidden />
                Delete
              </Button>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
