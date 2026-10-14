//! The Playwright tab's test accounts: the repo's `npm run users` CLI
//! (`cli/users/users.mjs`), run from the app, so the repo's own checks apply
//! to every change (a rename rewrites every reference, a referenced account
//! cannot be deleted, a changed username or password clears that account's
//! saved session).
//!
//! - **The list** (`accounts`) is read from `src/users/users.json`, read
//!   only: no process, no run slot. Each password is left where it is
//!   (`RepoAccount` has no password field).
//! - **The reference counts** (`refs`) come after, from `npm run users --
//!   list --json` (its `specs` and `otherRefs`), a short repo command. The
//!   CLI puts each password in that JSON too; only the key and the counts
//!   are parsed (`AccountRefs`), and the listing's output never reaches a
//!   sink, a log or an error.
//! - **Changes** are `add`, `set`, `rm` and `rename --cascade`, built by
//!   `change_args`. Without `--apply` the CLI writes nothing and prints what
//!   it would do; that preview is what a change without `apply` returns. With
//!   `apply` the change is written and the accounts are read again from the
//!   registry. A refusal (exit code other than 0) comes back as the CLI said
//!   it.
//! - **Show** (`password`) reads one password from `src/users/users.json`,
//!   read only. The CLI has no command that reads one password: its `list
//!   --json` carries every password at once, and as a repo command it would
//!   wait for a test run to finish. Nothing else in the app reads a password.
//!
//! **How a password reaches the CLI.** The CLI takes a password only as an
//! argument (`add … --password P`, `set <key> password P`); it reads no
//! stdin and no environment variable for it. So for the few seconds an
//! `add` or a password `set` runs, the password is in the command line of
//! that process (and of the `cmd.exe` npm runs the script through), where
//! any program of the same user that lists processes can read it, for that
//! run's lifetime. That is accepted: the same user can read `users.json`
//! itself. No file keeps it: npm's own debug log, which records the whole
//! argv, goes to the app's own folder with `--logs-max=0` and that folder is
//! emptied after every run (`process::npm_args`), never to the user's
//! shared `npm-cache/_logs`.
//!
//! **Where a password never goes.** Not into the app's log (`process` logs
//! only the run's label, `npm run users`), not into `PwOutput` (no output
//! line of an accounts command is emitted, only its `PwDone`), and not into
//! an error or a preview: every line the CLI prints is cleaned
//! (`clean_line`) before it is kept, with passwords replaced by `HIDDEN` and
//! a JSON parse error (whose message can quote the file) replaced whole.
//! The change's own password is always hidden, whatever its length; the
//! other passwords in `users.json` only from `MIN_HIDDEN` characters up, so
//! a one or two letter password does not blank every match of those letters
//! in the output. `--silent` (added by `process`) keeps npm's `> node …
//! <args>` banner out of the output.
//!
//! **Values.** A value starting with `--` (the CLI would read it as an
//! option) or holding a control character (a line break ends `cmd.exe`'s
//! command line there, silently dropping what follows, `--apply` included)
//! is refused before anything runs.

use super::process::{self, Launch, RunEvent, Sink};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// The account registry, relative to the clone. Read here, never written:
/// only the CLI's `--apply` writes it.
pub const REGISTRY: &str = "src/users/users.json";
/// What a password in the CLI's output is replaced with.
pub const HIDDEN: &str = "[hidden]";
/// How long one accounts command may take.
pub const USERS_TIMEOUT: Duration = Duration::from_secs(120);

pub const NO_REGISTRY: &str = "The clone has no src/users/users.json.";
pub const REGISTRY_NOT_JSON: &str = "src/users/users.json is not valid JSON, so the accounts could not be read.";
pub const NO_SUCH_ACCOUNT: &str = "That account is not in src/users/users.json.";
pub const NO_PASSWORD: &str = "That account has no password in src/users/users.json.";
pub const LIST_UNREADABLE: &str = "The list of accounts from npm run users could not be read.";
pub const USERS_STOPPED: &str = "The users command was stopped before it finished.";
pub const USERS_TIMED_OUT: &str = "The users command did not finish within 2 minutes, so it was stopped.";
pub const CONTROL_CHARACTER: &str =
    "A value cannot hold a line break, a tab or another control character, because npm run users would not receive it whole. Nothing was run.";
/// The shortest password in `users.json` hidden in output (the change's own
/// password is hidden whatever its length).
pub const MIN_HIDDEN: usize = 4;
pub const OPTION_LIKE: &str = "A value cannot start with --, because npm run users would read it as an option. Nothing was run.";

/// Starts `npm` with arguments (`run users -- …`): in the app
/// `Launch::repo` with npm, in the tests a fake.
pub type Starter<'a> = &'a dyn Fn(&[String]) -> Result<Launch, String>;

