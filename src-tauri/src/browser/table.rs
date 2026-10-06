//! Tables and grids: `expect_row`, `expect_no_row`, `expect_sorted` and
//! `expect_row_count`.
//!
//! The page is asked ONE thing - the table's header texts and rows, as a
//! person reads them (`READ_TABLE_JS`) - and everything a check decides is
//! decided here in Rust, from that read, by pure functions (`find_row`,
//! `check_sorted`, `count_check`). The table is found the way every
//! locator finds its element, frame chains included, and read in the frame
//! it lives in.
//!
//! - **An HTML `<table>`**: headers from `thead th`, or from the first row
//!   when every cell of it is a `th`; rows are every other row, outside
//!   `thead` and `tfoot`.
//! - **An ARIA grid** (`role=grid`, `treegrid` or `table`): headers from
//!   `role=columnheader` in order; rows are `role=row` holding no column
//!   header and not `aria-hidden`; cells are `gridcell`, `cell` and
//!   `rowheader`, in page order.
//!
//! Cells are matched to headers by position (a `colspan` is not followed),
//! and a short row is padded with empty cells. Only rendered rows are read:
//! a grid that pages or loads as it scrolls is checked as shown.
//!
//! Each check looks again until it holds or the step's check timeout runs
//! out, re-reading the table every time, as `expect_text` does: a grid still
//! loading is waited for. Only the last look's failure is said.

use super::actions::{harness, harness_timeout, ActionOutcome};
use super::cdp::{CdpError, Driver};
use super::dialogs::cut;
use super::expect::NOT_ON_PAGE;
use super::input::{matched_many, STILL_LOOKING};
use super::locator::{resolve_explained, Target};
use super::page;
use serde_json::Value;
use std::time::{Duration, Instant};

/// `this` is the element the `table` locator found. Its header texts and
/// rows, or `null` when it is neither a `<table>` nor an ARIA grid. Text is
/// what is rendered (`innerText`), trimmed, with inner spaces collapsed.
pub const READ_TABLE_JS: &str = r#"function() {
  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  const text = (el) => norm(el.innerText);
  const shown = (el) => !el.checkVisibility || el.checkVisibility();
  if (this.tagName === 'TABLE') {
    const thead = this.tHead, tfoot = this.tFoot;
    let headerRow = null;
    if (thead && thead.rows.length) headerRow = thead.rows[thead.rows.length - 1];
    const all = Array.from(this.rows).filter((r) => !(tfoot && tfoot.contains(r)) && !(thead && thead.contains(r)));
    if (!headerRow && all.length && all[0].cells.length && Array.from(all[0].cells).every((c) => c.tagName === 'TH')) {
      headerRow = all[0];
    }
    const headers = headerRow ? Array.from(headerRow.cells).map(text) : [];
    const rows = all.filter((r) => r !== headerRow && shown(r)).map((r) => Array.from(r.cells).map(text));
    return { headers, rows };
  }
  const role = (this.getAttribute('role') || '').toLowerCase();
  if (role !== 'grid' && role !== 'treegrid' && role !== 'table') return null;
  const GRID = '[role=grid],[role=treegrid],[role=table]';
  // Not a grid nested inside this one.
  const own = (el) => el.closest(GRID) === this;
  const headers = Array.from(this.querySelectorAll('[role=columnheader]')).filter(own).map(text);
  const rows = Array.from(this.querySelectorAll('[role=row]'))
    .filter(own)
    .filter((r) => !r.querySelector('[role=columnheader]'))
    .filter((r) => !r.closest('[aria-hidden=true]') && shown(r))
    .map((r) => Array.from(r.querySelectorAll('[role=gridcell],[role=cell],[role=rowheader]'))
      .filter((c) => c.closest('[role=row]') === r)
      .map(text));
  return { headers, rows };
}"#;

