//! Where sqlcmd is, what it is handed, and what comes back.
//!
//! One process, spawned directly with a separate string per argument -
//! never a shell, so the password is never a token anything could
//! re-parse. The statement travels in `-Q`, so a multi-line SELECT stays
//! one batch. Everything that leaves this module - a failure, and the rows
//! themselves - has been through `hide_password` first.
//!
//! The password is NOT an argument. An argument can be read out of the
//! process list (`tasklist /v`, Process Explorer, `wmic process`) for as
//! long as sqlcmd runs, so it travels in sqlcmd's own `SQLCMDPASSWORD`
//! environment variable, set on the child process only - the same way the
//! pms-sql skill has always run it. Both sqlcmd 15 (the ODBC tools) and
//! go-sqlcmd read it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::guard;

/// At most this many data rows come back from one statement. The header
/// line is not one of them.
pub const ROW_CAP: usize = 200;

/// And at most this many characters, however few rows that is: one wide
/// column can be bigger than two hundred narrow rows.
pub const CHAR_CAP: usize = 60_000;

/// How long sqlcmd gets, both as its own query timeout and as the wall
/// clock the process is killed against.
pub const TIMEOUT_SECS: u64 = 30;

/// How long sqlcmd waits to connect before it gives up. Without it sqlcmd
/// 15 waits its own default, and a server that is off the network then
/// costs most of `TIMEOUT_SECS` before anyone hears why.
pub const LOGIN_TIMEOUT_SECS: u64 = 15;

/// The variable sqlcmd reads the password from when `-P` is not given.
pub const PASSWORD_ENV: &str = "SQLCMDPASSWORD";

/// Where sqlcmd cuts a variable-length value when `-W` is on. `-y` would
/// widen it, and sqlcmd 15 refuses `-y` together with `-W` - so a value
/// exactly this long may have been cut, and the answer says so.
pub const CUT_AT: usize = 256;

/// What to say when sqlcmd is nowhere on the machine.
pub const NOT_INSTALLED: &str = "sqlcmd is not installed on this machine - install it with `winget install sqlcmd` (or the SQL Server command line tools), then try again";

const EXE: &str = "sqlcmd.exe";

/// A console window would flash on every statement without this.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// One connection, read out of a connection string.
#[derive(Clone, PartialEq)]
pub struct Connection {
    pub server: String,
    pub database: String,
    pub user: String,
    pub password: String,
    pub trust_cert: bool,
}

/// Hand-written so the password cannot reach a log line, a panic message or
/// a bug report through a stray `{:?}`.
impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("server", &self.server)
            .field("database", &self.database)
            .field("user", &self.user)
            .field("password", &"(hidden)")
            .field("trust_cert", &self.trust_cert)
            .finish()
    }
}

/// Reads the `Server=;Database=;User Id=;Password=;TrustServerCertificate=`
/// keys, case-insensitively and ignoring spaces inside the key name. A
/// missing key names itself and nothing else: the error is shown to a
/// person, and the rest of the string is a credential.
pub fn parse_connection(connection_string: &str) -> Result<Connection, String> {
    let mut server = String::new();
    let mut database = String::new();
    let mut user = String::new();
    let mut password = String::new();
    let mut trust_cert = false;

    for part in connection_string.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = guard::normalised_key(key);
        let value = value.trim();
        // The user id is read from the list the guard reads it from, so the
        // two can never disagree about who is signing in.
        if guard::USER_ID_KEYS.contains(&key.as_str()) {
            user = value.to_string();
            continue;
        }
        match key.as_str() {
            "server" | "datasource" | "address" | "addr" => server = value.to_string(),
            "database" | "initialcatalog" => database = value.to_string(),
            "password" | "pwd" => password = value.to_string(),
            "trustservercertificate" => {
                trust_cert = matches!(value.to_ascii_lowercase().as_str(), "true" | "yes" | "1")
            }
            _ => {}
        }
    }

    for (name, value) in [
        ("Server", &server),
        ("Database", &database),
        ("User Id", &user),
        ("Password", &password),
    ] {
        if value.is_empty() {
            return Err(format!("the connection string has no {name}="));
        }
    }
    Ok(Connection { server, database, user, password, trust_cert })
}

/// The environment variable that decides where sqlcmd is, overriding the
/// search below.
///
/// It is AUTHORITATIVE rather than a first candidate: set to a path that
/// is not a file, the answer is "not installed", not "keep looking". That
/// is what lets a test on a machine which HAS sqlcmd (this one does) see
/// the answer a machine without it gets, and it is the honest reading
/// anyway - a person who names a path meant that path.
pub const SQLCMD_OVERRIDE: &str = "TCM_SQLCMD";

