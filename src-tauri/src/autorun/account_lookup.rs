//! Account passwords looked up by the app, so they never pass through an AI
//! assistant.
//!
//! Test environments keep their logins in a database table (PeoplesHR:
//! `PeoplesHR.HS_SM_USER_ACCOUNT`) in plain text, for development and testing
//! only. Each environment names that table, its username column and its
//! password column (`AccountLookup`); the app builds ONE SELECT from those
//! names itself - never free SQL - and reads the passwords for the logins it
//! is asked about through the same read-only path the expiry and active
//! session queries use (`RowSource`, which the database guard stands in front
//! of).
//!
//! Rules this module keeps:
//! - Only a test environment looks passwords up (`NOT_TEST`).
//! - Names are plain SQL identifiers: `schema.table` or `db.schema.table`,
//!   each part letters, digits and underscores, optionally in brackets.
//! - A value that is clearly a hash or an encrypted blob is never used
//!   (`looks_like_hash`).
//! - A password is never logged, echoed or quoted by an error. What the
//!   database says about a failure is mapped to a fixed sentence here; its
//!   own words are dropped.

use super::password_expiry::{cell, column, parse_table};
use super::setup_find::{quoted_literal, RowSource};
use crate::db::{guard, query};
use std::collections::HashMap;

/// What a new environment, and one saved before this setting existed, uses.
pub const DEFAULT_TABLE: &str = "PeoplesHR.HS_SM_USER_ACCOUNT";
pub const DEFAULT_USERNAME_COLUMN: &str = "USRACC_USER_ID";
pub const DEFAULT_PASSWORD_COLUMN: &str = "USRACC_PASSWORD";

/// The environment's account lookup: where its test logins and their
/// passwords are kept. `password_column` empty means "detect it" (kept only for a custom table).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct AccountLookup {
    pub table: String,
    pub username_column: String,
    pub password_column: String,
}

impl Default for AccountLookup {
    fn default() -> Self {
        AccountLookup {
            table: DEFAULT_TABLE.to_string(),
            username_column: DEFAULT_USERNAME_COLUMN.to_string(),
            password_column: DEFAULT_PASSWORD_COLUMN.to_string(),
        }
    }
}

/// Said for a table name that is not `schema.table` or `db.schema.table`.
pub const TABLE_RULE: &str =
    "the account table is written as schema.table or database.schema.table, with letters, digits and underscores only";
/// Said for a column name that is not a plain name.
pub const COLUMN_RULE: &str = "a column is one name of letters, digits and underscores";
/// Said when the table or the username column is empty.
pub const NEEDS_TABLE: &str = "the account table and its username column are needed";

/// Said where passwords are looked up on an environment that is not marked a
/// test environment.
pub const NOT_TEST: &str = "passwords are looked up only on an environment marked as a test environment";
/// Said when the lookup cannot read the database because reading is off.
pub const DB_OFF: &str = "reading the database is off - turn the Database Access module on in Settings";
/// Said when the database does not answer.
pub const DID_NOT_ANSWER: &str = "the database did not answer - check the connection (VPN or network?)";
/// Said when the database answers with an error that is not one of the above.
pub const REFUSED: &str = "the database refused the account lookup";
/// Said when its answer cannot be read as rows.
pub const UNREADABLE: &str = "the database's answer to the account lookup could not be read";
/// Said when the answer was cut at the row cap.
pub const CUT_SHORT: &str = "the database's answer was cut short - look up fewer logins at a time";
/// Said where a hash is all a login has.
pub const HASH_ONLY: &str = "the database holds only a hash";
/// Said where a login's row holds no password.
pub const EMPTY_PASSWORD: &str = "the database holds no password for this login";
/// Said where the value read back is the database login's own password: the
/// database layer hides that password in every answer, so it cannot be read.
pub const SAME_AS_DB_LOGIN: &str = "the same as the database login's password, which TCM never reads back";
/// What the database layer writes in place of the connection's own password.
pub const HIDE_MARKER: &str = "(hidden)";
/// Said when the assistant's database tool is switched off.
pub const DB_TOOL_OFF: &str =
    "the database tool (db_query) is switched off for assistants in this environment - turn it on on the AI Bridge tab";