/// What the page gave back: header texts, and each row's cell texts, padded
/// to the headers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TableRead {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl TableRead {
    /// From the reader's answer; `None` when the element was not a table.
    pub fn from_value(v: &Value) -> Option<TableRead> {
        let obj = v.as_object()?;
        let strs = |v: &Value| -> Vec<String> {
            v.as_array().into_iter().flatten().map(|s| s.as_str().unwrap_or("").to_string()).collect()
        };
        let headers = strs(obj.get("headers")?);
        let rows = obj
            .get("rows")?
            .as_array()
            .into_iter()
            .flatten()
            .map(|r| {
                let mut cells = strs(r);
                if cells.len() < headers.len() {
                    cells.resize(headers.len(), String::new());
                }
                cells
            })
            .collect();
        Some(TableRead { headers, rows })
    }

    /// The column called `name` (trimmed, case ignored, every run of
    /// whitespace one space, a Unicode space included), or the sentence
    /// that says it has none.
    pub fn column(&self, name: &str) -> Result<usize, String> {
        let want = collapse(name).to_lowercase();
        self.headers.iter().position(|h| collapse(h).to_lowercase() == want).ok_or_else(|| {
            let list = if self.headers.is_empty() {
                "none - it has no header row".to_string()
            } else {
                self.headers.iter().map(|h| format!("\"{}\"", cut(h))).collect::<Vec<_>>().join(", ")
            };
            format!("the table has no column \"{}\" - its columns are {list}", name.trim())
        })
    }

    /// One cell's text; empty past the end of a ragged row.
    fn cell(&self, row: usize, col: usize) -> &str {
        self.rows[row].get(col).map(String::as_str).unwrap_or("")
    }
}

/// The cells an `expect_row` or `expect_no_row` looks for, by column, in
/// the order the script wrote them - a JSON object of strings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cells(pub Vec<(String, String)>);

impl serde::Serialize for Cells {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl<'de> serde::Deserialize<'de> for Cells {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        // serde_json keeps an object's order (`preserve_order`).
        let map = serde_json::Map::<String, Value>::deserialize(d)?;
        map.into_iter()
            .map(|(k, v)| match v {
                Value::String(s) => Ok((k, s)),
                _ => Err(D::Error::custom(format!("the cell \"{k}\" must be text"))),
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Cells)
    }
}

/// `<locator> is not a table or grid`.
pub fn not_a_table(target: &str) -> String {
    format!("{target} is not a table or grid")
}

/// Wanted cells as a sentence says them: `Status "Active", Name "Ann"`.
pub fn cells_words(cells: &[(String, String)]) -> String {
    cells.iter().map(|(c, t)| format!("{} \"{t}\"", c.trim())).collect::<Vec<_>>().join(", ")
}

/// The first row (0-based) holding every wanted cell: each text contained
/// in its cell, or equal to it with `exact` - case ignored either way.
/// `Err` is an unknown column.
pub fn find_row(read: &TableRead, cells: &[(String, String)], exact: bool) -> Result<Option<usize>, String> {
    let cols: Vec<(usize, String)> = cells
        .iter()
        .map(|(c, t)| read.column(c).map(|i| (i, collapse(t).to_lowercase())))
        .collect::<Result<_, _>>()?;
    Ok((0..read.rows.len()).find(|&r| {
        cols.iter().all(|(i, want)| {
            let got = collapse(read.cell(r, *i)).to_lowercase();
            if exact {
                got == *want
            } else {
                got.contains(want.as_str())
            }
        })
    }))
}

/// Every run of whitespace one space, none at either end. A non-breaking
/// or other Unicode space is whitespace (`char::is_whitespace`).
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `expect_row`: holds when some row has every cell.
pub fn row_check(read: &TableRead, cells: &[(String, String)], exact: bool) -> Result<String, String> {
    match find_row(read, cells, exact)? {
        Some(r) => Ok(format!("row {} has {}", r + 1, cells_words(cells))),
        None => Err(format!("no row has {} - the table has {} rows", cells_words(cells), read.rows.len())),
    }
}

/// `expect_no_row`: holds when no row has every cell.
pub fn no_row_check(read: &TableRead, cells: &[(String, String)], exact: bool) -> Result<String, String> {
    match find_row(read, cells, exact)? {
        Some(r) => Err(format!("a row has {} (row {})", cells_words(cells), r + 1)),
        None => Ok(format!("no row has {} - the table has {} rows", cells_words(cells), read.rows.len())),
    }
}

/// Ascending or descending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    pub fn word(self) -> &'static str {
        match self {
            SortOrder::Ascending => "ascending",
            SortOrder::Descending => "descending",
        }
    }
}