// ---- Types the webview sees ------------------------------------------------

/// One account of the repo's registry. Holds no password: Show reads one
/// on its own (`password`). Its reference counts come separately
/// (`AccountRefs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct RepoAccount {
    /// `<environment>.<module>.<role>[.<discriminator>]`.
    pub key: String,
    pub username: String,
    /// Empty when the account has none.
    pub employee_no: String,
    pub display_name: String,
    /// `empTitle` in the registry.
    pub title: String,
    /// Another account's key, or empty.
    pub supervisor: String,
}

/// What uses one account, from `npm run users -- list --json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct AccountRefs {
    pub key: String,
    /// Specs that sign in as it (`test.use` or `actingAs`).
    pub specs: u32,
    /// Other references: test case `**User:**` lines and supervisor
    /// pointers. Any reference at all refuses a delete.
    pub other_refs: u32,
}

/// One field `set` changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub enum AccountField {
    Username,
    Password,
    EmployeeNo,
    DisplayName,
    Title,
    Supervisor,
}

impl AccountField {
    /// The field's name to the CLI.
    pub fn cli_name(self) -> &'static str {
        match self {
            AccountField::Username => "username",
            AccountField::Password => "password",
            AccountField::EmployeeNo => "employeeNo",
            AccountField::DisplayName => "displayName",
            AccountField::Title => "empTitle",
            AccountField::Supervisor => "supervisor",
        }
    }
}

/// A change to the registry. Deliberately not `Debug`: it can hold a
/// password, and nothing should be able to print one by accident.
#[derive(Clone, Deserialize, specta::Type)]
#[serde(tag = "kind")]
pub enum AccountChange {
    /// A new account. The optional fields are left out when empty.
    Add {
        key: String,
        username: String,
        password: String,
        employee_no: String,
        display_name: String,
        title: String,
        supervisor: String,
    },
    /// One field of an existing account. An empty value clears an optional
    /// field (the CLI's `-`).
    Set { key: String, field: AccountField, value: String },
    /// Delete an account. The CLI refuses while anything references it.
    Remove { key: String },
    /// Move an account to a new key, rewriting every reference (`--cascade`).
    Rename { from: String, to: String },
}

/// What a change answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct ChangeOutcome {
    /// Whether it was written (`--apply`), or only previewed.
    pub applied: bool,
    /// What the CLI printed, passwords hidden.
    pub output: Vec<String>,
    /// After an applied change, the accounts read again (`accounts`).
    pub accounts: Option<Vec<RepoAccount>>,
    /// Why reading them again after an applied change failed, if it did.
    /// The change itself was written.
    pub list_error: Option<String>,
}

// ---- Arguments -----------------------------------------------------------------

fn npm(rest: Vec<String>) -> Vec<String> {
    ["run", "users", "--"].into_iter().map(String::from).chain(rest).collect()
}

/// `npm` arguments for listing the accounts (for their reference counts).
pub fn list_args() -> Vec<String> {
    npm(vec!["list".into(), "--json".into()])
}

/// `npm` arguments for `change`, with `--apply` when `apply`. Refused when
/// a value starts with `--`: the CLI would read it as an option.
pub fn change_args(change: &AccountChange, apply: bool) -> Result<Vec<String>, String> {
    let s = |v: &str| v.to_string();
    let mut rest: Vec<String> = match change {
        AccountChange::Add { key, username, password, employee_no, display_name, title, supervisor } => {
            let mut a = vec![s("add"), key.trim().to_string(), s("--username"), username.clone(), s("--password"), password.clone()];
            for (flag, value) in [("--employee-no", employee_no), ("--display-name", display_name), ("--emp-title", title), ("--supervisor", supervisor)] {
                if !value.trim().is_empty() {
                    a.push(s(flag));
                    a.push(value.clone());
                }
            }
            a
        }
        AccountChange::Set { key, field, value } => {
            let value = if value.trim().is_empty() { s("-") } else { value.clone() };
            vec![s("set"), key.trim().to_string(), s(field.cli_name()), value]
        }
        AccountChange::Remove { key } => vec![s("rm"), key.trim().to_string()],
        AccountChange::Rename { from, to } => vec![s("rename"), from.trim().to_string(), to.trim().to_string(), s("--cascade")],
    };
    if values(change).iter().any(|v| v.trim_start().starts_with("--")) {
        return Err(OPTION_LIKE.to_string());
    }
    if values(change).iter().any(|v| v.chars().any(char::is_control)) {
        return Err(CONTROL_CHARACTER.to_string());
    }
    if apply {
        rest.push(s("--apply"));
    }
    Ok(npm(rest))
}

