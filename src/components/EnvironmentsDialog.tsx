// The environments: add, edit, remove, and give each one a default
// password. An environment is a name, a website address, the database it
// uses and - optionally - a default password for its accounts. Exactly one
// is active; switching is the AI Bridge tab's Environment card.
//
// The default password goes in and never comes out: the field is cleared
// the moment Set succeeds, and a row only ever learns whether one is set.

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { commands, type EnvInput, type EnvListView, type EnvView } from "../bindings";
import { IconAdd, IconCancel, IconClear, IconConfirm, IconEdit, IconRemove } from "../lib/actionIcons";
import { saveSelectedDb } from "../lib/dbServer";
import { envKeys, useEnvironments } from "../lib/environments";
import { Button } from "./ui/button";
import Combobox from "./ui/combobox";
import { Input, Textarea } from "./ui/input";
import { Modal } from "./ui/modal";
import { Switch } from "./ui/switch";

/** Said beside the Test environment switch. */
export const TEST_ENVIRONMENT_WARNING =
  "The AI assistant can read the full logins of this environment's accounts. Use only for test environments.";

/** Said for an environment with no address of its own. */
export const RECIPE_ADDRESS = "Using the sign-in recipe's address";

/** Said under Also allowed while there is no address: without one the
 * recipe's address and allowed sites are used (Rust refuses sites saved
 * without an address). */
export const ALLOWED_NEEDS_ADDRESS =
  "Also allowed needs a website address - until then the sign-in recipe's are used.";

/** What the form is editing: a new environment, or one by id. */
type Form = {
  id: string;
  name: string;
  start_url: string;
  /** One origin per line. */
  also: string;
  db_id: string;
  test_environment: boolean;
  /** What every draft an Auto Run fixture or script makes here starts with. */
  test_prefix: string;
};

/** The prefix a new environment starts with, as Rust's own default. */
export const DEFAULT_TEST_PREFIX = "AUTOTEST";

function formFor(env: EnvView): Form {
  return {
    id: env.id,
    name: env.name,
    start_url: env.start_url,
    also: env.allowed_origins.join("\n"),
    db_id: env.db_id,
    test_environment: env.test_environment,
    test_prefix: env.test_prefix,
  };
}