/// How `expect_sorted` reads a column: `"text"`, `"number"`, `"date"`, or
/// `{ "date": "dd/MM/yyyy" }` for dates in one given format.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(untagged)]
pub enum SortAs {
    Kind(SortKind),
    Format { date: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SortKind {
    Text,
    Number,
    Date,
}

/// A value to order by.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
enum Key {
    Text(String),
    Number(f64),
    Date((i32, u32, u32)),
}

/// A number as tables write it: a leading `-`, digits with `,` between
/// thousands, an optional `.` and decimals.
pub fn parse_number(value: &str) -> Option<f64> {
    let mut v = value.trim();
    // One trailing percent sign: `40%` is 40.
    if let Some(rest) = v.strip_suffix('%') {
        v = rest.trim_end();
    }
    // One leading currency symbol or code, before or after a leading `-`,
    // with or without a space: `-$5`, `$-5`, `LKR 1,250.50`, `Rs. 900`.
    let mut neg = false;
    if let Some(rest) = v.strip_prefix('-') {
        neg = true;
        v = rest.trim_start();
    }
    if let Some(rest) = strip_currency(v) {
        v = rest.trim_start();
        if !neg {
            if let Some(rest) = v.strip_prefix('-') {
                neg = true;
                v = rest.trim_start();
            }
        }
    }
    let body = v;
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    if int.is_empty() || !int.chars().all(|c| c.is_ascii_digit() || c == ',') {
        return None;
    }
    if int.contains(',') {
        let groups: Vec<&str> = int.split(',').collect();
        let first_ok = (1..=3).contains(&groups[0].len());
        if !first_ok || groups[1..].iter().any(|g| g.len() != 3) {
            return None;
        }
    }
    if let Some(f) = frac {
        if f.is_empty() || !f.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
    }
    let plain: String = int.chars().filter(|c| *c != ',').collect();
    let n: f64 = match frac {
        Some(f) => format!("{plain}.{f}").parse().ok()?,
        None => plain.parse().ok()?,
    };
    Some(if neg { -n } else { n })
}

/// `v` without the currency symbol or code it starts with: `$`, the pound
/// and euro signs as written, and `LKR`, `Rs.` and `Rs` ignoring case.
/// `None` when it starts with none - or with letters that only begin a
/// longer word.
fn strip_currency(v: &str) -> Option<&str> {
    for sym in ["$", "\u{a3}", "\u{20ac}"] {
        if let Some(rest) = v.strip_prefix(sym) {
            return Some(rest);
        }
    }
    for code in ["lkr", "rs.", "rs"] {
        let head: String = v.chars().take(code.len()).collect();
        if head.eq_ignore_ascii_case(code) {
            let rest = &v[head.len()..];
            if !rest.starts_with(|c: char| c.is_alphabetic()) {
                return Some(rest);
            }
        }
    }
    None
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn days_in(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        _ => 0,
    }
}

fn valid(y: i32, m: u32, d: u32) -> Option<(i32, u32, u32)> {
    ((1..=12).contains(&m) && d >= 1 && d <= days_in(y, m)).then_some((y, m, d))
}

/// One piece of a date format: `yyyy`, `MMM`, `MM`, `dd`, `d`, or a
/// character that must be there as it is.
#[derive(Debug, Clone, PartialEq)]
enum Part {
    Year,
    MonthName,
    Month,
    Day2,
    Day,
    Lit(char),
}

/// A date format in the letters the guide names, or `None` when it is not
/// one: it needs a `yyyy`, a month (`MM` or `MMM`) and a day (`dd` or `d`).
fn format_parts(fmt: &str) -> Option<Vec<Part>> {
    let mut parts = Vec::new();
    let mut rest = fmt;
    while !rest.is_empty() {
        let (part, len) = if rest.starts_with("yyyy") {
            (Part::Year, 4)
        } else if rest.starts_with("MMM") {
            (Part::MonthName, 3)
        } else if rest.starts_with("MM") {
            (Part::Month, 2)
        } else if rest.starts_with("dd") {
            (Part::Day2, 2)
        } else if rest.starts_with('d') {
            (Part::Day, 1)
        } else {
            let c = rest.chars().next().expect("not empty");
            if c.is_ascii_alphabetic() {
                return None;
            }
            (Part::Lit(c), c.len_utf8())
        };
        parts.push(part);
        rest = &rest[len..];
    }
    let has = |p: &[Part]| parts.iter().any(|x| p.contains(x));
    (has(&[Part::Year]) && has(&[Part::Month, Part::MonthName]) && has(&[Part::Day, Part::Day2])).then_some(parts)
}

/// Is `fmt` a date format `expect_sorted` can read?
pub fn is_date_format(fmt: &str) -> bool {
    format_parts(fmt).is_some()
}

fn take_digits(s: &str, min: usize, max: usize) -> Option<(u32, &str)> {
    let n = s.chars().take(max).take_while(char::is_ascii_digit).count();
    if n < min {
        return None;
    }
    Some((s[..n].parse().ok()?, &s[n..]))
}

/// `value` read in the format `fmt`.
fn parse_with(value: &str, parts: &[Part]) -> Option<(i32, u32, u32)> {
    let (mut y, mut m, mut d) = (None, None, None);
    let mut rest = value.trim();
    for p in parts {
        match p {
            Part::Year => {
                let (n, r) = take_digits(rest, 4, 4)?;
                y = Some(n as i32);
                rest = r;
            }
            Part::Month | Part::Day2 => {
                let (n, r) = take_digits(rest, 2, 2)?;
                if *p == Part::Month { m = Some(n) } else { d = Some(n) }
                rest = r;
            }
            Part::Day => {
                let (n, r) = take_digits(rest, 1, 2)?;
                d = Some(n);
                rest = r;
            }
            Part::MonthName => {
                let word: String = rest.chars().take(3).collect();
                let i = MONTHS.iter().position(|mm| word.eq_ignore_ascii_case(mm))?;
                m = Some(i as u32 + 1);
                rest = &rest[word.len()..];
            }
            Part::Lit(c) => rest = rest.strip_prefix(*c)?,
        }
    }
    if !rest.is_empty() {
        return None;
    }
    valid(y?, m?, d?)
}

fn parts_of(fmt: &str) -> Vec<Part> {
    format_parts(fmt).expect("a format the reader knows")
}

/// A date in one of the formats read without being told: `yyyy-MM-dd` or
/// `d MMM yyyy`. Slashed dates are read by the caller, which has to decide
/// between `dd/MM` and `MM/dd`.
fn parse_unslashed(value: &str) -> Option<(i32, u32, u32)> {
    parse_with(value, &parts_of("yyyy-MM-dd")).or_else(|| parse_with(value, &parts_of("d MMM yyyy")))
}

fn not_a(value: &str, column: &str, what: &str) -> String {
    format!("\"{}\" in {column} is not a {what}", cut(value))
}

/// The dates in a column, ambiguity decided: every slashed date read as
/// `dd/MM/yyyy`, or every one as `MM/dd/yyyy` - whichever reads them all.
/// When both do and they order the column differently, the column could be
/// read two ways and the script has to say which.
fn date_keys(column: &str, values: &[&str]) -> Result<Vec<Key>, String> {
    let (dmy, mdy) = (parts_of("dd/MM/yyyy"), parts_of("MM/dd/yyyy"));
    let read = |parts: &[Part]| -> Option<Vec<(i32, u32, u32)>> {
        values.iter().map(|v| parse_unslashed(v).or_else(|| parse_with(v, parts))).collect()
    };
    match (read(&dmy), read(&mdy)) {
        (Some(a), Some(b)) => {
            let order = |k: &[(i32, u32, u32)]| k.windows(2).map(|w| w[0].cmp(&w[1])).collect::<Vec<_>>();
            if a != b && order(&a) != order(&b) {
                return Err(format!("the dates in {column} could be read two ways - give as: \"date\" a format"));
            }
            Ok(a.into_iter().map(Key::Date).collect())
        }
        (Some(a), None) | (None, Some(a)) => Ok(a.into_iter().map(Key::Date).collect()),
        (None, None) => {
            let bad = values
                .iter()
                .find(|v| parse_unslashed(v).or_else(|| parse_with(v, &dmy)).or_else(|| parse_with(v, &mdy)).is_none())
                // Every value reads one way or the other, but not all the
                // same way: the first that the day-first reading refuses.
                .or_else(|| values.iter().find(|v| parse_unslashed(v).or_else(|| parse_with(v, &dmy)).is_none()))
                .copied()
                .unwrap_or("");
            Err(not_a(bad, column, "date"))
        }
    }
}

/// Are `values` (one per row, in row order) in `order`, read `as`? Blank
/// cells are passed over. The failure names the first two rows out of
/// order (1-based), or a value that cannot be read so.
pub fn check_sorted(column: &str, values: &[String], order: SortOrder, sort_as: &SortAs) -> Result<String, String> {
    let kept: Vec<(usize, &str)> =
        values.iter().enumerate().map(|(i, v)| (i, v.trim())).filter(|(_, v)| !v.is_empty()).collect();
    let texts: Vec<&str> = kept.iter().map(|(_, v)| *v).collect();
    let keys: Vec<Key> = match sort_as {
        SortAs::Kind(SortKind::Text) => texts.iter().map(|v| Key::Text(v.to_lowercase())).collect(),
        SortAs::Kind(SortKind::Number) => texts
            .iter()
            .map(|v| parse_number(v).map(Key::Number).ok_or_else(|| not_a(v, column, "number")))
            .collect::<Result<_, _>>()?,
        SortAs::Kind(SortKind::Date) => date_keys(column, &texts)?,
        SortAs::Format { date } => {
            let parts = format_parts(date).ok_or_else(|| format!("\"{date}\" is not a date format - use letters such as dd/MM/yyyy"))?;
            texts
                .iter()
                .map(|v| parse_with(v, &parts).map(Key::Date).ok_or_else(|| not_a(v, column, "date")))
                .collect::<Result<_, _>>()?
        }
    };
    for (i, w) in keys.windows(2).enumerate() {
        let wrong = match order {
            SortOrder::Ascending => w[0] > w[1],
            SortOrder::Descending => w[0] < w[1],
        };
        if wrong {
            let (ra, a) = kept[i];
            let (rb, b) = kept[i + 1];
            return Err(format!(
                "{column} is not in {} order - row {} \"{}\" comes before row {} \"{}\"",
                order.word(),
                ra + 1,
                cut(a),
                rb + 1,
                cut(b)
            ));
        }
    }
    Ok(format!("{column} is in {} order ({} values)", order.word(), keys.len()))
}

/// What `expect_row_count` wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowCount {
    Equals(u32),
    AtLeast(u32),
    AtMost(u32),
}

/// The refusal when not exactly one is given.
pub const ONE_COUNT: &str = "expect_row_count takes one of equals, at_least or at_most";

/// The one count a script gave, or the refusal.
pub fn row_count(equals: Option<u32>, at_least: Option<u32>, at_most: Option<u32>) -> Result<RowCount, String> {
    match (equals, at_least, at_most) {
        (Some(n), None, None) => Ok(RowCount::Equals(n)),
        (None, Some(n), None) => Ok(RowCount::AtLeast(n)),
        (None, None, Some(n)) => Ok(RowCount::AtMost(n)),
        _ => Err(ONE_COUNT.to_string()),
    }
}

/// Does a table of `rows` rows have the count wanted?
pub fn count_check(rows: usize, want: RowCount) -> Result<String, String> {
    let n = rows as u64;
    let (ok, words) = match want {
        RowCount::Equals(e) => (n == u64::from(e), e.to_string()),
        RowCount::AtLeast(e) => (n >= u64::from(e), format!("at least {e}")),
        RowCount::AtMost(e) => (n <= u64::from(e), format!("at most {e}")),
    };
    if ok {
        Ok(format!("the table has {rows} rows"))
    } else {
        Err(format!("the table has {rows} rows, not {words}"))
    }
}

/// One table check, as the reader's answer is judged.
pub enum TableCheck<'a> {
    Row { cells: &'a [(String, String)], exact: bool },
    NoRow { cells: &'a [(String, String)], exact: bool },
    Sorted { column: &'a str, order: SortOrder, sort_as: &'a SortAs },
    Count(RowCount),
}

impl TableCheck<'_> {
    pub fn judge(&self, read: &TableRead) -> Result<String, String> {
        match self {
            TableCheck::Row { cells, exact } => row_check(read, cells, *exact),
            TableCheck::NoRow { cells, exact } => no_row_check(read, cells, *exact),
            TableCheck::Sorted { column, order, sort_as } => {
                let i = read.column(column)?;
                let values: Vec<String> = (0..read.rows.len()).map(|r| read.cell(r, i).to_string()).collect();
                check_sorted(column.trim(), &values, *order, sort_as)
            }
            TableCheck::Count(want) => count_check(read.rows.len(), *want),
        }
    }
}

/// Where a look ended, with the table it read when it read one.
enum Look {
    Holds { said: String, read: TableRead },
    /// Not yet, and what to say if it never does: a sentence of the
    /// table's own, or what stood between the locator and the table.
    NotYet { said: String, by_locator: bool, read: Option<TableRead> },
}

/// How long a table must stay the same before a check that a row is
/// ABSENT may pass (`TableCheck::needs_settling`).
pub const SETTLE_MS: u64 = 750;
/// The least time between two reads while a table settles.
pub const SETTLE_POLL_MS: u64 = 250;

impl TableCheck<'_> {
    /// Does this check pass on a table with too FEW rows? `expect_no_row`,
    /// a count of 0 and `at_most` do - so a grid whose rows have not
    /// arrived yet would pass them at once. They pass only once the table
    /// has held still for `SETTLE_MS`; the others pass as soon as they hold.
    pub fn needs_settling(&self) -> bool {
        matches!(self, TableCheck::NoRow { .. } | TableCheck::Count(RowCount::Equals(0) | RowCount::AtMost(_)))
    }
}

