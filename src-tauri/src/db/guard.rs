//! What a statement is allowed to do, decided before anything runs it.
//!
//! The rule the whole feature rests on: a statement is classified first and
//! run second. Reads go everywhere; the four write verbs go only to the dev
//! login; everything that changes a schema or hands out permissions is
//! refused outright, on every connection, with a sentence that names what
//! was found.
//!
//! EXEC/EXECUTE is the one write-shaped exception with its own rule: a call
//! to the fixed set of look-up system procedures (`sp_help` and its
//! neighbours - see `LOOKUP_PROCEDURES`) is a Read, on every connection,
//! because it only ever reads schema information. A call to any OTHER named
//! procedure can change data, so it is a Write, gated exactly like
//! INSERT/UPDATE/DELETE - dev login only, writes switch on. Dynamic SQL
//! (`EXEC('...')`, `EXEC(@sql)`, `EXEC @variable`, `sp_executesql`), any
//! other `sp_`/`xp_` procedure, and a database- or linked-server-qualified
//! name are refused outright, on every connection - see `classify_exec`.
//! A second statement smuggled onto the end, with or without a semicolon,
//! is refused wherever it appears, EXEC included.
//!
//! None of the above is SQL at all: sqlcmd itself reads a handful of
//! client commands - `!!`, `:r`, `:out`, `:connect`, `:setvar` - off the
//! start of a line, and `$(name)` anywhere, before any of it ever reaches
//! the server. `classify` refuses all of that on the ORIGINAL text, the
//! same way it reads `GO`, and `sqlcmd::sqlcmd_args` also runs sqlcmd with
//! `-X1`/`-x` so the two layers do not depend on each other.

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
    // EXEC/EXECUTE are not in this list any more - a leading EXEC/EXECUTE
    // is classified by `classify_exec` instead, which is the one place
    // narrow enough to tell a named procedure call from dynamic SQL. One
    // found anywhere else in the statement is still refused - see the
    // explicit check for that right after this loop.
    "GRANT",
    "REVOKE",
    "DENY",
    "BACKUP",
    "RESTORE",
    "OPENROWSET",
    "OPENQUERY",
    "OPENDATASOURCE",
    // T-SQL needs no semicolon between statements: `SELECT 1 KILL 1` is
    // two statements, and the semicolon check below never sees it. Each of
    // these can only START a statement - none belongs inside a SELECT or a
    // single INSERT/UPDATE/DELETE/MERGE - so finding one anywhere means a
    // second statement was welded on. Confirmed with SET PARSEONLY ON on
    // sgdev01db02 (2026-09-23); in 1.25.17 they all passed as reads.
    "KILL",
    "SHUTDOWN",
    "DBCC",
    "CHECKPOINT",
    "RECONFIGURE",
    "WAITFOR",
    "DECLARE",
    "DISABLE",
    "ENABLE",
    "BULK",
    "STATISTICS",
    "SETUSER",
    "REVERT",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "WRITETEXT",
    "UPDATETEXT",
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
    if sqlcmd_lines(sql)
        .filter_map(|line| line.split_whitespace().next())
        .any(|word| word.eq_ignore_ascii_case("GO"))
    {
        return refused(
            "a GO batch separator is not allowed here: send one statement on its own".to_string(),
        );
    }
    // Same reason as GO: sqlcmd reads a client command - `:r`, `:out`,
    // `:connect`, `:setvar`, `!!` - at the START OF A LINE, on the ORIGINAL
    // text, before a single one of these tools' own statements is ever
    // parsed. `-X1`/`-x` (see `sqlcmd::sqlcmd_args`) turn off `!!` and
    // `$(var)` substitution at the process level, but Microsoft's own docs
    // say `-X` does not reach `:r`/`:out`/`:connect` at all - so this is
    // the only door for those three, and the only one that cannot be
    // silently lost if a flag is ever dropped from `sqlcmd_args`. A
    // legitimate colon - a time literal, `a::b` - is never the first thing
    // on its line, so neither is refused.
    if sqlcmd_lines(sql).any(|line| {
        let after_ws = line.trim_start();
        after_ws.starts_with(':') || after_ws.starts_with("!!")
    }) {
        return refused(
            "sqlcmd commands (a line starting with \":\" or \"!!\") are not allowed here: send one SQL statement"
                .to_string(),
        );
    }
    // `$(name)` is sqlcmd's own scripting-variable substitution, active
    // wherever it appears in the line - not just at the start - and `-x`
    // is the belt to this braces: read on the ORIGINAL text for the same
    // reason as the check above.
    if sql.contains("$(") {
        return refused(
            "\"$(\" is not allowed here: sqlcmd would read it as a variable".to_string(),
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
    let head = leading_word(&upper);

    // EXEC/EXECUTE gets its own classifier, on a copy of `sql` that keeps a
    // bracketed or double-quoted name's real text - `body` above blanks it,
    // which is right for a keyword scan but useless for reading a procedure
    // NAME, the one thing this shape actually has to read.
    if head == "EXEC" || head == "EXECUTE" {
        let kept = strip_comments_keep_identifiers(sql);
        let kept_body = kept.trim().trim_start_matches(|c: char| c == ';' || c.is_whitespace());
        return classify_exec(kept_body, &head);
    }

    for word in REFUSED_WORDS {
        if has_word(&upper, word) {
            return refused(format!(
                "{word} is not allowed here: these tools run SELECT statements, and INSERT/UPDATE/DELETE only on the dev login"
            ));
        }
    }
    // EXEC/EXECUTE only leads a statement, never anything else - one found
    // here is how a second statement gets welded onto the end of a SELECT
    // or a write without a semicolon in sight.
    if has_word(&upper, "EXEC") || has_word(&upper, "EXECUTE") {
        return refused(
            "EXEC/EXECUTE is only allowed as the first word of a statement: send one statement at a time"
                .to_string(),
        );
    }
    // USE switches database, which is a second statement - except inside a
    // query hint, where `OPTION (USE HINT (...))` and `OPTION (USE PLAN
    // N'...')` are part of the SELECT itself.
    if words_after(&upper, "USE").iter().any(|next| next != "HINT" && next != "PLAN") {
        return refused(
            "USE is not allowed here: it starts a second statement - send one statement at a time"
                .to_string(),
        );
    }
    if let Some(prefix) = procedure_call(&upper) {
        return refused(format!(
            "a stored procedure call ({prefix}) is not allowed here: these tools run SELECT statements only"
        ));
    }

    match head.as_str() {
        "SELECT" | "WITH" => {
            // A CTE may lead a write: `WITH c AS (...) INSERT INTO t ...`.
            // Anything that mentions a write verb at all is treated as one -
            // erring towards Write only ever asks for a better connection.
            if WRITE_WORDS.iter().any(|w| has_word(&upper, w)) {
                return Verdict::Write;
            }
            // A SELECT has no SET in it. One that does has a second
            // statement welded on without a semicolon (`SELECT 1 SET
            // NOCOUNT ON`); the SET of an UPDATE never reaches this line.
            if has_word(&upper, "SET") {
                return refused(
                    "a second statement (SET) is not allowed here: send one statement at a time"
                        .to_string(),
                );
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

/// The original text cut at every character any sqlcmd build could take
/// for the end of a line. go-sqlcmd, the one this app runs, ends a line
/// only at `\n`; cutting at a lone `\r`, a vertical tab, a form feed and
/// the Unicode line breaks as well means a build that splits more eagerly
/// still cannot find a line start the guard never looked at. Erring this
/// way only ever refuses more.
fn sqlcmd_lines(sql: &str) -> impl Iterator<Item = &str> {
    sql.split(['\n', '\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}'])
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

/// Like `strip_comments_and_literals`, but a bracketed or double-quoted
/// name keeps its real text instead of being blanked to `[]`/`""`. Classifying
/// an EXEC statement means reading the actual procedure name - `sp_help` and
/// `DeleteEverything` have to look different - so the one place that costs
/// anything is here, not in the keyword scan the rest of the guard runs on
/// `body`, which never needs to know what a name actually says.
///
/// A string literal is still blanked: its content is a VALUE, not a name,
/// and `EXEC dbo.p 'DROP TABLE t'` must read the same as `EXEC dbo.p 1`.
fn strip_comments_keep_identifiers(sql: &str) -> String {
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
                        if chars.peek() == Some(&'\'') {
                            chars.next();
                            continue;
                        }
                        break;
                    }
                }
                out.push('\'');
            }
            // Unlike `strip_comments_and_literals`, brackets and double
            // quotes are not special here - their contents fall through to
            // `other` below and are kept exactly as written.
            other => out.push(other),
        }
    }
    out
}

/// The system procedures a person only ever reads schema information with -
/// never a row of data - so the owner approved them on every connection,
/// read-only included. Compared case-insensitively; `classify_exec` also
/// accepts them written as `sys.<name>` or `dbo.<name>`.
const LOOKUP_PROCEDURES: &[&str] = &[
    "sp_help",
    "sp_helptext",
    "sp_helpindex",
    "sp_columns",
    "sp_tables",
    "sp_stored_procedures",
    "sp_pkeys",
    "sp_fkeys",
];

/// One name part of a procedure name: `name` or `[name]`/`"name"`, with
/// whatever comes after it in the statement.
fn parse_name_part(s: &str) -> Option<(&str, &str)> {
    if let Some(inner) = s.strip_prefix('[') {
        let end = inner.find(']')?;
        return Some((&inner[..end], &inner[end + 1..]));
    }
    if let Some(inner) = s.strip_prefix('"') {
        let end = inner.find('"')?;
        return Some((&inner[..end], &inner[end + 1..]));
    }
    let mut end = 0usize;
    for (i, c) in s.char_indices() {
        let ok = if i == 0 { c.is_alphabetic() || c == '_' } else { c.is_alphanumeric() || c == '_' };
        if !ok {
            break;
        }
        end = i + c.len_utf8();
    }
    if end == 0 { None } else { Some((&s[..end], &s[end..])) }
}

/// What `parse_proc_name` found: one part (`name`), two (`schema.name`), or
/// more than the accepted shape allows.
enum ProcName<'a> {
    One(&'a str),
    Two(&'a str, &'a str),
    TooManyParts,
}

/// The accepted shape allows `name` or `schema.name` only - a THIRD part
/// means the name is database- or linked-server-qualified, which the owner
/// never approved: it reaches somewhere this app's own guard cannot see.
fn parse_proc_name(s: &str) -> Option<(ProcName<'_>, &str)> {
    let (part1, rest) = parse_name_part(s)?;
    let rest_trim = rest.trim_start();
    let Some(after_dot) = rest_trim.strip_prefix('.') else {
        return Some((ProcName::One(part1), rest));
    };
    let after_dot = after_dot.trim_start();
    let (part2, rest2) = parse_name_part(after_dot)?;
    if rest2.trim_start().starts_with('.') {
        return Some((ProcName::TooManyParts, rest2));
    }
    Some((ProcName::Two(part1, part2), rest2))
}

/// `s` with `word` stripped off the front, case-insensitively, as long as
/// what follows is not itself part of a longer name (`NULLABLE` is not
/// `NULL`). `None` for a length or a UTF-8 boundary that makes the
/// comparison impossible - never a panic, whatever text a person pastes.
fn strip_word_ci<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    if s.len() < word.len() || !s.is_char_boundary(word.len()) {
        return None;
    }
    let (head, tail) = s.split_at(word.len());
    if !head.eq_ignore_ascii_case(word) {
        return None;
    }
    if tail.chars().next().is_some_and(is_name_char) {
        return None;
    }
    Some(tail)
}

