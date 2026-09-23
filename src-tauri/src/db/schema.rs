//! A ranked lookup over the database's own catalogue, so the assistant can
//! find the right table by describing it rather than by guessing names.
//!
//! Two SELECTs, each through the same guard every other statement passes:
//! `lookup_sql` ranks the tables in ONE scan of `INFORMATION_SCHEMA.COLUMNS`
//! (the way the PHR X DB server searches), and `detail_sql` then reads the
//! matching columns, foreign keys and row counts for the picked tables only,
//! named as constants.
//!
//! It used to be one statement, with the details gathered for "the tables
//! in `picked`" through `EXISTS`. On the real hrmmain database (13,000+
//! tables) SQL Server re-ran the whole ranking for every row it tested, and
//! the lookup never answered inside sqlcmd's 30 s - with or without a schema
//! filter (measured 2026-09-23). Ranked in one scan it takes about 1.5 s,
//! the details about 1.6 s, and the old single statement over the new
//! ranking still 4 s: two round trips are the fast shape, not a compromise.

/// How many tables a lookup may ask for, whatever it was asked for.
const MAX_LIMIT: usize = 100;

/// A term that no table or column can be called, used when the query had
/// no words in it at all: nothing then matches, which is the honest answer.
const NO_TERM: &str = "~no~such~term~";

/// One table the ranking picked, with its score.
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub sch: String,
    pub tab: String,
    pub score: i64,
}

/// The ranking: one scan of the column catalogue, grouped by table.
///
/// A row matches when its table's name OR its column's name contains a
/// term. Per table, the name's score is the same on every row (so `MAX`
/// reads it back) and the best column is the `MAX` of the column scores:
/// table score + best column, the ranking this lookup has always used. The
/// per-term scores are ADDED, so a table matching two words outranks one
/// matching only one. `schema_filter` limits `TABLE_SCHEMA` when it is not
/// empty; `limit` becomes the `TOP (n)`. Answers `sch, tab, score`.
pub fn lookup_sql(query: &str, schema_filter: &str, limit: usize) -> String {
    let terms = terms_of(query);
    let where_schema = if schema_filter.trim().is_empty() {
        String::new()
    } else {
        format!(" AND c.TABLE_SCHEMA = N'{}'", escape(schema_filter.trim()))
    };
    let top = limit.clamp(1, MAX_LIMIT);
    format!(
        "SELECT TOP ({top}) c.TABLE_SCHEMA AS sch, c.TABLE_NAME AS tab,
       MAX({table_score}) + MAX({column_score}) AS score
FROM INFORMATION_SCHEMA.COLUMNS c
WHERE ({table_like} OR {column_like}){where_schema}
GROUP BY c.TABLE_SCHEMA, c.TABLE_NAME
ORDER BY score DESC, c.TABLE_NAME",
        table_score = score_of(&terms, "c.TABLE_NAME", 100, 60),
        column_score = score_of(&terms, "c.COLUMN_NAME", 40, 20),
        table_like = any_like(&terms, "c.TABLE_NAME"),
        column_like = any_like(&terms, "c.COLUMN_NAME"),
    )
}

/// The details for the picked tables: the columns that matched the words,
/// the foreign keys, and a row estimate. The tables arrive as constants, so
/// nothing here has to work out again which tables they were. Answers
/// `sch, tab, rows_est, cols, fks` - what `render_lookup` reads.
pub fn detail_sql(query: &str, picked: &[Picked]) -> String {
    let terms = terms_of(query);
    let values = picked
        .iter()
        .map(|p| format!("(N'{}', N'{}', {})", escape(&p.sch), escape(&p.tab), p.score))
        .collect::<Vec<String>>()
        .join(", ");
    format!(
        "WITH picked AS (
    SELECT sch, tab, score FROM (VALUES {values}) AS p(sch, tab, score)
), matched AS (
    SELECT c.TABLE_SCHEMA AS sch, c.TABLE_NAME AS tab,
           STRING_AGG(CAST(c.COLUMN_NAME + N' ' + c.DATA_TYPE AS NVARCHAR(MAX)), N' | ') AS col_text
    FROM INFORMATION_SCHEMA.COLUMNS c
    JOIN picked p ON p.sch = c.TABLE_SCHEMA AND p.tab = c.TABLE_NAME
    WHERE {column_like}
    GROUP BY c.TABLE_SCHEMA, c.TABLE_NAME
), links AS (
    SELECT fs.name AS sch, fo.name AS tab,
           STRING_AGG(CAST(pc.name + N' -> ' + rs.name + N'.' + ro.name + N'(' + rc.name + N')' AS NVARCHAR(MAX)), N' | ') AS fk_text
    FROM sys.foreign_keys fk
    JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id
    JOIN sys.objects fo ON fo.object_id = fk.parent_object_id
    JOIN sys.schemas fs ON fs.schema_id = fo.schema_id
    JOIN picked p ON p.sch = fs.name AND p.tab = fo.name
    JOIN sys.columns pc ON pc.object_id = fkc.parent_object_id AND pc.column_id = fkc.parent_column_id
    JOIN sys.objects ro ON ro.object_id = fk.referenced_object_id
    JOIN sys.schemas rs ON rs.schema_id = ro.schema_id
    JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id
    GROUP BY fs.name, fo.name
)
SELECT p.sch AS sch, p.tab AS tab,
       ISNULL((SELECT SUM(pa.[rows]) FROM sys.partitions pa
               WHERE pa.object_id = OBJECT_ID(QUOTENAME(p.sch) + N'.' + QUOTENAME(p.tab))
                 AND pa.index_id IN (0, 1)), 0) AS rows_est,
       ISNULL(m.col_text, N'') AS cols,
       ISNULL(l.fk_text, N'') AS fks
FROM picked p
LEFT JOIN matched m ON m.sch = p.sch AND m.tab = p.tab
LEFT JOIN links l ON l.sch = p.sch AND l.tab = p.tab
ORDER BY p.score DESC, p.tab",
        column_like = any_like(&terms, "c.COLUMN_NAME"),
    )
}

