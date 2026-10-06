//! What `expect_download` checks in a file the browser downloaded: its
//! name, and for a spreadsheet, CSV, text or PDF file what is in it.
//!
//! It reads a file that is already on disk and says, in one plain
//! sentence, what it found; the only other thing it does is write why a
//! PDF could not be read to the app log. Finding the download (which one, whether it
//! finished) is the runner's; keeping it under a safe name is
//! `browser::downloads`. Nothing here sends a file anywhere.
//!
//! - xlsx and xls are read with `calamine`, read only.
//! - csv is read with `csv`, the delimiter taken from its first line, the
//!   text UTF-8 (with or without a byte-order mark) or else Windows-1252.
//! - txt is UTF-8, or else Windows-1252.
//! - pdf is read with `pdf-extract`, page by page, compared ignoring case
//!   with every run of whitespace as one space.
//!
//! A sentence never quotes more than `MAX_QUOTED_CHARS` characters of what
//! the file holds, and never the path the file was kept at.

use calamine::{open_workbook, Data, Reader, Xls, Xlsx};
use std::path::Path;

use crate::test_files::human_size;

/// The largest file whose content is read for a check. Its name can still
/// be checked when it is bigger.
pub const MAX_CHECK_BYTES: u64 = 50 * 1024 * 1024;

/// The most characters of a cell's or a file's text one sentence quotes.
const MAX_QUOTED_CHARS: usize = 200;

/// Excel's own limits: 1,048,576 rows and 16,384 columns (XFD).
const MAX_ROWS: u32 = 1_048_576;
const MAX_COLS: u32 = 16_384;

/// One `expect_download`'s checks on the file, once it has been found.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadCheck {
    /// The whole file name, exactly or with `*` for any run of characters,
    /// ignoring case.
    pub name: String,
    /// The sheet to read, by name; the first when absent. A CSV has one
    /// sheet and ignores it.
    pub sheet: Option<String>,
    /// The first row.
    pub headers: Option<HeaderCheck>,
    pub cells: Vec<CellCheck>,
    /// Text each of which must appear in a csv or txt file.
    pub contains_text: Vec<String>,
    /// What a PDF's text must hold.
    pub pdf: Option<PdfCheck>,
}

/// A PDF's checks, in the order they are made.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PdfCheck {
    /// Phrases each of which must appear somewhere in the PDF.
    pub contains: Vec<String>,
    pub pages: Option<PageCount>,
    pub on_page: Vec<OnPage>,
}

/// How many pages the PDF has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageCount {
    Equals(u32),
    AtLeast(u32),
    AtMost(u32),
}

/// Phrases one page must hold. `page` counts from 1; `-1` is the last.
#[derive(Debug, Clone, PartialEq)]
pub struct OnPage {
    pub page: i32,
    pub contains: Vec<String>,
}

/// Every failure to get a PDF's text says this; why is in the app log.
pub const PDF_UNREADABLE: &str = "the PDF's text could not be read";

/// The first row, compared trimmed.
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderCheck {
    /// Exactly these, in this order.
    Exact(Vec<String>),
    /// Each of these, in any order, among others.
    Contains(Vec<String>),
}

/// One cell, by an A1-style reference, compared on its displayed text,
/// trimmed: equal to `text`, or containing it when `contains`.
#[derive(Debug, Clone, PartialEq)]
pub struct CellCheck {
    pub r#ref: String,
    pub text: String,
    pub contains: bool,
}

