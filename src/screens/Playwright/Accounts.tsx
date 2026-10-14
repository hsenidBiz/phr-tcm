// The Playwright tab's Accounts: the repo's test accounts, read from its
// src/users/users.json at once (no repo command), and changed through its own
// `npm run users` CLI (pw_repo::users), so the repo's checks apply: a rename
// rewrites every reference, a referenced account cannot be deleted, a changed
// username or password clears its saved session.
//
// What uses each account (Used by) comes after the list, from the CLI as a
// short repo command; the column says so until it arrives, and stays blank,
// with a note, if it cannot be read.
//
// The accounts are a table with each row's actions as icon buttons. Add,
// Edit, Rename and Delete open a form in a dialog. Every change is previewed
// first (the CLI without --apply), and Apply is offered only under that
// preview. Editing the form again drops the preview.
//
// Passwords: the list carries none. Show asks Rust for one
// (`pw_users_password`) and holds it in this component's state only; Hide,
// any change, or leaving the screen drops it.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useRef, useState, type ReactNode } from "react";
import { commands, type AccountChange, type AccountField, type AccountRefs, type ChangeOutcome } from "../../bindings";
import { Button } from "../../components/ui/button";
import { Input } from "../../components/ui/input";
import { Modal } from "../../components/ui/modal";
import { Select } from "../../components/ui/select";
import {
  IconAdd,
  IconCancel,
  IconConfirm,
  IconEdit,
  IconHideSecret,
  IconRefresh,
  IconRemove,
  IconRename,
  IconReview,
  IconShowSecret,
} from "../../lib/actionIcons";
import { unwrapStr } from "../../lib/ipc";
import { MODAL_SHORT } from "../AutoRun/modalWidths";
import { Hint } from "./parts";

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

function references(a: AccountRefs): string[] {
  if (a.specs === 0 && a.other_refs === 0) return ["Not used"];
  return [a.specs ? plural(a.specs, "spec") : "", a.other_refs ? plural(a.other_refs, "other reference") : ""].filter(Boolean);
}

/** `text` with a line-break chance after each of `chars`: a long dotted key
 * or an address wraps at its dots instead of mid-word. */
