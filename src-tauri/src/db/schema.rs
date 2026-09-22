//! A ranked lookup over the database's own catalogue, so the assistant can
//! find the right table by describing it rather than by guessing names.
//!
//! One SELECT, deliberately: it has to pass the same guard every other
//! statement passes, and a guard that made an exception for its own query
//! would not be a guard. That rules out dynamic SQL, temp tables and a
//! second round trip for the foreign keys - the keys are folded into the
//! same statement with `STRING_AGG` (SQL Server 2017 and later).

/// How many tables a lookup may ask for, whatever it was asked for.
const MAX_LIMIT: usize = 100;

/// A term that no table or column can be called, used when the query had
/// no words in it at all: the scores then stay at zero and the `score > 0`
/// filter returns nothing, which is the honest answer.
const NO_TERM: &str = "~no~such~term~";

/// The SQL for one lookup. `schema_filter` limits `TABLE_SCHEMA` when it is
/// not empty; `limit` becomes the `TOP (n)`.
pub fn lookup_sql(query: &str, schema_filter: &str, limit: usize) -> String {
    let terms = terms_of(query);
    let values = terms
        .iter()
        .map(|t| format!("(N'{}')", escape(t)))
        .collect::<Vec<String>>()
        .join(", ");
    // The same predicate on both catalogue views, so a filtered lookup
    // never scores a table from one schema against a column from another.
    let where_schema = if schema_filter.trim().is_empty() {
        String::new()
    } else {
        format!(" AND {{}}.TABLE_SCHEMA = N'{}'", escape(schema_filter.trim()))
    };
    let tables_schema = where_schema.replace("{}", "t");
    let columns_schema = where_schema.replace("{}", "c");
    let top = limit.clamp(1, MAX_LIMIT);

    format!(
        "WITH terms AS (
    SELECT term FROM (VALUES {values}) AS v(term)
), tables_scored AS (
    SELECT t.TABLE_SCHEMA AS sch, t.TABLE_NAME AS tab,
           SUM(CASE WHEN LOWER(t.TABLE_NAME) = v.term THEN 100
                    WHEN LOWER(t.TABLE_NAME) LIKE N'%' + v.term + N'%' THEN 60
                    ELSE 0 END) AS tab_score
    FROM INFORMATION_SCHEMA.TABLES t CROSS JOIN terms v
    WHERE 1 = 1{tables_schema}
    GROUP BY t.TABLE_SCHEMA, t.TABLE_NAME
), columns_scored AS (
    SELECT c.TABLE_SCHEMA AS sch, c.TABLE_NAME AS tab, c.COLUMN_NAME AS col, c.DATA_TYPE AS typ,
           SUM(CASE WHEN LOWER(c.COLUMN_NAME) = v.term THEN 40
                    WHEN LOWER(c.COLUMN_NAME) LIKE N'%' + v.term + N'%' THEN 20
                    ELSE 0 END) AS col_score
    FROM INFORMATION_SCHEMA.COLUMNS c CROSS JOIN terms v
    WHERE 1 = 1{columns_schema}
    GROUP BY c.TABLE_SCHEMA, c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE
), ranked AS (
    SELECT s.sch, s.tab, s.tab_score + ISNULL((SELECT MAX(k.col_score) FROM columns_scored k
                                               WHERE k.sch = s.sch AND k.tab = s.tab), 0) AS score
    FROM tables_scored s
), picked AS (
    SELECT TOP ({top}) sch, tab, score FROM ranked WHERE score > 0 ORDER BY score DESC, tab
), matched AS (
    SELECT k.sch, k.tab,
           STRING_AGG(CAST(k.col + N' ' + k.typ AS NVARCHAR(MAX)), N' | ') AS col_text
    FROM columns_scored k
    WHERE k.col_score > 0
      AND EXISTS (SELECT 1 FROM picked p WHERE p.sch = k.sch AND p.tab = k.tab)
    GROUP BY k.sch, k.tab
), links AS (
    SELECT fs.name AS sch, fo.name AS tab,
           STRING_AGG(CAST(pc.name + N' -> ' + rs.name + N'.' + ro.name + N'(' + rc.name + N')' AS NVARCHAR(MAX)), N' | ') AS fk_text
    FROM sys.foreign_keys fk
    JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id
    JOIN sys.objects fo ON fo.object_id = fk.parent_object_id
    JOIN sys.schemas fs ON fs.schema_id = fo.schema_id
    JOIN sys.columns pc ON pc.object_id = fkc.parent_object_id AND pc.column_id = fkc.parent_column_id
    JOIN sys.objects ro ON ro.object_id = fk.referenced_object_id
    JOIN sys.schemas rs ON rs.schema_id = ro.schema_id
    JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id
    WHERE EXISTS (SELECT 1 FROM picked p WHERE p.sch = fs.name AND p.tab = fo.name)
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
ORDER BY p.score DESC, p.tab"
    )
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

/// sqlcmd draws a rule of dashes under the header row.
fn is_rule(line: &str) -> bool {
    let bare: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    !bare.is_empty() && bare.chars().all(|c| c == '-')
}

/// And signs off with "(N rows affected)".
fn is_footer(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('(') && line.ends_with("rows affected)")
}