/// Whether `name` is the file `pattern` asks for: the whole name, ignoring
/// case, where `*` stands for any run of characters (none included) and
/// every other character stands for itself.
pub fn name_matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().flat_map(char::to_lowercase).collect();
    let n: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    // The classic two-pointer wildcard match: on a mismatch, go back to the
    // last star and let it take one more character.
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if pi < p.len() && p[pi] == n[ni] {
            pi += 1;
            ni += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// An A1-style reference as a zero-based (row, column): `A1` is (0, 0),
/// `AA10` is (9, 26). Letters then digits, nothing else, inside Excel's
/// own limits; anything else is `None`.
pub fn parse_a1(r: &str) -> Option<(u32, u32)> {
    let letters = r.chars().take_while(char::is_ascii_alphabetic).count();
    let (col_part, row_part) = r.split_at(letters);
    if col_part.is_empty() || col_part.len() > 3 || row_part.is_empty() || row_part.len() > 7 {
        return None;
    }
    if !row_part.bytes().all(|b| b.is_ascii_digit()) || row_part.starts_with('0') {
        return None;
    }
    let col = col_part.bytes().fold(0u32, |acc, b| acc * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1));
    let row: u32 = row_part.parse().ok()?;
    if col > MAX_COLS || row > MAX_ROWS {
        return None;
    }
    Some((row - 1, col - 1))
}

/// Checks the downloaded file at `path`, shown to the person as
/// `shown_name`. `Ok` is the passed sentence (`downloaded "<name>"
/// (<size>)` and a clause per check); `Err` is the first failure's.
pub fn check_file(path: &Path, shown_name: &str, check: &DownloadCheck) -> Result<String, String> {
    if !name_matches(&check.name, shown_name) {
        return Err(format!("got \"{shown_name}\", expected a file named \"{}\"", check.name));
    }
    let size = std::fs::metadata(path)
        .map_err(|e| format!("\"{shown_name}\" could not be read: {}", clip(&e.to_string())))?
        .len();
    let mut sentence = match saved_as(path, shown_name) {
        Some(on_disk) => format!("downloaded \"{shown_name}\" (saved as \"{on_disk}\", {})", human_size(size)),
        None => format!("downloaded \"{shown_name}\" ({})", human_size(size)),
    };

    let wants_sheet = check.headers.is_some() || !check.cells.is_empty();
    let wants_text = !check.contains_text.is_empty();
    if !wants_sheet && !wants_text && check.pdf.is_none() {
        return Ok(sentence);
    }
    if let Some(pdf) = &check.pdf {
        pdf_size_gate(size)?;
        let pages = pdf_pages(&read_bytes(path, shown_name)?).map_err(|detail| {
            crate::applog::warn(format!("expect_download: the text of \"{shown_name}\" could not be read: {detail}"));
            PDF_UNREADABLE.to_string()
        })?;
        sentence.push_str(&check_pdf(&pages, pdf)?);
    }
    if !wants_sheet && !wants_text {
        return Ok(sentence);
    }
    if size > MAX_CHECK_BYTES {
        return Err(format!("\"{shown_name}\" is {}, over the 50 MB that can be checked", human_size(size)));
    }

    let kind = Kind::of(shown_name);
    if wants_sheet {
        let grid = read_grid(path, shown_name, kind, check.sheet.as_deref())?;
        if let Some(headers) = &check.headers {
            check_headers(&grid, headers)?;
            sentence.push_str(", headers match");
        }
        for c in &check.cells {
            sentence.push_str(", ");
            sentence.push_str(&check_cell(&grid, c)?);
        }
    }
    if wants_text {
        if !matches!(kind, Kind::Csv | Kind::Txt) {
            return Err(format!("\"{shown_name}\" could not be read as text: it is not a .csv or .txt file"));
        }
        let bytes = read_bytes(path, shown_name)?;
        let text = decode(&bytes);
        for want in &check.contains_text {
            if !text.contains(want.as_str()) {
                return Err(format!("\"{shown_name}\" does not contain \"{want}\""));
            }
            sentence.push_str(&format!(", it contains \"{want}\""));
        }
    }
    Ok(sentence)
}

/// The name the file is kept under, when it is not the one it was matched
/// by: a second download of one name is numbered (`x (2).csv`), and a
/// person opening the run's folder looks for that one. Names that differ
/// only in case are one name to Windows.
fn saved_as(path: &Path, shown_name: &str) -> Option<String> {
    let on_disk = path.file_name()?.to_string_lossy().into_owned();
    (on_disk.to_lowercase() != shown_name.to_lowercase()).then_some(on_disk)
}

