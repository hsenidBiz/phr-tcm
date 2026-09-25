import { useId, useState, type ReactNode } from "react";
import { commands, type DbCredentialsForm, type DbDatabase } from "../bindings";
import { IconCancel, IconConfirm, IconUndo } from "../lib/actionIcons";
import { cn } from "../lib/cn";
import { toast } from "../lib/toast";
import { Button } from "./ui/button";
import { Checkbox } from "./ui/checkbox";
import { Input } from "./ui/input";
import { Modal } from "./ui/modal";

/**
 * One database's login, edited where it lives: Rust keeps it in Windows
 * Credential Manager, and this form only ever sends one IN.
 *
 * The password is never shown back - not even the saved one, which the
 * webview does not have. It exists here only in this form's own state while
 * it is typed, and goes with the form when it closes. A blank password
 * means "keep the saved one", for Test connection and Save alike.
 */
export function DbCredentialsModal({
  database,
  onClose,
  onSaved,
}: {
  database: DbDatabase;
  onClose: () => void;
  /** The database's new public view, after a save or a reset. */
  onSaved: (d: DbDatabase) => void;
}) {
  const titleId = useId();
  const [server, setServer] = useState(database.server);
  const [port, setPort] = useState(database.port == null ? "" : String(database.port));
  const [name, setName] = useState(database.database);
  const [user, setUser] = useState(database.user);
  const [password, setPassword] = useState("");
  const [trustCert, setTrustCert] = useState(database.trust_cert);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState<"test" | "save" | "reset" | null>(null);

  /** The form as Rust takes it, or the reason it cannot be sent. */
  const form = (): DbCredentialsForm | string => {
    const p = port.trim();
    const n = Number(p);
    if (p && !(/^\d+$/.test(p) && n >= 1 && n <= 65535)) {
      return "The port is a number from 1 to 65535.";
    }
    return {
      server: server.trim(),
      port: p ? n : null,
      database: name.trim(),
      user: user.trim(),
      password: password === "" ? null : password,
      trust_cert: trustCert,
    };
  };

  /** Runs one command, showing a refusal under the fields. */
  const run = async <T,>(
    kind: "test" | "save" | "reset",
    call: () => Promise<{ status: "ok"; data: T } | { status: "error"; error: string }>,
  ): Promise<T | null> => {
    setBusy(kind);
    setResult(null);
    try {
      const res = await call();
      if (res.status === "error") {
        setResult({ ok: false, text: res.error });
        return null;
      }
      return res.data;
    } catch (e) {
      setResult({ ok: false, text: e instanceof Error ? e.message : String(e) });
      return null;
    } finally {
      setBusy(null);
    }
  };

  const withForm = (then: (f: DbCredentialsForm) => void) => {
    const f = form();
    if (typeof f === "string") setResult({ ok: false, text: f });
    else then(f);
  };

  const test = () =>
    withForm(async (f) => {
      const said = await run("test", () => commands.testDbConnection(database.id, f));
      if (said != null) setResult({ ok: true, text: said });
    });

  const save = () =>
    withForm(async (f) => {
      const saved = await run("save", () => commands.saveDbCredentials(database.id, f));
      if (!saved) return;
      toast.success(`Saved the login for ${database.label}.`);
      onSaved(saved);
      onClose();
    });

  const reset = async () => {
    const back = await run("reset", () => commands.resetDbCredentials(database.id));
    if (!back) return;
    toast.success(`${database.label} is back to its default login.`);
    onSaved(back);
    onClose();
  };

  const field = (label: string, control: ReactNode, className?: string) => (
    <label className={cn("block text-xs text-muted", className)}>
      {label}
      {control}
    </label>
  );
  const inputClass = "mt-1 w-full py-1.5 text-xs";

  return (
    <Modal onClose={onClose} labelledBy={titleId} className="flex w-[440px] max-w-full flex-col gap-3 p-4">
      <h2 id={titleId} className="text-sm font-semibold text-text">
        Credentials for {database.label}
      </h2>

      {database.shipped ? (
        // The app's own databases: where they are is the app's, who signs
        // in is the person's.
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs">
          <dt className="text-muted">Server</dt>
          <dd className="id-mono break-all text-text">
            {database.server}
            {database.port != null ? `,${database.port}` : ""}
          </dd>
          <dt className="text-muted">Database</dt>
          <dd className="id-mono break-all text-text">{database.database}</dd>
        </dl>
      ) : (
        <>
          <div className="flex gap-2">
            {field(
              "Server",
              <Input className={inputClass} value={server} onChange={(e) => setServer(e.target.value)} />,
              "min-w-0 flex-1",
            )}
            {field(
              "Port",
              <Input
                className={inputClass}
                inputMode="numeric"
                placeholder="1433"
                value={port}
                onChange={(e) => setPort(e.target.value)}
              />,
              "w-20",
            )}
          </div>
          {field(
            "Database",
            <Input className={inputClass} value={name} onChange={(e) => setName(e.target.value)} />,
          )}
        </>
      )}

      <div className="flex gap-2">
        {field(
          "User",
          <Input className={inputClass} value={user} onChange={(e) => setUser(e.target.value)} />,
          "min-w-0 flex-1",
        )}
        {field(
          "Password",
          <Input
            type="password"
            // Never offered to a password manager, nor filled from one: the
            // login's home is Windows Credential Manager.
            autoComplete="new-password"
            className={inputClass}
            placeholder={database.has_password ? "Saved - leave blank to keep it" : undefined}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />,
          "min-w-0 flex-1",
        )}
      </div>

      {!database.shipped && (
        <label className="flex items-center gap-2 text-xs text-muted">
          <Checkbox ariaLabel="Trust the server certificate" checked={trustCert} onCheckedChange={setTrustCert} />
          Trust the server certificate
        </label>
      )}

      {result && (
        <p role="status" className={cn("break-words text-xs", result.ok ? "text-success" : "text-danger")}>
          {result.text}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-2">
        <Button size="sm" variant="outline" disabled={busy != null} onClick={test}>
          {busy === "test" ? "Testing" : "Test connection"}
        </Button>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          {database.shipped && database.customised && (
            <Button size="sm" variant="ghost" disabled={busy != null} onClick={() => void reset()}>
              <IconUndo aria-hidden />
              {busy === "reset" ? "Resetting" : "Reset to default"}
            </Button>
          )}
          <Button size="sm" variant="ghost" onClick={onClose}>
            <IconCancel aria-hidden />
            Cancel
          </Button>
          <Button size="sm" disabled={busy != null} onClick={save}>
            <IconConfirm aria-hidden />
            {busy === "save" ? "Saving" : "Save"}
          </Button>
        </div>
      </div>
    </Modal>
  );
}