/// Every value a change carries.
fn values(change: &AccountChange) -> Vec<&str> {
    match change {
        AccountChange::Add { key, username, password, employee_no, display_name, title, supervisor } => {
            vec![key, username, password, employee_no, display_name, title, supervisor]
        }
        AccountChange::Set { key, value, .. } => vec![key, value],
        AccountChange::Remove { key } => vec![key],
        AccountChange::Rename { from, to } => vec![from, to],
    }
    .into_iter()
    .map(String::as_str)
    .collect()
}

/// The passwords a change carries: an added account's, or a password `set`'s
/// (as given and as the CLI stores it, trimmed).
pub fn change_secrets(change: &AccountChange) -> Vec<String> {
    let raw = match change {
        AccountChange::Add { password, .. } => password.as_str(),
        AccountChange::Set { field: AccountField::Password, value, .. } => value.as_str(),
        _ => return vec![],
    };
    [raw, raw.trim()].into_iter().filter(|p| !p.is_empty()).map(String::from).collect()
}

// ---- Reading the registry (read only) -------------------------------------------

fn read_registry(clone: &Path) -> Result<serde_json::Map<String, Value>, String> {
    let text = std::fs::read_to_string(clone.join(REGISTRY)).map_err(|_| NO_REGISTRY.to_string())?;
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(REGISTRY_NOT_JSON.to_string()),
    }
}

/// Every password in the clone's registry. Empty when it cannot be read.
pub fn registry_passwords(clone: &Path) -> Vec<String> {
    read_registry(clone)
        .map(|m| m.values().filter_map(|e| e.get("password")?.as_str().map(String::from)).filter(|p| !p.is_empty()).collect())
        .unwrap_or_default()
}

/// The clone's accounts, in the registry's order, read from
/// `src/users/users.json` and nothing else: no process starts. Each
/// account's password stays in the file.
pub fn accounts(clone: &Path) -> Result<Vec<RepoAccount>, String> {
    let text = |e: &Value, k: &str| match e.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    Ok(read_registry(clone)?
        .iter()
        .filter(|(_, e)| e.is_object())
        .map(|(key, e)| RepoAccount {
            key: key.clone(),
            username: text(e, "username"),
            employee_no: text(e, "employeeNo"),
            display_name: text(e, "displayName"),
            title: text(e, "empTitle"),
            supervisor: text(e, "supervisor"),
        })
        .collect())
}

/// Account `key`'s password, for Show only. Read from the registry; never
/// logged.
pub fn password(clone: &Path, key: &str) -> Result<String, String> {
    let registry = read_registry(clone)?;
    let entry = registry.get(key.trim()).ok_or_else(|| NO_SUCH_ACCOUNT.to_string())?;
    match entry.get("password").and_then(Value::as_str) {
        Some(p) if !p.is_empty() => Ok(p.to_string()),
        _ => Err(NO_PASSWORD.to_string()),
    }
}

/// The registry's passwords hidden in output: those of `MIN_HIDDEN`
/// characters or more.
fn hidden_registry_passwords(clone: &Path) -> Vec<String> {
    registry_passwords(clone).into_iter().filter(|p| p.chars().count() >= MIN_HIDDEN).collect()
}

// ---- Parsing and cleaning output -------------------------------------------------

/// One row of `list --json`: the key and the counts only. The `password`
/// the CLI includes is not a field, so it is never kept.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListRow {
    key: String,
    #[serde(default)]
    specs: u32,
    #[serde(default)]
    other_refs: u32,
}

/// Each account's reference counts in `list --json`'s stdout. Anything
/// ahead of the JSON array is passed over. An error never quotes the text:
/// it holds passwords.
pub fn parse_list(stdout: &str) -> Result<Vec<AccountRefs>, String> {
    let start = stdout.find('[').ok_or_else(|| LIST_UNREADABLE.to_string())?;
    let rows: Vec<ListRow> = serde_json::from_str(&stdout[start..]).map_err(|_| LIST_UNREADABLE.to_string())?;
    Ok(rows.into_iter().map(|r| AccountRefs { key: r.key, specs: r.specs, other_refs: r.other_refs }).collect())
}

/// One output line made safe to keep and show: a JSON parse error (Node
/// quotes the text it could not parse) becomes `REGISTRY_NOT_JSON`, and
/// every one of `secrets` becomes `HIDDEN`, longest first.
pub fn clean_line(line: &str, secrets: &[String]) -> String {
    if line.contains("is not valid JSON") || line.contains("in JSON at position") || line.contains("Unexpected token") {
        return REGISTRY_NOT_JSON.to_string();
    }
    // Each as typed, and as JSON writes it (a `"` or `\` escaped), which is
    // how a password reaches output the CLI prints as JSON.
    let mut forms: Vec<String> = Vec::new();
    for s in secrets.iter().filter(|s| !s.is_empty()) {
        forms.push(s.clone());
        let escaped = serde_json::to_string(s).unwrap_or_default();
        let escaped = escaped.get(1..escaped.len().saturating_sub(1)).unwrap_or_default().to_string();
        if !escaped.is_empty() && escaped != *s {
            forms.push(escaped);
        }
    }
    forms.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let mut out = line.to_string();
    for s in forms {
        out = out.replace(s.as_str(), HIDDEN);
    }
    out
}