/// Where sqlcmd is on THIS machine: the override if one is set, otherwise
/// the search below over the real environment. The one impure finder;
/// `find_sqlcmd` takes its environment as arguments and stays testable.
pub fn sqlcmd_path() -> Option<PathBuf> {
    // An empty variable is not a choice - it is how a shell passes "unset"
    // by accident, and reading it as "sqlcmd is missing" would break the
    // feature for a reason nobody could see.
    if let Some(raw) = std::env::var(SQLCMD_OVERRIDE).ok().filter(|v| !v.trim().is_empty()) {
        let picked = PathBuf::from(raw.trim());
        return picked.is_file().then_some(picked);
    }
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    find_sqlcmd(&var("PATH"), &var("ProgramFiles"), &var("ProgramFiles(x86)"))
}

/// Where sqlcmd is: PATH first, then the folders the Microsoft installers
/// and go-sqlcmd use. PATH leads because a person who installed it
/// deliberately put it there.
pub fn find_sqlcmd(
    path_var: &str,
    program_files: &str,
    program_files_x86: &str,
) -> Option<PathBuf> {
    sqlcmd_candidates(path_var, program_files, program_files_x86).into_iter().find(|p| p.is_file())
}

fn sqlcmd_candidates(
    path_var: &str,
    program_files: &str,
    program_files_x86: &str,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in path_var.split(';') {
        let entry = entry.trim().trim_matches('"').trim();
        if entry.is_empty() {
            continue;
        }
        out.push(Path::new(entry).join(EXE));
    }
    for root in [program_files, program_files_x86] {
        if root.is_empty() {
            continue;
        }
        for version in ["170", "180"] {
            out.push(
                Path::new(root)
                    .join("Microsoft SQL Server")
                    .join("Client SDK")
                    .join("ODBC")
                    .join(version)
                    .join("Tools")
                    .join("Binn")
                    .join(EXE),
            );
        }
    }
    // go-sqlcmd, which is what `winget install sqlcmd` puts on a machine.
    if !program_files.is_empty() {
        out.push(Path::new(program_files).join("sqlcmd").join(EXE));
    }
    out
}

/// What one run of sqlcmd produced.
pub struct Output {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The one process this app spawns for SQL. Faked in tests through this
/// trait, so every test above can assert the arguments without a server.
///
/// There is no stdin: the statement goes through `-Q` so that a multi-line
/// SELECT stays one batch, and nothing else is ever written to the process.
pub trait Runner {
    fn run(
        &self,
        exe: &Path,
        args: &[String],
        env: &[(String, String)],
        timeout: Duration,
    ) -> impl std::future::Future<Output = Result<Output, String>>;
}

/// The real one.
pub struct RealRunner;

impl Runner for RealRunner {
    async fn run(
        &self,
        exe: &Path,
        args: &[String],
        env: &[(String, String)],
        timeout: Duration,
    ) -> Result<Output, String> {
        use std::process::Stdio;

        let mut command = tokio::process::Command::new(exe);
        command
            .args(args)
            // The child's environment only - the app's own is never touched.
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // The process dies with the future: a sqlcmd nobody is waiting
            // for any more must not keep a connection open behind the app.
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let child = command.spawn().map_err(|e| format!("sqlcmd could not be started: {e}"))?;

        match tokio::time::timeout(timeout, child.wait_with_output()).await {
            Err(_) => Err(format!("sqlcmd did not answer within {} s", timeout.as_secs())),
            Ok(Err(e)) => Err(format!("sqlcmd could not be read: {e}")),
            Ok(Ok(out)) => Ok(Output {
                status: out.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            }),
        }
    }
}

/// The argument list for sqlcmd. Every item is its own string: there is no
/// command line to quote and therefore nothing to escape, which is the
/// whole reason the password can be passed at all.
///
/// The flags, the set the pms-sql skill runs and both sqlcmd 15 (the ODBC
/// tools) and go-sqlcmd accept: `-S` server, `-d` database, `-U` user,
/// `-C` trust the server certificate, `-b` exit on error, `-f 65001`
/// UTF-8 in and out, `-l` login timeout, `-t` query timeout, `-W` trim
/// trailing spaces, `-s` column separator, `-Q` the statement and exit.
///
/// Two things deliberately absent. `-y`/`-Y`: sqlcmd 15 refuses them
/// together with `-W` ("The -W and the -y/-Y options are mutually
/// exclusive") before it dials anything - 1.25.16/17 passed both, and every
/// call failed on a machine with the ODBC tools' sqlcmd. `-y 0` alone
/// keeps whole values but drops the header row, which is what names the
/// columns, so `-W` wins and a value at `CUT_AT` is flagged instead. And
/// `-h`, for the same reason. The password is not here at all - see
/// `sqlcmd_env`.
pub fn sqlcmd_args(c: &Connection, sql: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-S".into(),
        c.server.clone(),
        "-d".into(),
        c.database.clone(),
        "-U".into(),
        c.user.clone(),
    ];
    if c.trust_cert {
        args.push("-C".into());
    }
    args.extend([
        "-b".into(),
        "-f".into(),
        "65001".into(),
        "-l".into(),
        LOGIN_TIMEOUT_SECS.to_string(),
        "-t".into(),
        TIMEOUT_SECS.to_string(),
        "-W".into(),
        "-s".into(),
        "\t".into(),
        "-Q".into(),
        sql.to_string(),
    ]);
    args
}