/// The most logins asked about in one statement: under the 200 rows one
/// answer holds, with room for the header, rule and footer.
pub const CHUNK_LOGINS: usize = 150;
/// The longest statement built for one chunk, under the guard's 20,000.
const CHUNK_SQL_CHARS: usize = 15_000;
/// How a failed lookup is fixed, for a person.
pub const FIX_IN_APP: &str = "check the account lookup in Edit environments";
/// How a failed lookup is fixed, for an assistant.
pub const FIX_BY_TOOL: &str = "change the account lookup in Edit environments, or call set_account_lookup";

/// The reason a login with no row in `table` is given.
pub fn no_row_reason(table: &str) -> String {
    format!("no row in {table} for this login")
}

fn bare_name(part: &str) -> Option<String> {
    let inner = match part.strip_prefix('[') {
        Some(rest) => rest.strip_suffix(']')?,
        None => part,
    };
    let mut chars = inner.chars();
    let first = chars.next()?;
    let ok = (first.is_ascii_alphabetic() || first == '_')
        && inner.len() <= 128
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then(|| inner.to_string())
}

/// The parts of a table name, brackets dropped: two (`schema`, `table`) or
/// three (`database`, `schema`, `table`).
pub fn table_parts(table: &str) -> Result<Vec<String>, String> {
    let table = table.trim();
    if table.is_empty() {
        return Err(NEEDS_TABLE.to_string());
    }
    let parts: Option<Vec<String>> = table.split('.').map(bare_name).collect();
    match parts {
        Some(p) if (2..=3).contains(&p.len()) => Ok(p),
        _ => Err(TABLE_RULE.to_string()),
    }
}

/// A column name, brackets dropped.
pub fn column_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(NEEDS_TABLE.to_string());
    }
    bare_name(name).ok_or_else(|| COLUMN_RULE.to_string())
}

impl AccountLookup {
    /// Trimmed, the form it is saved in.
    pub fn tidy(mut self) -> Self {
        self.table = self.table.trim().to_string();
        self.username_column = self.username_column.trim().to_string();
        self.password_column = self.password_column.trim().to_string();
        self
    }

    /// A lookup saved with no password column on the default table reads as
    /// the default column; a custom table with none keeps "detect it".
    pub fn with_default_password_column(mut self) -> Self {
        let on_default_table = |t: &str| {
            let plain: String = t.chars().filter(|c| *c != '[' && *c != ']').collect();
            plain.trim().eq_ignore_ascii_case(DEFAULT_TABLE)
        };
        if self.password_column.trim().is_empty() && on_default_table(&self.table) {
            self.password_column = DEFAULT_PASSWORD_COLUMN.to_string();
        }
        self
    }

    /// Every name is a plain identifier (the password column may be empty).
    pub fn check(&self) -> Result<(), String> {
        table_parts(&self.table)?;
        column_name(&self.username_column)?;
        if !self.password_column.trim().is_empty() {
            column_name(&self.password_column)?;
        }
        Ok(())
    }

    /// The table as the SQL writes it: `[schema].[table]`.
    pub fn quoted_table(&self) -> Result<String, String> {
        Ok(table_parts(&self.table)?.iter().map(|p| format!("[{p}]")).collect::<Vec<_>>().join("."))
    }

    /// The password column as configured, or `None` when it is to be detected.
    pub fn configured_password_column(&self) -> Result<Option<String>, String> {
        let name = self.password_column.trim();
        if name.is_empty() {
            Ok(None)
        } else {
            column_name(name).map(Some)
        }
    }

    /// The table's columns, by name and type, read from the database's own
    /// catalogue. Only names come back, never a value.
    pub fn columns_sql(&self) -> Result<String, String> {
        let parts = table_parts(&self.table)?;
        let (catalogue, schema, table) = match parts.as_slice() {
            [s, t] => (String::new(), s, t),
            [d, s, t] => (format!("[{d}]."), s, t),
            _ => return Err(TABLE_RULE.to_string()),
        };
        let sql = format!(
            "SELECT COLUMN_NAME, DATA_TYPE FROM {catalogue}INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {}",
            quoted_literal(schema),
            quoted_literal(table)
        );
        read_only(sql)
    }

    /// The one SELECT that reads the passwords of `users`:
    /// `SELECT [user], [pwd] FROM [schema].[table] WHERE [user] IN (N'a', N'b')`.
    pub fn select_sql(&self, password_column: &str, users: &[String]) -> Result<String, String> {
        if users.is_empty() {
            return Err("there are no logins to look up".to_string());
        }
        let user = format!("[{}]", column_name(&self.username_column)?);
        let pwd = format!("[{}]", column_name(password_column)?);
        let list = users.iter().map(|u| quoted_literal(u)).collect::<Vec<_>>().join(", ");
        read_only(format!("SELECT {user}, {pwd} FROM {} WHERE {user} IN ({list})", self.quoted_table()?))
    }