async fn look<D: Driver>(d: &mut D, target: &Target, check: &TableCheck<'_>) -> Result<Look, CdpError> {
    let found = resolve_explained(d, target).await?;
    let handle = match found.handles.as_slice() {
        [] => {
            let why = found.unreachable_frame.unwrap_or_else(|| NOT_ON_PAGE.to_string());
            return Ok(Look::NotYet { said: why, by_locator: true, read: None });
        }
        [one] => one.clone(),
        many if target.is_legacy() => many[0].clone(),
        many => return Ok(Look::NotYet { said: matched_many(many.len()), by_locator: true, read: None }),
    };
    let v = page::call_value(d, &handle, READ_TABLE_JS, &[]).await?;
    let Some(read) = TableRead::from_value(&v) else {
        return Ok(Look::NotYet { said: not_a_table(&target.describe()), by_locator: false, read: None });
    };
    Ok(match check.judge(&read) {
        Ok(said) => Look::Holds { said, read },
        Err(said) => Look::NotYet { said, by_locator: false, read: Some(read) },
    })
}

/// Look, and look again (the table re-read each time) until `check` holds
/// - and, for a check that a row is absent, until the table has also held
/// still for `SETTLE_MS` - or `timeout_ms` runs out. Then the last read is
/// judged. The deadline is cleared on every way out.
pub async fn expect_table<D: Driver>(
    d: &mut D,
    target: &Target,
    check: TableCheck<'_>,
    timeout_ms: u64,
    poll_ms: u64,
) -> ActionOutcome {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    d.set_deadline(Some(deadline));
    let out = keep_looking(d, target, &check, timeout_ms, poll_ms, deadline).await;
    d.set_deadline(None);
    out
}