/// What a file is, by its name's extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Xlsx,
    Xls,
    Csv,
    Txt,
    Other,
}

impl Kind {
    fn of(name: &str) -> Kind {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "xlsx" => Kind::Xlsx,
            "xls" => Kind::Xls,
            "csv" => Kind::Csv,
            "txt" => Kind::Txt,
            _ => Kind::Other,
        }
    }
}

/// A sheet's cells as text, by zero-based row and column. Rows may be
/// ragged; a cell past the data is empty.
struct Grid {
    rows: Vec<Vec<String>>,
}

impl Grid {
    fn text(&self, row: u32, col: u32) -> &str {
        self.rows
            .get(row as usize)
            .and_then(|r| r.get(col as usize))
            .map(|s| s.trim())
            .unwrap_or("")
    }

    /// The first row, trimmed, without the empty cells at its end.
    fn headers(&self) -> Vec<String> {
        let mut first: Vec<String> =
            self.rows.first().map(|r| r.iter().map(|s| s.trim().to_string()).collect()).unwrap_or_default();
        while first.last().is_some_and(|s| s.is_empty()) {
            first.pop();
        }
        first
    }
}

fn not_a_sheet(shown_name: &str, reason: &str) -> String {
    format!("\"{shown_name}\" could not be read as a spreadsheet: {}", clip(reason))
}

fn read_bytes(path: &Path, shown_name: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("\"{shown_name}\" could not be read: {}", clip(&e.to_string())))
}

fn read_grid(path: &Path, shown_name: &str, kind: Kind, sheet: Option<&str>) -> Result<Grid, String> {
    match kind {
        Kind::Xlsx | Kind::Xls => {
            // A damaged workbook must be a sentence, never the end of the
            // run: whatever calamine does with one, a panic included, comes
            // back as "could not be read".
            let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                read_workbook(path, shown_name, kind, sheet)
            }));
            read.unwrap_or_else(|_| Err(not_a_sheet(shown_name, "the file is damaged")))
        }
        Kind::Csv => read_csv(&read_bytes(path, shown_name)?, shown_name),
        Kind::Txt | Kind::Other => Err(not_a_sheet(shown_name, "it is not an .xlsx, .xls or .csv file")),
    }
}

fn read_workbook(path: &Path, shown_name: &str, kind: Kind, sheet: Option<&str>) -> Result<Grid, String> {
    if kind == Kind::Xls {
        let book: Xls<_> =
            open_workbook(path).map_err(|e: calamine::XlsError| not_a_sheet(shown_name, &e.to_string()))?;
        return read_book(book, shown_name, sheet);
    }
    let book: Xlsx<_> =
        open_workbook(path).map_err(|e: calamine::XlsxError| not_a_sheet(shown_name, &e.to_string()))?;
    read_book(book, shown_name, sheet)
}

fn read_book<R, B>(mut book: B, shown_name: &str, sheet: Option<&str>) -> Result<Grid, String>
where
    R: std::io::Read + std::io::Seek,
    B: Reader<R>,
    B::Error: std::fmt::Display,
{
    let names = book.sheet_names();
    let chosen = match sheet.map(str::trim).filter(|s| !s.is_empty()) {
        Some(want) => names.iter().find(|n| n.trim().to_lowercase() == want.to_lowercase()).cloned().ok_or_else(|| {
            format!("sheet \"{want}\" is not in \"{shown_name}\" (it has: {})", clip(&names.join(", ")))
        })?,
        None => names.first().cloned().ok_or_else(|| not_a_sheet(shown_name, "it has no sheets"))?,
    };
    let range = book.worksheet_range(&chosen).map_err(|e| not_a_sheet(shown_name, &e.to_string()))?;
    // The range starts at the first cell holding something, which need not
    // be A1; the grid is laid out from A1 so a reference means what it says.
    let mut rows: Vec<Vec<String>> = Vec::new();
    if let (Some((r0, c0)), Some((r1, c1))) = (range.start(), range.end()) {
        for r in r0..=r1 {
            let mut row = vec![String::new(); c0 as usize];
            for c in c0..=c1 {
                row.push(range.get_value((r, c)).map(shown).unwrap_or_default());
            }
            let at = r as usize;
            if rows.len() <= at {
                rows.resize(at + 1, Vec::new());
            }
            rows[at] = row;
        }
    }
    Ok(Grid { rows })
}