    /// The one SELECT that lists up to `limit` logins, optionally only those
    /// holding `search`: `SELECT TOP (N) [user] FROM [schema].[table] WHERE
    /// [user] IS NOT NULL [AND [user] LIKE N'%term%' ESCAPE N'!'] ORDER BY [user]`.
    /// Reads no password. The term is matched literally: LIKE wildcards in it
    /// are escaped.
    pub fn list_sql(&self, search: &str, limit: u32) -> Result<String, String> {
        let user = format!("[{}]", column_name(&self.username_column)?);
        let term = search.trim();
        let filter = if term.is_empty() {
            String::new()
        } else {
            let mut escaped = String::new();
            for c in term.chars() {
                if matches!(c, '!' | '%' | '_' | '[') {
                    escaped.push('!');
                }
                escaped.push(c);
            }
            format!(" AND {user} LIKE {} ESCAPE N'!'", quoted_literal(&format!("%{escaped}%")))
        };
        read_only(format!(
            "SELECT TOP ({limit}) {user} FROM {} WHERE {user} IS NOT NULL{filter} ORDER BY {user}",
            self.quoted_table()?
        ))
    }
}

/// The most logins the picker lists at once.
pub const LIST_LIMIT: u32 = 200;

/// The account key for `login`: lowercase, every character outside
/// `a-z0-9._-` a hyphen, hyphen runs collapsed, leading characters trimmed to
/// a letter or digit, cut to 64; `user-` in front when nothing usable is left
/// or the name is reserved; `-2`, `-3`... until it is not in `taken`.
pub fn key_for_login(login: &str, taken: &std::collections::HashSet<String>) -> String {
    let mut key = String::new();
    for c in login.trim().to_lowercase().chars() {
        let c = if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-') { c } else { '-' };
        if !(c == '-' && key.ends_with('-')) {
            key.push(c);
        }
    }
    let key = key.trim_start_matches(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit()));
    let mut key: String = key.chars().take(64).collect();
    if key.is_empty() {
        // Nothing usable in the login: `user-1`, then `user-2` on a clash.
        let mut n = 1;
        while taken.contains(&format!("user-{n}")) {
            n += 1;
        }
        return format!("user-{n}");
    }
    if !super::accounts::valid_key(&key) {
        key = format!("user-{key}");
        key = key.chars().take(64).collect();
        key = key.trim_end_matches('-').to_string();
    }
    let base = key.clone();
    let mut n = 2;
    while taken.contains(&key) {
        let tail = format!("-{n}");
        let keep: String = base.chars().take(64 - tail.len()).collect();
        key = format!("{keep}{tail}");
        n += 1;
    }
    key
}

/// The statement, if the database guard reads it as a plain read.
fn read_only(sql: String) -> Result<String, String> {
    match guard::classify(&sql) {
        guard::Verdict::Read => Ok(sql),
        _ => Err(REFUSED.to_string()),
    }
}

/// The columns that could hold the password: character-typed, named with
/// `pass` or `pwd`, and not a date, a flag or a prompt about one.
pub fn password_candidates(columns: &[(String, String)]) -> Vec<String> {
    const CHARACTER: &[&str] = &["char", "varchar", "nchar", "nvarchar", "text", "ntext"];
    const NOT: &[&str] = &["date", "time", "day", "flag", "flg", "prompt", "valid", "change", "expir"];
    columns
        .iter()
        .filter(|(name, kind)| {
            let n = name.to_lowercase();
            CHARACTER.contains(&kind.to_lowercase().as_str())
                && (n.contains("pass") || n.contains("pwd"))
                && !NOT.iter().any(|w| n.contains(w))
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// The password column among `columns`: the one candidate, or a sentence
/// naming the candidates (column names are fine to show) and asking for the
/// column to be set.
pub fn detect_password_column(columns: &[(String, String)], table: &str) -> Result<String, String> {
    let mut found = password_candidates(columns);
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!("no password column was found in {table} - set the password column")),
        _ => Err(format!(
            "the password column of {table} could not be told apart (it could be {}) - set the password column",
            found.join(", ")
        )),
    }
}

/// What a lookup found for one login.
#[derive(Clone, PartialEq, Eq)]
pub enum Found {
    Password(String),
    /// The value is a hash or an encrypted blob: not a password.
    Hash,
    /// The row's password is empty.
    Empty,
    /// The value is the database login's own password, which the database
    /// layer replaces with a marker: it cannot be read, and must not be used.
    Masked,
}

/// Hand-written so a password cannot reach a log line through `{:?}`.
impl std::fmt::Debug for Found {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Found::Password(_) => f.write_str("Password(hidden)"),
            Found::Hash => f.write_str("Hash"),
            Found::Empty => f.write_str("Empty"),
            Found::Masked => f.write_str("Masked"),
        }
    }
}