/// One value out of the accepted grammar: a number, a string literal
/// (already blanked to `''`/`N''` by `strip_comments_keep_identifiers`), an
/// `@variable`, or `NULL`/`DEFAULT`. Returns what is left after it.
fn parse_exec_value(s: &str) -> Option<&str> {
    let r = s.trim_start();
    if r.is_empty() {
        return None;
    }
    if let Some(rest) = strip_word_ci(r, "NULL") {
        return Some(rest);
    }
    if let Some(rest) = strip_word_ci(r, "DEFAULT") {
        return Some(rest);
    }
    if let Some(rest) = r.strip_prefix('@') {
        let len: usize = rest.chars().take_while(|c| is_name_char(*c)).map(|c| c.len_utf8()).sum();
        return if len == 0 { None } else { Some(&rest[len..]) };
    }
    // A blanked string literal is exactly two adjacent quote characters,
    // optionally led by the `N` that marks a Unicode one.
    if let Some(rest) = r.strip_prefix("N''").or_else(|| r.strip_prefix("n''")) {
        return Some(rest);
    }
    if let Some(rest) = r.strip_prefix("''") {
        return Some(rest);
    }
    // A number, optionally signed, optionally with a decimal part.
    let bytes = r.as_bytes();
    let mut i = 0;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        let mut j = i + 1;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > i + 1 {
            i = j;
        }
    }
    Some(&r[i..])
}