/// The child process's environment: the password, where sqlcmd looks for it
/// when `-P` is not given. Never an argument - see the module note.
pub fn sqlcmd_env(c: &Connection) -> Vec<(String, String)> {
    vec![(PASSWORD_ENV.to_string(), c.password.clone())]
}

/// A notice when some value is exactly `CUT_AT` characters long - where
/// sqlcmd stops. The header and the dashes rule under it are skipped.
fn cut_notice(stdout: &str) -> Option<String> {
    let hit = stdout
        .lines()
        .skip(2)
        .any(|line| line.split('\t').any(|cell| cell.trim_end().chars().count() == CUT_AT));
    hit.then(|| {
        format!(
            "... a value is exactly {CUT_AT} characters, which is where sqlcmd cuts text - it may be longer: select SUBSTRING(column, {}, {CUT_AT}) to read on",
            CUT_AT + 1
        )
    })
}

/// Runs one statement and returns the tab-separated text, capped, and
/// whether either cap fired.
///
/// The guard is asked here rather than trusted to have been asked: this is
/// the only function that reaches a `Runner`, so classifying inside it is
/// what makes "nothing runs unclassified" a property of the code and not a
/// convention. The connection's own user id decides whether a write passes.
pub async fn run_sql<R: Runner>(
    r: &R,
    exe: &Path,
    c: &Connection,
    sql: &str,
) -> Result<(String, bool), String> {
    guard::allowed(sql, guard::access_for_user(&c.user))?;

    let args = sqlcmd_args(c, sql);
    let out = r
        .run(exe, &args, &sqlcmd_env(c), Duration::from_secs(TIMEOUT_SECS))
        .await
        .map_err(|e| hide_password(&e, &c.password))?;

    if out.status != 0 {
        // sqlcmd puts server errors on stdout unless asked otherwise, so
        // whichever of the two said something is the message.
        let said = if out.stderr.trim().is_empty() { out.stdout.trim() } else { out.stderr.trim() };
        let said = if said.is_empty() {
            format!("sqlcmd stopped with status {}", out.status)
        } else {
            // sqlcmd prints a connection failure twice - once as it fails
            // and once as the batch it never ran. Saying it once is enough.
            undouble(said)
        };
        return Err(hide_password(&said, &c.password));
    }
    let (mut text, capped) = cap(&out.stdout);
    if let Some(notice) = cut_notice(&out.stdout) {
        text.push('\n');
        text.push_str(&notice);
    }
    Ok((hide_password(&text, &c.password), capped))
}

/// Keeps the header line, at most `ROW_CAP` rows after it, and at most
/// `CHAR_CAP` characters of that - then says, after the body, each cut it
/// had to make. Both notices can appear; neither ever lands inside a row.
///
/// Returns whether either cap fired, alongside the text, so a caller that
/// wants to say "capped" does not have to guess from the text itself - a
/// SELECT can return a value that happens to contain the same words.
fn cap(stdout: &str) -> (String, bool) {
    let mut lines = stdout.lines();
    let mut body = lines.next().unwrap_or("").to_string();
    let rows: Vec<&str> = lines.collect();
    for row in rows.iter().take(ROW_CAP) {
        body.push('\n');
        body.push_str(row);
    }

    let mut capped = false;
    let mut notices: Vec<String> = Vec::new();
    if rows.len() > ROW_CAP {
        notices.push(format!("... {} more rows (capped)", rows.len() - ROW_CAP));
        capped = true;
    }
    if body.chars().count() > CHAR_CAP {
        let kept: String = body.chars().take(CHAR_CAP).collect();
        // Cut between rows, so nothing is left half-written and read as
        // data. The exception is a single row wider than the whole cap:
        // trimming to the line boundary there would hand back the header
        // and nothing else, so that one is cut where it falls.
        body = match kept.rfind('\n') {
            Some(at) if kept[..at].lines().count() > 1 => kept[..at].to_string(),
            _ => kept,
        };
        notices.push(format!("... output capped at {CHAR_CAP} characters"));
        capped = true;
    }

    for notice in notices {
        body.push('\n');
        body.push_str(&notice);
    }
    (body, capped)
}

/// Drops a line that only repeats the one before it.
fn undouble(said: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for line in said.lines() {
        if kept.last() != Some(&line) {
            kept.push(line);
        }
    }
    kept.join("\n")
}

/// sqlcmd echoes the whole command line back in some failures. Nothing
/// leaves this module without going through here first.
fn hide_password(text: &str, password: &str) -> String {
    if password.is_empty() {
        return text.to_string();
    }
    text.replace(password, "(hidden)")
}