/// What a lookup found, by login (lower-case). A login not here has no row.
#[derive(Debug, Clone)]
pub struct LookedUp {
    pub table: String,
    pub password_column: String,
    found: HashMap<String, Found>,
}

impl LookedUp {
    pub fn of(&self, username: &str) -> Option<&Found> {
        self.found.get(&username.trim().to_lowercase())
    }
}

/// Whether `value` is clearly a hash or an encrypted blob and not something a
/// person typed: a modular-crypt or prefixed hash, hex of a digest's length,
/// base64 of one, or one very long unbroken token.
pub fn looks_like_hash(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() || v.chars().any(char::is_whitespace) {
        return false;
    }
    const PREFIXES: &[&str] = &[
        "$2a$", "$2b$", "$2x$", "$2y$", "$argon2", "$pbkdf2", "$scrypt", "$1$", "$5$", "$6$", "$apr1$", "$P$",
        "$H$", "{SHA}", "{SSHA}", "pbkdf2_", "sha1$", "sha256$", "md5$",
    ];
    if PREFIXES.iter().any(|p| v.starts_with(p)) {
        return true;
    }
    if let Some(hex) = v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        if hex.len() >= 32 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }
    if matches!(v.len(), 32 | 40 | 56 | 64 | 96 | 128) && v.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    let base64 = v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='));
    if base64 && v.len() >= 24 && v.len() % 4 == 0 {
        let padded = v.ends_with('=');
        let mixed = v.chars().any(|c| c.is_ascii_uppercase())
            && v.chars().any(|c| c.is_ascii_lowercase())
            && v.chars().any(|c| c.is_ascii_digit());
        if padded || (matches!(v.len(), 44 | 88) && mixed) {
            return true;
        }
    }
    v.chars().count() >= 64
}

/// A database error as one fixed sentence. The database's own words are
/// dropped: they can quote a value.
fn failure_of(said: &str, table: &str) -> String {
    if said.contains(query::DB_UNREACHABLE) || crate::api_templates::gate::unreachable(said) {
        return DID_NOT_ANSWER.to_string();
    }
    let lower = said.to_lowercase();
    if lower.contains("invalid object name") {
        format!("the table {table} was not found in the database")
    } else if lower.contains("invalid column name") {
        format!("a column of the account lookup was not found in {table}")
    } else {
        REFUSED.to_string()
    }
}

/// The table's columns as `(name, type)`, or the fixed sentence for why not.
pub async fn read_columns<S: RowSource>(rows: &S, lookup: &AccountLookup) -> Result<Vec<(String, String)>, String> {
    let sql = lookup.columns_sql()?;
    let text = rows.select(&sql).await.map_err(|said| failure_of(&said, &lookup.table))?;
    let first = text.lines().next().map(str::trim).unwrap_or("");
    if first.starts_with("rows:") && first.ends_with("(capped)") {
        return Err(format!("{} has too many columns to list - set the password column", lookup.table));
    }
    let (header, body) = parse_table(&text).map_err(|_| UNREADABLE.to_string())?;
    let (Some(name), Some(kind)) = (column(&header, "column_name"), column(&header, "data_type")) else {
        return Err(UNREADABLE.to_string());
    };
    let out: Vec<(String, String)> = body
        .iter()
        .filter_map(|r| Some((cell(r, name)?, cell(r, kind).unwrap_or_default())))
        .collect();
    if out.is_empty() {
        return Err(format!("the table {} was not found in the database", lookup.table));
    }
    Ok(out)
}