/// One argument: `[@name =] value [OUTPUT|OUT]`. An `@name` not followed by
/// `=` is not a named parameter - it IS the value, an `@variable` passed
/// positionally - so it is left for `parse_exec_value` to read instead.
fn consume_exec_arg(s: &str) -> Option<&str> {
    let r = s.trim_start();
    let r = if let Some(after_at) = r.strip_prefix('@') {
        let len: usize =
            after_at.chars().take_while(|c| is_name_char(*c)).map(|c| c.len_utf8()).sum();
        let after_name = after_at[len..].trim_start();
        match after_name.strip_prefix('=') {
            Some(after_eq) => after_eq.trim_start(),
            None => r,
        }
    } else {
        r
    };
    let r = parse_exec_value(r)?;
    let after_ws = r.trim_start();
    if let Some(rest) = strip_word_ci(after_ws, "OUTPUT") {
        return Some(rest);
    }
    if let Some(rest) = strip_word_ci(after_ws, "OUT") {
        return Some(rest);
    }
    Some(r)
}

/// Keeps an over-long leftover from making the refusal sentence itself
/// unreadable.
fn shorten(s: &str) -> String {
    const MAX: usize = 60;
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(MAX).collect::<String>())
    }
}

/// Consumes as many comma-separated arguments as the accepted grammar
/// allows, then accepts `verdict` only if nothing besides one optional
/// trailing `;` is left. Anything else - a second statement, `DROP TABLE
/// t`, a bare `GO` with no line of its own - is refused by the same test,
/// because none of them can be an argument's value.
fn finish_exec_args(rest: &str, verdict: Verdict, head: &str) -> Verdict {
    let mut r = rest;
    loop {
        let t = r.trim_start();
        if t.is_empty() {
            r = t;
            break;
        }
        match consume_exec_arg(t) {
            Some(next) => {
                let after_ws = next.trim_start();
                match after_ws.strip_prefix(',') {
                    Some(after_comma) => r = after_comma,
                    None => {
                        r = next;
                        break;
                    }
                }
            }
            None => {
                r = t;
                break;
            }
        }
    }
    let leftover = r.trim();
    if leftover.is_empty() || leftover == ";" {
        return verdict;
    }
    refused(format!(
        "{head} accepts a procedure name and a comma-separated argument list only: \"{}\" is not part of that shape - send one statement at a time",
        shorten(leftover)
    ))
}

