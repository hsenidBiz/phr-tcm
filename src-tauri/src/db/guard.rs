//! What a statement is allowed to do, decided before anything runs it.
//!
//! The rule the whole feature rests on: a statement is classified first and
//! run second. Reads go everywhere; the four write verbs go only to the dev
//! login; everything that changes a schema, hands out permissions, calls a
//! procedure, or smuggles a second statement in is refused outright, on
//! every connection, with a sentence that names what was found.

/// The longest statement the tools will look at. Anything bigger is far
/// more likely to be a paste accident than a question about the database,
/// and the guard has to read every character of it.
pub const MAX_SQL_CHARS: usize = 20_000;

/// The one sentence a write on a read-only connection comes back with. It
/// names the connection to switch to and where to switch it, because that
/// is the only thing the person can act on.
pub const READ_ONLY_SENTENCE: &str = "this connection is read only: INSERT/UPDATE/DELETE need the Dev - dev login connection, chosen in the AI Bridge tab";

/// What a statement is allowed to do on this connection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Access {
    ReadOnly,
    DevWrites,
}

/// What one statement turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Read,
    Write,
    Refused(String),
}

/// Words that end the conversation wherever they appear - not just as the
/// leading word. A SELECT with `DROP TABLE` welded onto the end is still a
/// DROP, and none of these has any business in a question about data.
///
/// The three `OPEN...` functions are here because their payload travels in
/// a string literal, and `strip_comments_and_literals` blanks literals
/// before anything is classified - so `OPENROWSET(..., 'EXEC xp_cmdshell
/// ...')` would otherwise read as a plain SELECT. The function has to be
/// refused; its contents cannot be inspected.
const REFUSED_WORDS: &[&str] = &[
    "DROP",
    "TRUNCATE",
    "ALTER",
    "CREATE",
    "EXECUTE",
    "EXEC",
    "GRANT",
    "REVOKE",
    "DENY",
    "BACKUP",
    "RESTORE",
    "OPENROWSET",
    "OPENQUERY",
    "OPENDATASOURCE",
];

/// The verbs that change data. Searched as whole words anywhere in the
/// statement, because a CTE can put one behind a leading `WITH`.
const WRITE_WORDS: &[&str] = &["INSERT", "UPDATE", "DELETE", "MERGE"];

/// The dev-login preset is the only connection that may write: recognised
/// by its User Id, compared case-insensitively, never by the password.
pub fn access_for(connection_string: &str) -> Access {
    match user_id_of(connection_string) {
        Some(user) => access_for_user(&user),
        None => Access::ReadOnly,
    }
}

/// The same decision from a user id on its own - what `sqlcmd::run_sql`
/// asks, so a `Connection` is enough to know what it may do.
pub fn access_for_user(user_id: &str) -> Access {
    if user_id.trim().to_ascii_lowercase().ends_with("_devlogin") {
        Access::DevWrites
    } else {
        Access::ReadOnly
    }
}

/// The connection-string keys that name the signing-in user.
///
/// This list is shared with `sqlcmd::parse_connection` on purpose. When the
/// two disagreed, a hand-typed `User=x_devlogin` was `ReadOnly` to the
/// guard and the dev login to sqlcmd - the door that decides and the door
/// that enforces have to be reading the same field.
pub(crate) const USER_ID_KEYS: &[&str] = &["userid", "uid", "user"];

/// A connection-string key, folded the way a SQL Server driver folds one:
/// spaces removed, lower-cased. `User Id`, `UserId` and `USER ID` are one key.
pub(crate) fn normalised_key(raw: &str) -> String {
    raw.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase()
}

/// The user id out of a connection string.
fn user_id_of(connection_string: &str) -> Option<String> {
    for part in connection_string.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        if USER_ID_KEYS.contains(&normalised_key(key).as_str()) {
            return Some(value.trim().to_string());
        }
    }
    None
}