/// What a dry check of the lookup found: the password column to read, or why
/// there is none. Reads the table's column list only, never a value.
pub async fn resolve_password_column<S: RowSource>(rows: &S, lookup: &AccountLookup) -> Result<String, String> {
    lookup.check()?;
    let columns = read_columns(rows, lookup).await?;
    let has = |name: &str| columns.iter().any(|(c, _)| c.eq_ignore_ascii_case(name));
    let user = column_name(&lookup.username_column)?;
    if !has(&user) {
        return Err(format!("the username column {user} was not found in {}", lookup.table));
    }
    match lookup.configured_password_column()? {
        Some(p) if has(&p) => Ok(p),
        Some(p) => Err(format!("the password column {p} was not found in {}", lookup.table)),
        None => detect_password_column(&columns, &lookup.table),
    }
}

/// Looks up the passwords of `users` in the environment's account table.
/// Detects the password column first when none is set. The `Err` is a fixed
/// sentence about why it could not be done - never a value.
pub async fn look_up<S: RowSource>(rows: &S, lookup: &AccountLookup, users: &[String]) -> Result<LookedUp, String> {
    lookup.check()?;
    let mut asked: Vec<String> = vec![];
    for u in users.iter().map(|u| u.trim()).filter(|u| !u.is_empty()) {
        if !asked.iter().any(|a| a.to_lowercase() == u.to_lowercase()) {
            asked.push(u.to_string());
        }
    }
    let pwd = match lookup.configured_password_column()? {
        Some(p) => p,
        None => {
            let columns = read_columns(rows, lookup).await?;
            detect_password_column(&columns, &lookup.table)?
        }
    };
    let user_name = column_name(&lookup.username_column)?.to_lowercase();
    let mut found: HashMap<String, Found> = HashMap::new();
    // In chunks, so more logins than one answer holds (200 rows) still work
    // and no statement outgrows the guard's length limit.
    let mut at = 0;
    while at < asked.len() {
        let mut end = at;
        let mut chars = 0;
        while end < asked.len() && end - at < CHUNK_LOGINS && (end == at || chars + asked[end].len() + 8 < CHUNK_SQL_CHARS) {
            chars += asked[end].len() + 8;
            end += 1;
        }
        let sql = lookup.select_sql(&pwd, &asked[at..end])?;
        at = end;
        let text = rows.select(&sql).await.map_err(|said| failure_of(&said, &lookup.table))?;
        let first = text.lines().next().map(str::trim).unwrap_or("");
        if first.starts_with("rows:") && first.ends_with("(capped)") {
            return Err(CUT_SHORT.to_string());
        }
        let (header, body) = parse_table(&text).map_err(|_| UNREADABLE.to_string())?;
        let user_at = column(&header, &user_name).ok_or_else(|| UNREADABLE.to_string())?;
        let pwd_at = column(&header, &pwd.to_lowercase()).ok_or_else(|| UNREADABLE.to_string())?;
        for row in &body {
            let Some(user) = cell(row, user_at) else { continue };
            let got = match cell(row, pwd_at) {
                None => Found::Empty,
                // The database layer hides the connection's own password in
                // every answer; a login that shares it reads as the marker.
                Some(v) if v.contains(HIDE_MARKER) => Found::Masked,
                Some(v) if looks_like_hash(&v) => Found::Hash,
                Some(v) => Found::Password(v),
            };
            let slot = found.entry(user.to_lowercase()).or_insert_with(|| got.clone());
            // Two rows for one login (names differing only in case): a usable
            // password wins over a hash or an empty one.
            if matches!(got, Found::Password(_)) && !matches!(slot, Found::Password(_)) {
                *slot = got;
            }
        }
    }
    Ok(LookedUp { table: lookup.table.trim().to_string(), password_column: pwd, found })
}

/// The logins in the environment's account table that hold `search` (all of
/// them when it is empty), up to `LIST_LIMIT`, and whether the list was cut at
/// that limit. Reads no password. The `Err` is a fixed sentence.
pub async fn list_logins<S: RowSource>(
    rows: &S,
    lookup: &AccountLookup,
    search: &str,
) -> Result<(Vec<String>, bool), String> {
    lookup.check()?;
    let sql = lookup.list_sql(search, LIST_LIMIT)?;
    let user_name = column_name(&lookup.username_column)?.to_lowercase();
    let text = rows.select(&sql).await.map_err(|said| failure_of(&said, &lookup.table))?;
    let first = text.lines().next().map(str::trim).unwrap_or("");
    let capped = first.starts_with("rows:") && first.ends_with("(capped)");
    let (header, body) = parse_table(&text).map_err(|_| UNREADABLE.to_string())?;
    let at = column(&header, &user_name).ok_or_else(|| UNREADABLE.to_string())?;
    let mut logins: Vec<String> = vec![];
    for row in &body {
        if let Some(login) = cell(row, at) {
            if !logins.iter().any(|l| l == &login) {
                logins.push(login);
            }
        }
    }
    let cut = capped || body.len() >= LIST_LIMIT as usize;
    Ok((logins, cut))
}