/// The tables `lookup_sql`'s answer names, in its order. sqlcmd's header,
/// rule and footer are skipped, and so is anything that is not three cells
/// ending in a whole number.
pub fn parse_ranked(tsv: &str) -> Vec<Picked> {
    tsv.lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.trim().is_empty() && !is_rule(line) && !is_footer(line))
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('\t').map(str::trim).collect();
            match cells.as_slice() {
                [sch, tab, score] if !sch.is_empty() && !tab.is_empty() => score
                    .parse::<i64>()
                    .ok()
                    .map(|score| Picked { sch: sch.to_string(), tab: tab.to_string(), score }),
                _ => None,
            }
        })
        .collect()
}

/// The per-term scores for one name, added: an exact match, a partial one,
/// or nothing.
fn score_of(terms: &[String], expr: &str, exact: u32, partial: u32) -> String {
    terms
        .iter()
        .map(|t| {
            let t = escape(t);
            format!(
                "CASE WHEN LOWER({expr}) = N'{t}' THEN {exact} WHEN LOWER({expr}) LIKE N'%{t}%' THEN {partial} ELSE 0 END"
            )
        })
        .collect::<Vec<String>>()
        .join(" + ")
}

/// Whether a name contains any of the terms.
fn any_like(terms: &[String], expr: &str) -> String {
    terms
        .iter()
        .map(|t| format!("LOWER({expr}) LIKE N'%{}%'", escape(t)))
        .collect::<Vec<String>>()
        .join(" OR ")
}

/// The words of a query: lower-cased, split on everything that is not a
/// letter, a digit or an apostrophe. The apostrophe stays because it
/// belongs to the word (`o'brien`), and is doubled by `escape` on the way
/// into the literal.
///
/// A single character is dropped. It is a substring of half the names in
/// the database, so it contributes the same 60 points to everything and
/// only drags unrelated tables up the ranking.
fn terms_of(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = query
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|w| w.trim_matches('\'').to_lowercase())
        .filter(|w| w.chars().any(|c| c.is_alphanumeric()) && w.chars().count() > 1)
        .collect();
    terms.sort();
    terms.dedup();
    if terms.is_empty() {
        terms.push(NO_TERM.to_string());
    }
    terms
}

/// A T-SQL string literal's contents: the one character that ends a
/// literal is the one that has to be doubled.
fn escape(value: &str) -> String {
    value.replace('\'', "''")
}

/// Renders sqlcmd's answer to `lookup_sql` as text an assistant reads: one
/// block per table, its matching columns, then its foreign keys.
pub fn render_lookup(tsv: &str) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for line in tsv.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || is_rule(line) || is_footer(line) {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').map(|c| c.trim()).collect();
        if cells.len() < 5 || (cells[0] == "sch" && cells[1] == "tab") {
            continue;
        }
        let (sch, tab, rows, cols, fks) = (cells[0], cells[1], cells[2], cells[3], cells[4]);
        let mut block = format!("{sch}.{tab} ({rows} rows est.)");
        if !cols.is_empty() {
            block.push_str(&format!("\n  columns: {}", cols.replace(" | ", ", ")));
        }
        if !fks.is_empty() {
            for fk in fks.split(" | ") {
                block.push_str(&format!("\n  foreign key: {fk}"));
            }
        }
        blocks.push(block);
    }
    if blocks.is_empty() {
        return "no table or column matches those words".to_string();
    }
    blocks.join("\n\n")
}