function breakAfter(text: string, chars: string): ReactNode[] {
  const out: ReactNode[] = [];
  let run = "";
  for (const ch of text) {
    run += ch;
    if (chars.includes(ch)) {
      out.push(run, <wbr key={out.length} />);
      run = "";
    }
  }
  out.push(run);
  return out;
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
  const titleId = useId();
  const accounts = useQuery({
    queryKey: [ACCOUNTS_KEY, clone],
    queryFn: () => unwrapStr(commands.pwUsersList(clone)),
    // A failed read is a file that could not be read: List again retries.
    retry: false,
    // A change here sets the new list; List again and Refresh read it again.
    staleTime: Infinity,
  });
  /** What uses each account: a repo command, asked for once the list is in. */
  const refs = useQuery({
    queryKey: [ACCOUNTS_KEY, clone, "refs"],
    queryFn: () => unwrapStr(commands.pwUsersRefs(clone)),
    enabled: accounts.isSuccess,
    retry: false,
    staleTime: Infinity,
  });
  const refsByKey = new Map((refs.data ?? []).map((r) => [r.key, r]));
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
    // A change can move what uses an account: counted again, after.
    void qc.invalidateQueries({ queryKey: [ACCOUNTS_KEY, clone, "refs"] });
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
  const close = () => !busy && open(null);
  const th = "px-3 py-2 text-left text-xs font-medium text-muted";
  const iconButton = "px-2";

  return (
    <section className="space-y-3" aria-label="Accounts">
      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" disabled={active} onClick={() => open(EMPTY_ADD)}>
          <IconAdd aria-hidden />
          Add account
        </Button>
        <Button size="sm" variant="outline" disabled={active} onClick={() => void qc.invalidateQueries({ queryKey: [ACCOUNTS_KEY, clone] })}>
          <IconRefresh aria-hidden />
          List again
        </Button>
        <Hint className="min-w-0 flex-1">
          Changed through the repo's npm run users, previewed first. Passwords stay hidden until you show one.
        </Hint>
      </div>

      {error && !form && <p className="whitespace-pre-wrap text-xs text-danger">{error}</p>}
      {done.length > 0 && (
        <div className="rounded-lg border border-success/40 bg-success/10 p-3">
          <p className="mb-1 text-xs font-medium text-success">Applied</p>
          <pre aria-label="Applied" className="overflow-x-auto font-mono text-xs text-text">
            {done.join("\n")}
          </pre>
        </div>
      )}

      {accounts.isLoading && <Hint>Listing the accounts…</Hint>}
      {accounts.isError && <p className="whitespace-pre-wrap text-xs text-danger">{accounts.error.message}</p>}
      {refs.isError && <Hint className="whitespace-pre-wrap">{`Used by could not be read: ${refs.error.message}`}</Hint>}
      {accounts.isSuccess && list.length === 0 && <Hint>No accounts in the repo yet.</Hint>}
      {list.length > 0 && (
        <div className="overflow-x-auto rounded-lg border border-border bg-surface">
          <table className="w-full text-sm" aria-label="Test accounts">
            <thead className="border-b border-border bg-surface-2">
              <tr>
                <th className={th}>Key</th>
                <th className={th}>Username</th>
                <th className={`${th} whitespace-nowrap`}>Employee no</th>
                <th className={th}>Display name</th>
                <th className={th}>Title</th>
                <th className={th}>Used by</th>
                <th className={`${th} text-right`}>
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {list.map((a) => {
                const showing = a.key in shown;
                const used = refsByKey.get(a.key);
                return (
                  <tr key={a.key} className="border-b border-border align-top last:border-b-0 hover:bg-surface-2/60">
                    <td className="px-3 py-2">
                      <span className="font-mono text-xs font-medium text-text">{breakAfter(a.key, ".")}</span>
                    </td>
                    <td className="px-3 py-2">
                      <div className="text-text">{breakAfter(a.username, "@.")}</div>
                      {showing && (
                        <div className="mt-0.5 text-xs text-muted">
                          {"Password: "}
                          <code aria-label={`Password of ${a.key}`} className="font-mono text-text">
                            {shown[a.key]}
                          </code>
                        </div>
                      )}
                    </td>
                    <td className="px-3 py-2 font-mono text-xs whitespace-nowrap text-muted">{a.employee_no || "-"}</td>
                    <td className="px-3 py-2">
                      <div className="text-text">{a.display_name || <span className="text-faint">-</span>}</div>
                      {a.supervisor && <div className="text-xs text-muted">{["reports to ", ...breakAfter(a.supervisor, ".")]}</div>}
                    </td>
                    <td className="px-3 py-2 text-muted">{a.title || <span className="text-faint">-</span>}</td>
                    <td className="px-3 py-2 text-xs">
                      {refs.isPending && accounts.isSuccess && !refs.isError ? (
                        <span className="text-faint" aria-label={`Counting what uses ${a.key}`}>
                          …
                        </span>
                      ) : (
                        used &&
                        references(used).map((r) => (
                          <div key={r} className={r === "Not used" ? "text-faint" : "text-muted"}>
                            {r}
                          </div>
                        ))
                      )}
                    </td>
                    <td className="w-px px-2 py-1.5 whitespace-nowrap">
                      <div className="flex items-center justify-end gap-0.5">
                        <Button
                          size="sm"
                          variant="ghost"
                          className={iconButton}
                          aria-label={`${showing ? "Hide" : "Show"} password of ${a.key}`}
                          title={showing ? "Hide password" : "Show password"}
                          onClick={() => void togglePassword(a.key)}
                        >
                          {showing ? <IconHideSecret aria-hidden /> : <IconShowSecret aria-hidden />}
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className={iconButton}
                          aria-label={`Edit ${a.key}`}
                          title="Edit"
                          disabled={active}
                          onClick={() => open({ mode: "edit", key: a.key, field: "Username", value: "" })}
                        >
                          <IconEdit aria-hidden />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className={iconButton}
                          aria-label={`Rename ${a.key}`}
                          title="Rename"
                          disabled={active}
                          onClick={() => open({ mode: "rename", key: a.key, to: a.key })}
                        >
                          <IconRename aria-hidden />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className={`${iconButton} hover:text-danger`}
                          aria-label={`Delete ${a.key}`}
                          title="Delete"
                          disabled={active}
                          onClick={() => open({ mode: "delete", key: a.key })}
                        >
                          <IconRemove aria-hidden />
                        </Button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {form && (
        <Modal onClose={close} labelledBy={titleId} className={`${MODAL_SHORT} flex max-h-[90vh] flex-col`}>
          <div className="flex min-h-0 flex-1 flex-col" role="group" aria-label={heading(form)}>
            <div className="border-b border-border px-5 py-3">
              <h2 id={titleId} className="text-sm font-semibold text-text">
                {heading(form)}
              </h2>
              <ol className="mt-1 flex items-center gap-2 text-xs" aria-label="Steps">
                <li className={preview ? "text-muted" : "font-medium text-accent"}>1. Fill in and preview</li>
                <li aria-hidden className="text-faint">
                  /
                </li>
                <li className={preview ? "font-medium text-accent" : "text-faint"}>2. Apply</li>
              </ol>
            </div>

            <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-5 py-4">
              {form.mode === "add" && (
                <div className="grid gap-3 sm:grid-cols-2">
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
                <div className="grid gap-3 sm:grid-cols-[12rem_minmax(0,1fr)] sm:items-end">
                  <div className={fieldLabel}>
                    <span aria-hidden>Field</span>
                    <Select aria-label="Field" value={form.field} onChange={(e) => edit({ ...form, field: e.target.value as AccountField, value: "" })}>
                      {FIELDS.map((f) => (
                        <option key={f.field} value={f.field}>
                          {f.label}
                        </option>
                      ))}
                    </Select>
                  </div>
                  <Field
                    label="New value"
                    type={form.field === "Password" ? "password" : "text"}
                    value={form.value}
                    onChange={(value) => edit({ ...form, value })}
                  />
                  <Hint className="sm:col-span-2">Leave it empty to clear an optional field.</Hint>
                </div>
              )}
              {form.mode === "rename" && (
                <>
                  <Field label="New key" value={form.to} onChange={(to) => edit({ ...form, to })} />
                  <Hint>Every spec and test case that names it is rewritten too.</Hint>
                </>
              )}
              {form.mode === "delete" && <Hint>The repo refuses while anything still uses the account.</Hint>}
              {(form.mode === "add" || (form.mode === "edit" && form.field === "Password")) && (
                <Hint>
                  While the change runs, the password is visible in the Windows process list for a few seconds. It is never
                  kept in a file or in the app's log.
                </Hint>
              )}

              {preview && (
                <div className="space-y-1">
                  <p className="text-xs font-medium text-text">What npm run users will do:</p>
                  <pre aria-label="Preview" className="overflow-x-auto rounded-md border border-border bg-bg p-2 font-mono text-xs text-text">
                    {preview.join("\n")}
                  </pre>
                </div>
              )}
              {error && <p className="whitespace-pre-wrap text-xs text-danger">{error}</p>}
            </div>

            <div className="flex items-center justify-end gap-2 border-t border-border px-5 py-3">
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => open(null)}>
                <IconCancel aria-hidden />
                Cancel
              </Button>
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
            </div>
          </div>
        </Modal>
      )}
    </section>
  );
}