/// Classifies a statement whose leading word is EXEC or EXECUTE against the
/// one shape the owner approved: `EXEC[UTE] [@rc =] <procname> [args]`.
/// `kept` is `sql` run through `strip_comments_keep_identifiers`, trimmed
/// the same way `body` is in `classify` - comments gone, a string literal
/// blanked to its quotes, a bracketed or quoted name kept exactly as
/// written.
fn classify_exec(kept: &str, head: &str) -> Verdict {
    let after = kept[head.len()..].trim_start();
    if after.is_empty() {
        return refused(format!(
            "{head} needs a stored procedure name: send EXEC <procedure> [args]"
        ));
    }
    // Dynamic SQL, built as a string and run: `EXEC('...')`/`EXEC (@sql)`.
    // There is no procedure name here at all - only a batch of SQL nobody
    // can classify without running it.
    if after.starts_with('(') {
        return refused(format!(
            "{head}(...) runs dynamic SQL and cannot be classified here: these tools call one named stored procedure, never a string of SQL built at runtime"
        ));
    }

    // `@rc = procname` reads a return code into a variable - the `=` is
    // what tells it apart from `EXEC @sql`, dynamic SQL run out of a
    // variable that holds a batch of text instead of a procedure's name.
    let after = if let Some(rest) = after.strip_prefix('@') {
        let len: usize = rest.chars().take_while(|c| is_name_char(*c)).map(|c| c.len_utf8()).sum();
        let past_name = rest[len..].trim_start();
        match past_name.strip_prefix('=') {
            Some(past_eq) => past_eq.trim_start(),
            None => {
                return refused(format!(
                    "{head} of a variable runs dynamic SQL and cannot be classified here: these tools call one named stored procedure, never a string of SQL built at runtime"
                ));
            }
        }
    } else {
        after
    };

    let Some((name_parts, rest)) = parse_proc_name(after) else {
        return refused(format!(
            "{head} does not name one stored procedure: send EXEC <procedure> [args]"
        ));
    };
    let (schema, name) = match name_parts {
        ProcName::TooManyParts => {
            return refused(format!(
                "{head} of a database- or linked-server-qualified name is not allowed here: only <procedure> or <schema>.<procedure> may run"
            ));
        }
        ProcName::One(name) => (None, name),
        ProcName::Two(schema, name) => (Some(schema), name),
    };

    let name_lower = name.to_ascii_lowercase();
    let schema_is_lookup =
        schema.is_none_or(|s| s.eq_ignore_ascii_case("sys") || s.eq_ignore_ascii_case("dbo"));
    if schema_is_lookup && LOOKUP_PROCEDURES.contains(&name_lower.as_str()) {
        return finish_exec_args(rest, Verdict::Read, head);
    }
    if name_lower == "sp_executesql" {
        return refused(format!(
            "{head} of sp_executesql is not allowed here: it runs dynamic SQL, built as a string, which cannot be classified"
        ));
    }
    if name_lower.starts_with("sp_") || name_lower.starts_with("xp_") {
        return refused(format!(
            "{head} of {name} is not allowed here: only sp_help, sp_helptext, sp_helpindex, sp_columns, sp_tables, sp_stored_procedures, sp_pkeys and sp_fkeys may be called, and only to read"
        ));
    }
    // A named procedure that is not a look-up one can change data, so it
    // gets exactly the rule INSERT/UPDATE/DELETE already have - `allowed`
    // is where that rule is actually enforced against the connection.
    finish_exec_args(rest, Verdict::Write, head)
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

/// The word after each whole-word occurrence of `word` ("" at the end).
fn words_after(upper: &str, word: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(at) = upper[from..].find(word) {
        let start = from + at;
        let end = start + word.len();
        let before = upper[..start].chars().next_back();
        let after = upper[end..].chars().next();
        if !before.is_some_and(is_name_char) && !after.is_some_and(is_name_char) {
            out.push(leading_word(&upper[end..]));
        }
        from = end;
        if from >= upper.len() {
            break;
        }
    }
    out
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