/// A cell's text as a spreadsheet shows it, as near as a reader without
/// the cell's number format can: `1001`, not `1001.0`, and `TRUE`.
fn shown(d: &Data) -> String {
    match d {
        Data::Bool(true) => "TRUE".to_string(),
        Data::Bool(false) => "FALSE".to_string(),
        other => other.to_string(),
    }
}

fn read_csv(bytes: &[u8], shown_name: &str) -> Result<Grid, String> {
    let text = decode(bytes);
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter(&text))
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| not_a_sheet(shown_name, &e.to_string()))?;
        rows.push(record.iter().map(str::to_string).collect());
    }
    Ok(Grid { rows })
}

/// Comma, semicolon or tab: whichever the first record has most of
/// outside double quotes, comma when it has none (a single column) or on a
/// tie. A quoted `"Smith, John"` is text, not two columns, and a quoted
/// line break does not end the first record.
fn delimiter(text: &str) -> u8 {
    let (mut commas, mut semicolons, mut tabs) = (0usize, 0usize, 0usize);
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\n' if !quoted => break,
            ',' if !quoted => commas += 1,
            ';' if !quoted => semicolons += 1,
            '\t' if !quoted => tabs += 1,
            _ => {}
        }
    }
    let mut best = (b',', commas);
    for (d, n) in [(b';', semicolons), (b'\t', tabs)] {
        if n > best.1 {
            best = (d, n);
        }
    }
    best.0
}

/// The file's text: UTF-8 without its byte-order mark, or else
/// Windows-1252 - what a spreadsheet saved on a European Windows machine
/// writes.
fn decode(bytes: &[u8]) -> String {
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(body) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::WINDOWS_1252.decode_without_bom_handling(body).0.into_owned(),
    }
}

fn check_headers(grid: &Grid, want: &HeaderCheck) -> Result<(), String> {
    let actual = grid.headers();
    let (ok, expected) = match want {
        HeaderCheck::Exact(w) => {
            let w: Vec<&str> = w.iter().map(|s| s.trim()).collect();
            (actual.iter().map(String::as_str).eq(w.iter().copied()), list(w.iter().copied()))
        }
        HeaderCheck::Contains(w) => (
            w.iter().all(|h| actual.iter().any(|a| a == h.trim())),
            format!("{} among them", list(w.iter().map(|s| s.trim()))),
        ),
    };
    if ok {
        return Ok(());
    }
    Err(format!("headers are {}, expected {expected}", clip(&list(actual.iter().map(String::as_str)))))
}

/// `["a", "b"]`.
fn list<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let quoted: Vec<String> = items.map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

/// The passed clause for one cell, or the failure sentence.
fn check_cell(grid: &Grid, c: &CellCheck) -> Result<String, String> {
    let r = c.r#ref.trim();
    let Some((row, col)) = parse_a1(r) else {
        return Err(format!("\"{r}\" is not a cell reference like B2"));
    };
    let actual = grid.text(row, col);
    let want = c.text.trim();
    if c.contains {
        if actual.contains(want) {
            return Ok(format!("{r} contains \"{want}\""));
        }
        return Err(format!("{r} is \"{}\", expected it to contain \"{want}\"", clip(actual)));
    }
    if actual == want {
        return Ok(format!("{r} is \"{want}\""));
    }
    Err(format!("{r} is \"{}\", expected \"{want}\"", clip(actual)))
}

/// At most `MAX_QUOTED_CHARS` characters of `s`, with `...` when it was
/// longer.
fn clip(s: &str) -> String {
    match s.char_indices().nth(MAX_QUOTED_CHARS) {
        Some((at, _)) => format!("{}...", &s[..at]),
        None => s.to_string(),
    }
}