/// The whole column list of ONE table, when the words are a table's name
/// rather than a topic - `dbo.LeaveRequest`, or `LeaveRequest` on its own.
/// `None` for anything else, which is the signal to rank instead.
///
/// The ranked lookup above answers "which tables are about this" and shows
/// only the columns that matched the words. Asked for a table by name, an
/// assistant wants the other question - "what is in this table" - and the
/// matching-columns filter is then exactly the wrong answer. Same tool,
/// because an assistant should not have to know which shape it is asking
/// for; the name it typed is what decides.
pub fn describe_sql(query: &str) -> Option<String> {
    let (schema_name, table) = split_name(query)?;
    // `escape` is still applied although `is_name` has already ruled out a
    // quote: the guard's rule is that nothing reaches a literal unescaped,
    // and an exception argued from a caller's behaviour is how that rule
    // stops holding later.
    let where_schema = match schema_name {
        Some(s) => format!(" AND LOWER(c.TABLE_SCHEMA) = N'{}'", escape(&s)),
        None => String::new(),
    };
    Some(format!(
        "SELECT TOP (500) c.TABLE_SCHEMA AS sch, c.TABLE_NAME AS tab, c.COLUMN_NAME AS col,
       c.DATA_TYPE AS typ, c.CHARACTER_MAXIMUM_LENGTH AS len, c.IS_NULLABLE AS nul
FROM INFORMATION_SCHEMA.COLUMNS c
WHERE LOWER(c.TABLE_NAME) = N'{}'{where_schema}
ORDER BY c.TABLE_SCHEMA, c.TABLE_NAME, c.ORDINAL_POSITION",
        escape(&table)
    ))
}

/// `schema.table` or `table`, lower-cased, or `None` when the words are
/// not a name: anything with a space, a punctuation mark other than the
/// one dot, two dots, or an empty half.
fn split_name(query: &str) -> Option<(Option<String>, String)> {
    let query = query.trim();
    match query.split_once('.') {
        Some((schema_name, table)) => {
            (is_name(schema_name) && is_name(table))
                .then(|| (Some(schema_name.to_lowercase()), table.to_lowercase()))
        }
        None => is_name(query).then(|| (None, query.to_lowercase())),
    }
}

/// A SQL Server identifier as a person types one: a letter or underscore,
/// then letters, digits and underscores. Deliberately narrower than what
/// the server allows - a name needing brackets is a name this shortcut
/// declines, and the ranked lookup still finds it.
fn is_name(word: &str) -> bool {
    let mut chars = word.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Renders sqlcmd's answer to `describe_sql`. Empty when the table does
/// not exist, which is what tells the caller to fall back to the ranked
/// lookup rather than reporting "no such table" for a word that was never
/// meant as one.
///
/// A bare name with no schema matches every schema that has a table of
/// that name (`dbo.LeaveRequest` AND `hr.LeaveRequest`), so the rows are
/// grouped by `sch`+`tab` and rendered one block per table - the same
/// grouping `render_lookup` does, just by consecutive rows here since the
/// query's own `ORDER BY` keeps one table's columns together.
pub fn render_describe(tsv: &str) -> String {
    let mut tables: Vec<(String, String, Vec<String>)> = Vec::new();
    for line in tsv.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() || is_rule(line) || is_footer(line) {
            continue;
        }
        let cells: Vec<&str> = line.split('\t').map(|c| c.trim()).collect();
        if cells.len() < 6 || (cells[0] == "sch" && cells[1] == "tab") {
            continue;
        }
        let (sch, tab, col, typ, len, nul) =
            (cells[0], cells[1], cells[2], cells[3], cells[4], cells[5]);
        let width = match len {
            // -1 is how SQL Server reports the (max) types; NULL is every
            // type that has no length of its own.
            "-1" => "(max)".to_string(),
            "NULL" | "" => String::new(),
            other => format!("({other})"),
        };
        let nullable = if nul.eq_ignore_ascii_case("NO") { "not null" } else { "null" };
        let column = format!("{col} {typ}{width} {nullable}");
        match tables.last_mut() {
            Some((s, t, cols)) if s == sch && t == tab => cols.push(column),
            _ => tables.push((sch.to_string(), tab.to_string(), vec![column])),
        }
    }
    if tables.is_empty() {
        return String::new();
    }
    tables
        .into_iter()
        .map(|(sch, tab, cols)| {
            format!("{sch}.{tab} ({} columns)\n  columns: {}", cols.len(), cols.join(", "))
        })
        .collect::<Vec<String>>()
        .join("\n\n")
}

/// sqlcmd draws a rule of dashes under the header row.
///
/// `pub(crate)` so `query::data_row_count` can tell sqlcmd's own furniture
/// from an actual row without re-implementing this test.
pub(crate) fn is_rule(line: &str) -> bool {
    let bare: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    !bare.is_empty() && bare.chars().all(|c| c == '-')
}

/// And signs off with "(N rows affected)".
pub(crate) fn is_footer(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('(') && line.ends_with("rows affected)")
}