// ---- Running ---------------------------------------------------------------------

/// What one accounts command left: its exit code, its stdout as it came
/// (kept only to parse a listing), and every line of both streams, cleaned.
struct Captured {
    code: i32,
    raw_stdout: String,
    /// `(from stderr, cleaned line)`, in the order they came.
    lines: Vec<(bool, String)>,
}

/// Runs `args` (after `npm`) as a repo command, keeping its output to
/// itself. Only its end reaches `show` (`PwDone`); no line does.
fn capture(args: &[String], secrets: &[String], show: &Sink, launch: Starter) -> Result<Captured, String> {
    // Short: a listing may wait for it rather than be refused.
    let launched = launch(args)?.brief();
    let (tx, rx) = mpsc::channel::<(bool, RunEvent)>();
    let sink = |from_err: bool| -> Sink {
        let tx = Mutex::new(tx.clone());
        Arc::new(move |e| {
            let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send((from_err, e));
        })
    };
    let run = process::start_split(launched, sink(false), sink(true))?;
    drop(tx);
    let mut deadline = Instant::now() + USERS_TIMEOUT;
    let mut timed_out = false;
    let mut raw = Vec::new();
    let mut lines = Vec::new();
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((from_err, RunEvent::Output { line, .. })) => {
                lines.push((from_err, clean_line(&line, secrets)));
                if !from_err {
                    raw.push(line);
                }
            }
            Ok((_, RunEvent::Done { run: done, code })) => {
                show(RunEvent::Done { run: done, code });
                if timed_out {
                    return Err(USERS_TIMED_OUT.to_string());
                }
                return match code {
                    None => Err(USERS_STOPPED.to_string()),
                    Some(code) => Ok(Captured { code, raw_stdout: raw.join("\n"), lines }),
                };
            }
            Err(mpsc::RecvTimeoutError::Timeout) if !timed_out => {
                timed_out = true;
                let _ = process::stop(run);
                deadline = Instant::now() + process::OUTPUT_GRACE + Duration::from_secs(5);
            }
            Err(_) => return Err(if timed_out { USERS_TIMED_OUT } else { USERS_STOPPED }.to_string()),
        }
    }
}

/// A failed command's answer: what it printed (cleaned), or its exit code.
fn refusal(lines: &[String], code: i32) -> String {
    let text = lines.iter().map(|l| l.trim_end()).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>().join("\n");
    if text.is_empty() {
        format!("npm run users ended with exit code {code} and said nothing.")
    } else {
        text
    }
}

/// What uses each of the clone's accounts, from `npm run users -- list
/// --json`.
pub fn refs(clone: &Path, show: &Sink, launch: Starter) -> Result<Vec<AccountRefs>, String> {
    let secrets = hidden_registry_passwords(clone);
    let c = capture(&list_args(), &secrets, show, launch)?;
    if c.code != 0 {
        // Only stderr: a listing's stdout is the JSON with every password.
        let errors: Vec<String> = c.lines.into_iter().filter(|(e, _)| *e).map(|(_, l)| l).collect();
        return Err(refusal(&errors, c.code));
    }
    parse_list(&c.raw_stdout)
}

/// Runs `change`: a preview without `apply`, else the change written and
/// the accounts read again from the registry (no second command). A
/// refusal is the CLI's own words.
pub fn change(clone: &Path, change: &AccountChange, apply: bool, show: &Sink, launch: Starter) -> Result<ChangeOutcome, String> {
    let args = change_args(change, apply)?;
    let mut secrets = change_secrets(change);
    secrets.extend(hidden_registry_passwords(clone));
    let c = capture(&args, &secrets, show, launch)?;
    let output: Vec<String> = c.lines.into_iter().map(|(_, l)| l).collect();
    if c.code != 0 {
        return Err(refusal(&output, c.code));
    }
    if !apply {
        return Ok(ChangeOutcome { applied: false, output, accounts: None, list_error: None });
    }
    let (accounts, list_error) = match accounts(clone) {
        Ok(a) => (Some(a), None),
        Err(e) => (None, Some(e)),
    };
    Ok(ChangeOutcome { applied: true, output, accounts, list_error })
}
