//! Where sqlcmd is, what it is handed, and what comes back.
//!
//! One process, spawned directly with a separate string per argument -
//! never a shell, so the password is never a token anything could
//! re-parse. The statement travels in `-Q`, so a multi-line SELECT stays
//! one batch. Everything that leaves this module has been through
//! `hide_password` first.

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
        let key: String =
            key.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
        let value = value.trim();
        match key.as_str() {
            "server" | "datasource" | "address" | "addr" => server = value.to_string(),
            "database" | "initialcatalog" => database = value.to_string(),
            "userid" | "uid" | "user" => user = value.to_string(),
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
pub trait Runner {
    fn run(
        &self,
        exe: &Path,
        args: &[String],
        stdin: &str,
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
        stdin: &str,
        timeout: Duration,
    ) -> Result<Output, String> {
        use std::process::Stdio;

        let mut command = tokio::process::Command::new(exe);
        command
            .args(args)
            .stdin(if stdin.is_empty() { Stdio::null() } else { Stdio::piped() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // The process dies with the future: a sqlcmd nobody is waiting
            // for any more must not keep a connection open behind the app.
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);

        let mut child = command.spawn().map_err(|e| format!("sqlcmd could not be started: {e}"))?;
        if !stdin.is_empty() {
            if let Some(mut pipe) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                pipe.write_all(stdin.as_bytes())
                    .await
                    .map_err(|e| format!("sqlcmd would not take the statement: {e}"))?;
                let _ = pipe.shutdown().await;
            }
        }

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
/// The flags, checked against go-sqlcmd 1.10's own `-?`: `-S` server, `-d`
/// database, `-U` user, `-P` password, `-C` trust the server certificate,
/// `-s` column separator, `-W` trim trailing spaces, `-t` query timeout,
/// `-b` exit on error, `-y`/`-Y` no width truncation, `-Q` the statement
/// and exit. `-h` is deliberately absent: the header row is what names the
/// columns in the answer.
pub fn sqlcmd_args(c: &Connection, sql: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-S".into(),
        c.server.clone(),
        "-d".into(),
        c.database.clone(),
        "-U".into(),
        c.user.clone(),
        "-P".into(),
        c.password.clone(),
    ];
    if c.trust_cert {
        args.push("-C".into());
    }
    args.extend([
        "-s".into(),
        "\t".into(),
        "-W".into(),
        "-y".into(),
        "0".into(),
        "-Y".into(),
        "0".into(),
        "-t".into(),
        TIMEOUT_SECS.to_string(),
        "-b".into(),
        "-Q".into(),
        sql.to_string(),
    ]);
    args
}

/// Runs one statement and returns the tab-separated text, capped.
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
) -> Result<String, String> {
    guard::allowed(sql, guard::access_for_user(&c.user))?;

    let args = sqlcmd_args(c, sql);
    let out = r
        .run(exe, &args, "", Duration::from_secs(TIMEOUT_SECS))
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
    Ok(cap(&out.stdout))
}

/// Keeps the header line, at most `ROW_CAP` rows after it, and at most
/// `CHAR_CAP` characters overall, saying so on a last line when it cut.
fn cap(stdout: &str) -> String {
    let mut lines = stdout.lines();
    let mut out = lines.next().unwrap_or("").to_string();
    let rows: Vec<&str> = lines.collect();
    for row in rows.iter().take(ROW_CAP) {
        out.push('\n');
        out.push_str(row);
    }
    if rows.len() > ROW_CAP {
        out.push_str(&format!("\n... {} more rows (capped)", rows.len() - ROW_CAP));
    }
    if out.chars().count() > CHAR_CAP {
        let kept: String = out.chars().take(CHAR_CAP).collect();
        out = format!("{kept}\n... output capped at {CHAR_CAP} characters");
    }
    out
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