async fn keep_looking<D: Driver>(
    d: &mut D,
    target: &Target,
    check: &TableCheck<'_>,
    timeout_ms: u64,
    poll_ms: u64,
    deadline: Instant,
) -> ActionOutcome {
    let mut looked = false;
    let mut last = format!("waited {timeout_ms}ms: {} {STILL_LOOKING}", target.describe());
    let settling = check.needs_settling();
    // The table as last read, and since when it has read the same.
    let mut same: Option<(TableRead, Instant)> = None;
    // The last read holds, but has not held still long enough yet.
    let mut holding: Option<String> = None;
    let mut keep = |read: Option<TableRead>| -> Duration {
        let Some(read) = read else {
            same = None;
            return Duration::ZERO;
        };
        let since = match &same {
            Some((before, t)) if *before == read => *t,
            _ => Instant::now(),
        };
        same = Some((read, since));
        since.elapsed()
    };
    loop {
        page::release(d).await;
        match look(d, target, check).await {
            Ok(Look::Holds { said, read }) => {
                looked = true;
                let still = keep(Some(read));
                if !settling || still >= Duration::from_millis(SETTLE_MS) {
                    return ActionOutcome::passed(said);
                }
                holding = Some(said);
            }
            Ok(Look::NotYet { said, by_locator, read }) => {
                looked = true;
                keep(read);
                holding = None;
                last = if by_locator { format!("waited {timeout_ms}ms: {} {said}", target.describe()) } else { said };
            }
            Err(e) if e.is_transient() => {
                looked = true;
                keep(None);
                holding = None;
                last = format!("waited {timeout_ms}ms: {} {e}", target.describe());
            }
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return harness(e),
        }
        if Instant::now() >= deadline {
            // Still changing when time ran out: the last read decides.
            if let Some(said) = holding {
                return ActionOutcome::passed(said);
            }
            return if looked { ActionOutcome::failed(last) } else { harness_timeout(timeout_ms, &target.describe()) };
        }
        let poll = if settling { poll_ms.max(SETTLE_POLL_MS) } else { poll_ms };
        let left = deadline.saturating_duration_since(Instant::now());
        d.idle(Duration::from_millis(poll).min(left)).await;
    }
}