/// A source of rows that answers nothing: stands in where the database could
/// not be reached, so `Err(reason)` is carried by the caller instead.
pub struct NoRows;

impl RowSource for NoRows {
    async fn select(&self, _sql: &str) -> Result<String, String> {
        Err("no database".to_string())
    }
}

/// How the lookup went for each login, for the answer a person or an
/// assistant reads: counts and login keys only, never a password.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Tally {
    pub asked: usize,
    pub found: usize,
    pub no_row: Vec<String>,
    pub hash_only: Vec<String>,
    pub empty: Vec<String>,
    pub masked: Vec<String>,
}

impl Tally {
    /// Counts one login by what the lookup says of it.
    pub fn add(&mut self, key: &str, found: Option<&Found>) {
        self.asked += 1;
        match found {
            Some(Found::Password(_)) => self.found += 1,
            Some(Found::Hash) => self.hash_only.push(key.to_string()),
            Some(Found::Empty) => self.empty.push(key.to_string()),
            Some(Found::Masked) => self.masked.push(key.to_string()),
            None => self.no_row.push(key.to_string()),
        }
    }

    /// "2 of 3 passwords found in T; no row for: a; hash only: b".
    pub fn sentence(&self, table: &str) -> String {
        let mut s = format!("{} of {} passwords found in {table}", self.found, self.asked);
        if !self.no_row.is_empty() {
            s.push_str(&format!("; no row for: {}", self.no_row.join(", ")));
        }
        if !self.hash_only.is_empty() {
            s.push_str(&format!("; hash only: {}", self.hash_only.join(", ")));
        }
        if !self.empty.is_empty() {
            s.push_str(&format!("; no password stored for: {}", self.empty.join(", ")));
        }
        if !self.masked.is_empty() {
            s.push_str(&format!("; same as the database login's password (never read back): {}", self.masked.join(", ")));
        }
        s
    }
}

/// What one proposed login ends up with once the lookup has been asked.
/// `found` is what the lookup said (`None`: no row), `agent_password` and
/// `agent_reason` what the assistant gave (for example from a seed script).
/// The table wins over an agent-supplied password; with no usable value in
/// the table the agent's own stands.
pub fn settle_proposal(
    found: Option<&Found>,
    agent_password: Option<String>,
    agent_reason: Option<String>,
    table: &str,
) -> (Option<String>, Option<String>) {
    match found {
        Some(Found::Password(p)) => (Some(p.clone()), None),
        Some(Found::Hash) => (None, Some(HASH_ONLY.to_string())),
        Some(Found::Empty) => match (agent_password, agent_reason) {
            (Some(p), _) => (Some(p), None),
            (None, r) => (None, Some(r.unwrap_or_else(|| EMPTY_PASSWORD.to_string()))),
        },
        // The table holds the database login's own password: whatever the
        // assistant sent, it can only have read the same masked value.
        Some(Found::Masked) => (None, Some(SAME_AS_DB_LOGIN.to_string())),
        None => match (agent_password, agent_reason) {
            (Some(p), _) => (Some(p), None),
            (None, Some(r)) => (None, Some(r)),
            (None, None) => (None, Some(no_row_reason(table))),
        },
    }
}

/// The failure sentence an answer gives, with how to fix it.
pub fn failure_sentence(why: &str, fix: &str) -> String {
    format!("the account lookup did not run: {why}. To fix it, {fix}.")
}

/// Which database errors a person must read as "turn something on", for the
/// lookup rows source: keeps the module-off sentence in the lookup's words.
pub fn db_refusal(status_and_why: (u16, String)) -> String {
    let (_, why) = status_and_why;
    if why == crate::ai_bridge::FLOW_NEEDS_READING {
        DB_OFF.to_string()
    } else if why == query::NO_CONNECTION {
        "no database connection is chosen - pick one on the AI Bridge tab".to_string()
    } else {
        why
    }
}