function inputFor(form: Form): EnvInput {
  return {
    id: form.id,
    name: form.name,
    start_url: form.start_url,
    // No address, no allowed sites of its own: the recipe's are used.
    allowed_origins:
      form.start_url.trim() === ""
        ? []
        : form.also
            .split("\n")
            .map((l) => l.trim())
            .filter(Boolean),
    db_id: form.db_id,
    test_environment: form.test_environment,
    test_prefix: form.test_prefix.trim(),
  };
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export default function EnvironmentsDialog({ onClose }: { onClose: () => void }) {
  const qc = useQueryClient();
  const envs = useEnvironments();
  const databases = useQuery({
    queryKey: ["db-databases"],
    queryFn: async () => (await commands.dbDatabases()) ?? [],
  });
  const [form, setForm] = useState<Form | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [problem, setProblem] = useState("");
  const [busy, setBusy] = useState(false);

  const list = envs.data?.environments ?? [];
  const active = envs.data?.active ?? "";
  const dbLabel = (id: string) =>
    databases.data?.find((d) => d.id === id)?.label ?? "database not set up any more";

  const take = (view: EnvListView) => qc.setQueryData(envKeys.list, view);

  /** Runs one command: shows its refusal in the dialog, and nothing else. */
  const run = async (work: () => Promise<void>) => {
    setProblem("");
    setBusy(true);
    try {
      await work();
    } catch (e) {
      setProblem(message(e));
    } finally {
      setBusy(false);
    }
  };

  const startAdd = () => {
    setProblem("");
    setPassword("");
    setRemoving(null);
    const current = list.find((e) => e.id === active);
    setForm({
      id: "",
      name: "",
      start_url: "",
      also: "",
      db_id: current?.db_id ?? databases.data?.[0]?.id ?? "",
      test_environment: false,
      test_prefix: DEFAULT_TEST_PREFIX,
    });
  };

  const startEdit = (env: EnvView) => {
    setProblem("");
    setPassword("");
    setRemoving(null);
    setForm(formFor(env));
  };

  const save = () =>
    run(async () => {
      if (!form) return;
      const res = await commands.envSave(inputFor(form));
      if (res.status === "error") throw new Error(res.error);
      take(res.data);
      // The card shows the active environment's database, so changing the
      // active one's database moves it too - but only to a database that
      // exists. Any other edit (a rename, say) leaves the card alone, and
      // an environment whose database is gone never writes that id there.
      const before = list.find((e) => e.id === form.id)?.db_id;
      const known = databases.data?.some((d) => d.id === form.db_id) ?? false;
      if (form.id && form.id === res.data.active && form.db_id !== before && known) {
        saveSelectedDb(form.db_id);
      }
      setForm(null);
      setPassword("");
    });

  const confirmRemove = (id: string) =>
    run(async () => {
      const res = await commands.envRemove(id);
      if (res.status === "error") throw new Error(res.error);
      take(res.data);
      setRemoving(null);
      if (form?.id === id) setForm(null);
    });

  const setDefaultPassword = () =>
    run(async () => {
      if (!form?.id) return;
      // Out of state before the call, so neither a refusal nor a rejected
      // call leaves it in the field: a password is retyped, never kept.
      const typed = password;
      setPassword("");
      const res = await commands.envSetDefaultPassword(form.id, typed);
      if (res.status === "error") throw new Error(res.error);
      await qc.invalidateQueries({ queryKey: envKeys.list });
    });

  const clearDefaultPassword = () =>
    run(async () => {
      if (!form?.id) return;
      const res = await commands.envClearDefaultPassword(form.id);
      if (res.status === "error") throw new Error(res.error);
      await qc.invalidateQueries({ queryKey: envKeys.list });
    });

  const editing = form ? list.find((e) => e.id === form.id) : undefined;

  return (
    <Modal onClose={onClose} className="flex max-h-[85vh] w-full max-w-2xl flex-col gap-3 p-5">
      <div>
        <h2 className="text-sm font-semibold text-text">Environments</h2>
        <p className="mt-1 text-xs text-muted">
          An environment is a website address, the database it uses and a default password for its
          accounts. Each keeps its own accounts and saved sign-ins.
        </p>
      </div>
      {envs.isError && <p className="text-xs text-danger">{message(envs.error)}</p>}

      <div className="min-h-0 flex-1 space-y-3 overflow-auto">
        <ul className="space-y-2">
          {list.map((env) => (
            <li key={env.id} className="space-y-1 rounded-md border border-border p-2">
              <div className="flex items-start gap-2">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-medium text-text">{env.name}</span>
                    {env.id === active && (
                      <span className="rounded-full bg-accent/15 px-1.5 py-0.5 text-[10px] font-medium leading-none text-accent-fill">
                        <span className="label-trim">Active</span>
                      </span>
                    )}
                    {env.test_environment && (
                      <span className="rounded-full bg-warning/15 px-1.5 py-0.5 text-[10px] font-medium leading-none text-warning">
                        <span className="label-trim">Test environment</span>
                      </span>
                    )}
                  </div>
                  <p className="truncate text-xs text-muted">{env.start_url || RECIPE_ADDRESS}</p>
                  <p className="truncate text-xs text-muted">{dbLabel(env.db_id)}</p>
                  <p className="text-xs text-faint">
                    {env.has_default_password ? "Default password set" : "No default password"}
                  </p>
                </div>
                <Button size="sm" variant="outline" aria-label={`Edit ${env.name}`} onClick={() => startEdit(env)}>
                  <IconEdit aria-hidden />
                  Edit
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  aria-label={`Remove ${env.name}`}
                  onClick={() => {
                    setProblem("");
                    setRemoving(env.id);
                  }}
                >
                  <IconRemove aria-hidden />
                </Button>
              </div>
              {removing === env.id && (
                <div className="flex flex-wrap items-center gap-2 border-t border-border/60 pt-2">
                  <span className="min-w-0 flex-1 text-xs text-text">
                    Remove {env.name}? Its accounts and saved sign-ins are deleted from this machine.
                  </span>
                  <Button size="sm" variant="ghost" onClick={() => setRemoving(null)}>
                    <IconCancel aria-hidden />
                    Keep
                  </Button>
                  <Button size="sm" variant="outline" disabled={busy} onClick={() => confirmRemove(env.id)}>
                    <IconRemove aria-hidden />
                    Remove
                  </Button>
                </div>
              )}
            </li>
          ))}
        </ul>

        {form && (
          <div className="space-y-3 rounded-md border border-border bg-surface-2/40 p-3">
            <h3 className="text-xs font-semibold text-text">
              {form.id ? `Edit ${editing?.name ?? "environment"}` : "New environment"}
            </h3>
            <label className="block space-y-1">
              <span className="text-xs font-medium text-muted">Name</span>
              <Input
                aria-label="Name"
                className="w-full"
                placeholder="QA"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </label>
            <label className="block space-y-1">
              <span className="text-xs font-medium text-muted">Website address</span>
              <Input
                aria-label="Website address"
                className="w-full"
                placeholder="https://qa.example.internal/"
                value={form.start_url}
                onChange={(e) => setForm({ ...form, start_url: e.target.value })}
              />
              <span className="block text-xs text-faint">
                Leave empty to use the sign-in recipe&apos;s address.
              </span>
            </label>
            <label className="block space-y-1">
              <span className="text-xs font-medium text-muted">Also allowed</span>
              <Textarea
                aria-label="Also allowed"
                className="min-h-[4rem] font-mono text-xs"
                placeholder="https://login.example.com"
                value={form.also}
                disabled={form.start_url.trim() === ""}
                onChange={(e) => setForm({ ...form, also: e.target.value })}
              />
              <span className="block text-xs text-faint">
                {form.start_url.trim() === ""
                  ? ALLOWED_NEEDS_ADDRESS
                  : "Other sites scripts may open, one per line."}
              </span>
            </label>
            <label className="block space-y-1">
              <span className="text-xs font-medium text-muted">Database</span>
              <Combobox
                ariaLabel="Environment database"
                className="w-full"
                placeholder="Pick a database…"
                value={form.db_id}
                items={(databases.data ?? []).map((d) => ({ value: d.id, label: d.label }))}
                loading={databases.isPending}
                onChange={(id) => setForm({ ...form, db_id: id })}
              />
            </label>
            <label className="block space-y-1">
              <span className="text-xs font-medium text-muted">Test name prefix</span>
              <Input
                aria-label="Test name prefix"
                className="w-full"
                placeholder={DEFAULT_TEST_PREFIX}
                value={form.test_prefix}
                onChange={(e) => setForm({ ...form, test_prefix: e.target.value })}
              />
              <span className="block text-xs text-faint">
                Every draft a fixture or script makes here starts with this, so Clean up can tell it from
                yours. 3 to 20 letters, digits or -.
              </span>
            </label>
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <Switch
                  checked={form.test_environment}
                  ariaLabel="Test environment"
                  onCheckedChange={(on) => setForm({ ...form, test_environment: on })}
                />
                <span className="text-xs font-medium text-text">Test environment</span>
              </div>
              <p className="text-xs text-muted">{TEST_ENVIRONMENT_WARNING}</p>
            </div>

            {form.id ? (
              <div className="space-y-1">
                <span className="text-xs font-medium text-muted">Default password</span>
                <div className="flex flex-wrap items-center gap-2">
                  <Input
                    type="password"
                    autoComplete="off"
                    aria-label="Default password"
                    className="min-w-0 flex-1"
                    placeholder="Used by accounts with no password of their own"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                  />
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy || password === ""}
                    onClick={setDefaultPassword}
                  >
                    <IconConfirm aria-hidden />
                    Set
                  </Button>
                  {editing?.has_default_password && (
                    <Button size="sm" variant="ghost" disabled={busy} onClick={clearDefaultPassword}>
                      <IconClear aria-hidden />
                      Clear default password
                    </Button>
                  )}
                </div>
              </div>
            ) : (
              <p className="text-xs text-faint">Save the environment first, then set its default password.</p>
            )}

            <div className="flex justify-end gap-2">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setForm(null);
                  setPassword("");
                  setProblem("");
                }}
              >
                <IconCancel aria-hidden />
                Discard
              </Button>
              <Button size="sm" disabled={busy} onClick={save}>
                <IconConfirm aria-hidden />
                Save environment
              </Button>
            </div>
          </div>
        )}
      </div>

      {problem && (
        <p role="alert" className="text-xs text-danger">
          {problem}
        </p>
      )}
      <div className="flex items-center justify-between gap-2">
        <Button size="sm" variant="outline" disabled={form !== null} onClick={startAdd}>
          <IconAdd aria-hidden />
          Add environment
        </Button>
        <Button size="sm" variant="ghost" onClick={onClose}>
          <IconCancel aria-hidden />
          Close
        </Button>
      </div>
    </Modal>
  );
}