/// Classifies ONE statement. Comments and the contents of string literals
/// are stripped before the first keyword is read, so a word that only
/// appears inside them counts as data and not as a verb.
pub fn classify(sql: &str) -> Verdict {
    if sql.chars().count() > MAX_SQL_CHARS {
        return refused(format!(
            "the statement is longer than {MAX_SQL_CHARS} characters: send a smaller one"
        ));
    }
    // GO is read off the ORIGINAL text: it is a batch separator sqlcmd acts
    // on itself, so it would split one "statement" into several. Only the
    // first word of the line is compared, because sqlcmd takes a repeat
    // count (`GO 5`) and tolerates a trailing comment after it.
    if sql
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .any(|word| word.eq_ignore_ascii_case("GO"))
    {
        return refused(
            "a GO batch separator is not allowed here: send one statement on its own".to_string(),
        );
    }

    let stripped = strip_comments_and_literals(sql);
    // A leading semicolon is how `;WITH` is written, and it is pasted along
    // with the CTE often enough to be worth not calling a second statement.
    let body = stripped.trim().trim_start_matches(|c: char| c == ';' || c.is_whitespace());
    if body.is_empty() {
        return refused("the statement is empty".to_string());
    }
    if let Some(at) = body.find(';') {
        if !body[at + 1..].trim().is_empty() {
            return refused(
                "a second statement is not allowed here: send one statement at a time".to_string(),
            );
        }
    }

    let upper = body.to_uppercase();
    for word in REFUSED_WORDS {
        if has_word(&upper, word) {
            return refused(format!(
                "{word} is not allowed here: these tools run SELECT statements, and INSERT/UPDATE/DELETE only on the dev login"
            ));
        }
    }
    if let Some(prefix) = procedure_call(&upper) {
        return refused(format!(
            "a stored procedure call ({prefix}) is not allowed here: these tools run SELECT statements only"
        ));
    }

    let head = leading_word(&upper);
    match head.as_str() {
        "SELECT" | "WITH" => {
            // A CTE may lead a write: `WITH c AS (...) INSERT INTO t ...`.
            // Anything that mentions a write verb at all is treated as one -
            // erring towards Write only ever asks for a better connection.
            if WRITE_WORDS.iter().any(|w| has_word(&upper, w)) {
                return Verdict::Write;
            }
            // `SELECT ... INTO newtable` is the one way a SELECT creates a
            // table, and it does not go near any of the DDL words above.
            // The write check ran first, so `INSERT INTO` is already gone.
            if has_word(&upper, "INTO") {
                return refused(
                    "SELECT INTO creates a table and cannot run here: these tools read data, they do not add to the schema".to_string(),
                );
            }
            Verdict::Read
        }
        "INSERT" | "UPDATE" | "DELETE" | "MERGE" => Verdict::Write,
        "" => refused(
            "this does not start with a SQL verb: send one SELECT statement".to_string(),
        ),
        other => refused(format!(
            "{other} cannot run here: these tools run SELECT statements, and INSERT/UPDATE/DELETE only on the dev login"
        )),
    }
}

/// The gate itself. `Read` always passes; `Write` passes only on the dev
/// login; a refusal is a refusal on every connection.
pub fn allowed(sql: &str, access: Access) -> Result<Verdict, String> {
    match classify(sql) {
        Verdict::Read => Ok(Verdict::Read),
        Verdict::Write => match access {
            Access::DevWrites => Ok(Verdict::Write),
            Access::ReadOnly => Err(READ_ONLY_SENTENCE.to_string()),
        },
        Verdict::Refused(why) => Err(why),
    }
}

fn refused(why: String) -> Verdict {
    Verdict::Refused(why)
}

/// Replaces comments and the insides of quoted things with a space, so the
/// keyword scan below reads only what the server would execute. The quotes
/// of a string literal are kept, so `SELECT 'x'` still looks like SQL.
fn strip_comments_and_literals(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '-' if chars.peek() == Some(&'-') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
                out.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                // T-SQL block comments nest, so the depth has to be tracked.
                let mut depth = 1usize;
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if prev == '/' && c == '*' {
                        depth += 1;
                        prev = '\0';
                        continue;
                    }
                    if prev == '*' && c == '/' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        prev = '\0';
                        continue;
                    }
                    if c == '\n' {
                        out.push('\n');
                    }
                    prev = c;
                }
                out.push(' ');
            }
            '\'' => {
                out.push('\'');
                while let Some(c) = chars.next() {
                    if c == '\'' {
                        // '' inside a literal is an escaped quote.
                        if chars.peek() == Some(&'\'') {
                            chars.next();
                            continue;
                        }
                        break;
                    }
                }
                out.push('\'');
            }
            // A bracketed or double-quoted identifier is a name, never a
            // verb, and may legally contain a quote or a comment marker.
            '[' => {
                out.push_str("[]");
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
            }
            '"' => {
                out.push_str("\"\"");
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// True when `word` appears in `upper` as a whole word. Identifier
/// characters (letters, digits, `_`, `@`, `#`) on either side mean it is
/// part of a longer name - `resp_code` is not `sp_`, `deleted` is not
/// `DELETE`.
fn has_word(upper: &str, word: &str) -> bool {
    let mut from = 0usize;
    while let Some(at) = upper[from..].find(word) {
        let start = from + at;
        let end = start + word.len();
        let before = upper[..start].chars().next_back();
        let after = upper[end..].chars().next();
        if !before.is_some_and(is_name_char) && !after.is_some_and(is_name_char) {
            return true;
        }
        from = end;
        if from >= upper.len() {
            break;
        }
    }
    false
}

/// `sp_` or `xp_` starting a name anywhere in the statement: the shape of
/// every system-procedure call, including the ones that shell out.
fn procedure_call(upper: &str) -> Option<&'static str> {
    for (prefix, shown) in [("SP_", "sp_"), ("XP_", "xp_")] {
        let mut from = 0usize;
        while let Some(at) = upper[from..].find(prefix) {
            let start = from + at;
            if !upper[..start].chars().next_back().is_some_and(is_name_char) {
                return Some(shown);
            }
            from = start + prefix.len();
            if from >= upper.len() {
                break;
            }
        }
    }
    None
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '@' || c == '#'
}

/// The statement's verb: the first name, past any leading whitespace, the
/// brackets of `(SELECT ...)`, and a byte order mark - which is what a file
/// saved by Notepad puts in front of the first word, and which `trim` does
/// not count as whitespace.
fn leading_word(upper: &str) -> String {
    upper
        .trim_start_matches(|c: char| c.is_whitespace() || c == '(' || c == '\u{feff}')
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}