/// A PDF over the 50 MB that can be read is refused before it is read.
pub fn pdf_size_gate(size: u64) -> Result<(), String> {
    if size > MAX_CHECK_BYTES {
        return Err("the PDF is larger than 50 MB".to_string());
    }
    Ok(())
}

/// Text as the PDF checks compare it: lowercase, with every run of
/// whitespace one space, and none at either end.
pub fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// A PDF's text, page by page, normalised. `Err` says why it could not be
/// read, for the log only: a damaged file, one that needs a password, or
/// one with no text on any page (a scanned image). Whatever the reader
/// does with a hostile file, a panic included, comes back as an `Err`.
pub fn pdf_pages(bytes: &[u8]) -> Result<Vec<String>, String> {
    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| extract_pages(bytes)));
    let pages = match read {
        Ok(r) => r?,
        Err(panic) => {
            let why = panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| panic.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "the reader stopped".to_string());
            return Err(format!("the PDF reader failed: {}", clip(&why)));
        }
    };
    if pages.iter().all(String::is_empty) {
        return Err(format!("no text on any of its {} pages", pages.len()));
    }
    Ok(pages)
}

fn extract_pages(bytes: &[u8]) -> Result<Vec<String>, String> {
    use pdf_extract::{output_doc_page, Document, PlainTextOutput};
    let mut doc = Document::load_mem(bytes).map_err(|e| clip(&e.to_string()))?;
    if doc.is_encrypted() {
        // A file locked only against changes opens with no password, as in
        // any viewer; one that needs a password to open cannot be read.
        doc.decrypt("").map_err(|e| format!("it is encrypted: {}", clip(&e.to_string())))?;
    }
    let numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    let mut pages = Vec::with_capacity(numbers.len());
    for n in numbers {
        let mut text = String::new();
        {
            let mut out = PlainTextOutput::new(&mut text);
            output_doc_page(&doc, &mut out, n).map_err(|e| format!("page {n}: {}", clip(&e.to_string())))?;
        }
        pages.push(normalise(&text));
    }
    Ok(pages)
}

/// The passed clauses for a PDF's checks, or the first failure.
fn check_pdf(pages: &[String], check: &PdfCheck) -> Result<String, String> {
    let mut clauses = String::new();
    let whole = pages.join(" ");
    for want in &check.contains {
        let want = want.trim();
        if !whole.contains(&normalise(want)) {
            return Err(format!("the PDF does not contain \"{want}\""));
        }
        clauses.push_str(&format!(", the PDF contains \"{want}\""));
    }
    if let Some(count) = check.pages {
        let n = pages.len() as u64;
        let (ok, expected) = match count {
            PageCount::Equals(e) => (n == u64::from(e), e.to_string()),
            PageCount::AtLeast(e) => (n >= u64::from(e), format!("at least {e}")),
            PageCount::AtMost(e) => (n <= u64::from(e), format!("at most {e}")),
        };
        if !ok {
            return Err(format!("the PDF has {}, not {expected}", page_word(n)));
        }
        clauses.push_str(&format!(", the PDF has {}", page_word(n)));
    }
    for on in &check.on_page {
        let at = match on.page {
            -1 => pages.len(),
            p if p >= 1 => usize::try_from(p).unwrap_or(usize::MAX),
            p => return Err(format!("the PDF has no page {p}")),
        };
        let Some(text) = at.checked_sub(1).and_then(|i| pages.get(i)) else {
            return Err(format!("the PDF has no page {}", on.page));
        };
        for want in &on.contains {
            let want = want.trim();
            if !text.contains(&normalise(want)) {
                return Err(format!("page {at} of the PDF does not contain \"{want}\""));
            }
            clauses.push_str(&format!(", page {at} of the PDF contains \"{want}\""));
        }
    }
    Ok(clauses)
}

/// `1 page`, `3 pages`.
fn page_word(n: u64) -> String {
    if n == 1 {
        "1 page".to_string()
    } else {
        format!("{n} pages")
    }
}
