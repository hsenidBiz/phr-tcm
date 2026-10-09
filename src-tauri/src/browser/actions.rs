//! The typed steps a script is made of, and what each does to the browser.
//!
//! Every action answers `{ ok, detail }`. `detail` is written for the
//! human watching, because in this runner the person - not the machine -
//! decides the verdict. An action that cannot tell what happened says so
//! rather than guessing.

use super::dialogs;
use super::table;
use super::drag;
use super::keys;
use super::cdp::{browser_silent, no_tab, tab_taken, CdpError, Driver, MAIN_CANNOT_CLOSE, MAIN_TAB};
use super::expect::{self, Check};
use super::input::{self, Blocked};
use super::locator::{resolve_explained, Target};
use super::page;
use super::timing::Timing;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

fn ok_status() -> u16 {
    200
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Navigate { url: String },
    /// Runs a saved component with its inputs. Expanded into the
    /// component's own steps before a script runs.
    UseComponent {
        component: String,
        #[serde(default)]
        #[specta(type = BTreeMap<String, specta_typescript::Unknown>)]
        inputs: serde_json::Map<String, serde_json::Value>,
    },
    Click { selector: Target },
    Fill { selector: Target, value: String },
    WaitFor { selector: Target, timeout_ms: u32 },
    CheckText { value: String },
    CheckUrl { contains: String },
    ExpectVisible {
        selector: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectHidden {
        selector: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectText {
        selector: Target,
        equals: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectContainsText {
        selector: Target,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectCount {
        selector: Target,
        equals: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    ExpectAttribute {
        selector: Target,
        name: String,
        equals: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    /// Change who is signed in. Carried out by the runner (it needs the
    /// tester's accounts and the project's recipe), not by this driver.
    SignIn { account: String },
    /// Put a file from the project's Test files into the page: into the
    /// file input `selector` names, or through the file chooser that
    /// clicking it opens. `file` is a test file's NAME, never a path. The
    /// runner finds the file (it knows the project) and hands this driver
    /// its path - see `upload_in`.
    Upload { selector: Target, file: String },
    /// Check a request the page made since this script step began: that
    /// one matching `url_contains` (and `method`, when given) finished,
    /// answered `status`, and, with `json`, carried those fields. Carried
    /// out by the runner, which alone holds the network record.
    ExpectResponse {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        method: Option<String>,
        url_contains: String,
        #[serde(default = "ok_status")]
        status: u16,
        // See `ApiExpect::json` for why the TypeScript type is overridden.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[specta(type = Option<specta_typescript::Unknown>)]
        json: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
        /// Any other key the script carried - see `Stray`.
        #[serde(flatten)]
        #[specta(skip)]
        stray: Stray,
    },
    /// Ask the current site a GET question, sent by the page itself, and
    /// check the answer. `path` is a path on the page's own site, never an
    /// address. Carried out by the runner.
    ApiRequest {
        path: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        query: BTreeMap<String, String>,
        #[serde(default)]
        expect: ApiExpect,
        /// Any other key the script carried - see `Stray`.
        #[serde(flatten)]
        #[specta(skip)]
        stray: Stray,
    },
    /// Something that may or may not show up (a consent banner, an "Another
    /// active session" prompt): if `selector` becomes visible within
    /// `within_ms` (`WHEN_VISIBLE_MS` when left out), the `then` actions
    /// run; otherwise the step passes with `NOT_SHOWN`. `then` holds plain
    /// actions only - see `validate`. The sign-in recipe's own `WhenVisible`
    /// is the same step; this one is a case script's. Carried out by the
    /// runner, which alone can place an `upload` inside it.
    WhenVisible {
        selector: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within_ms: Option<u32>,
        then: Vec<Action>,
    },
    /// Reload the page, as a person pressing F5 would, and wait for it to
    /// load again. Where it lands is the application's business: a page
    /// that sends a reload elsewhere (PeoplesHR's wizard goes back to the
    /// home page) is followed by `return_to_area`, not by an address.
    Reload,
    /// End the session the way a timeout would look to the site: the
    /// browser forgets what it holds for the page's own site, so the next
    /// request arrives with no session at all (see `expire_session`). The
    /// server's own record is not touched - what a script then checks is
    /// how the application treats a request whose session is gone.
    ExpireSession,
    /// Take the browser to an area by its recorded menu path: the case's
    /// own area (the trip a run makes before step 1) when `area` is left
    /// out, else the recorded area of that name - so a case can look at
    /// another area partway through and come back. The one way about for a
    /// project that refuses `navigate`. Carried out by the runner, which
    /// alone knows the areas' routes.
    ReturnToArea {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        area: Option<String>,
    },
    /// Press one key on whatever has the focus, as a keyboard would: Tab
    /// and Shift+Tab move the focus, Enter and Space activate, Escape
    /// closes. One of `PRESS_KEYS`, with any of Ctrl, Shift, Alt and Meta
    /// held for it (`Ctrl+ArrowUp`, see `keys`), nothing else - a script
    /// that needs a field's text uses `fill`. `times` (1 to 50) presses the
    /// same combination that many times.
    PressKey {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        times: Option<u8>,
    },
    /// Pick `from` up and drop it on `to` - before it, after it, or onto
    /// it (`position`, onto when left out) - as a mouse would, within
    /// `within_ms` (`drag::DRAG_WAIT_MS` when left out). Serves pages that
    /// drag with mouse or pointer events and pages that use the browser's
    /// own drag and drop alike (see `drag`).
    Drag {
        from: Target,
        to: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<DropAt>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within_ms: Option<u32>,
    },
    /// The focus is on this element, or on something inside it (a card
    /// whose own button has it counts, as `:focus-within` would).
    ExpectFocused {
        selector: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    /// Check the file this step downloaded: the first download that started
    /// during the step, once it completes within `within_ms`
    /// (`DOWNLOAD_WAIT_MS` when left out). `name` is the whole file name,
    /// ignoring case, with `*` for any run of characters; the other keys
    /// read what is in it (`autorun::downloads::check_file`). Carried out by
    /// the runner, which alone knows where the step began.
    ExpectDownload {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within_ms: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sheet: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headers: Option<HeadersSpec>,
        /// `None` when left out; an empty list is refused, not ignored.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cells: Option<Vec<CellSpec>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contains_text: Option<Vec<String>>,
        /// What a PDF's text must hold - for a name ending in .pdf only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pdf: Option<PdfSpec>,
        /// Any other key the script carried - see `Stray`.
        #[serde(flatten)]
        #[specta(skip)]
        stray: Stray,
    },
    /// Wait for a tab the page opened since the previous step began (a
    /// `target=_blank` link, `window.open`), within `within_ms`
    /// (`TAB_WAIT_MS` when left out), and call it `name`. With
    /// `url_contains`, its address must contain that text. It does not
    /// switch to it: `switch_tab` does.
    ExpectTab {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url_contains: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within_ms: Option<u32>,
    },
    /// Open a new tab called `name` at `url`, in the same signed-in
    /// session, and switch to it. `url` follows `navigate`'s rules.
    OpenTab { name: String, url: String },
    /// Make the tab called `name` the current tab, and bring it to the
    /// front. Every later action acts in it.
    SwitchTab { name: String },
    /// Close the tab called `name`. If it was the current tab, `main` is
    /// current again. `main` is never closed.
    CloseTab { name: String },
    /// The page closes the tab called `name` itself (a print preview that
    /// closes after printing), within `within_ms` (`TAB_WAIT_MS` when left
    /// out).
    ExpectTabClosed {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within_ms: Option<u32>,
    },
    /// The next browser dialog (`alert`, `confirm`, `prompt`,
    /// `beforeunload`) in any tab: answered as `answer` says, with
    /// `prompt_text` typed into a prompt first, then its message checked -
    /// equal to `text`, or holding `contains` (ignoring case), or anything
    /// when neither is given. Armed when its step starts, so a dialog an
    /// earlier action of the step opens is caught; it waits up to
    /// `within_ms` (`dialogs::DIALOG_WAIT_MS` when left out) once the
    /// step's other actions are done. Carried out by the runner, which arms
    /// a step's expectations; run on its own, it arms itself first.
    ExpectDialog {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contains: Option<String>,
        answer: DialogAnswer,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prompt_text: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within_ms: Option<u32>,
    },
    /// Some row of the table or grid `table` has every one of `cells` (by
    /// column header): each text in its cell, ignoring case - or equal to
    /// it with `exact` (see `table`). Retried like `expect_text`.
    ExpectRow {
        table: Target,
        #[specta(type = BTreeMap<String, String>)]
        cells: table::Cells,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        exact: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    /// No row of `table` has every one of `cells`.
    ExpectNoRow {
        table: Target,
        #[specta(type = BTreeMap<String, String>)]
        cells: table::Cells,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        exact: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    /// The values of `column` in `table` are in `order`, read as text
    /// (when `as` is left out), numbers or dates. Blank cells are passed
    /// over.
    ExpectSorted {
        table: Target,
        column: String,
        order: table::SortOrder,
        #[serde(rename = "as", default, skip_serializing_if = "Option::is_none")]
        sort_as: Option<table::SortAs>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
    /// `table` has `equals` rows, or `at_least`, or `at_most` - exactly
    /// one of the three.
    ExpectRowCount {
        table: Target,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        equals: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at_least: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at_most: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        timeout_ms: Option<u32>,
    },
}

/// How an `expect_dialog` answers: OK, or Cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DialogAnswer {
    Accept,
    Dismiss,
}

/// Where on `to` a `drag` drops: its upper part, its lower part, or its
/// middle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DropAt {
    Before,
    After,
    #[default]
    Onto,
}

impl DropAt {
    /// As a sentence says it: `dragged <from> before <to>`.
    pub fn word(self) -> &'static str {
        match self {
            DropAt::Before => "before",
            DropAt::After => "after",
            DropAt::Onto => "onto",
        }
    }
}

/// How long `expect_tab` and `expect_tab_closed` wait when they name no
/// `within_ms`.
pub const TAB_WAIT_MS: u32 = 10_000;
/// The longest `expect_tab` and `expect_tab_closed` may wait.
pub const TAB_WAIT_MAX_MS: u32 = 60_000;

/// The rule every tab name follows.
pub const TAB_NAME_RULE: &str = "a tab name is 1 to 30 letters, digits, - or _";

/// Is this a name a script may give a tab?
pub fn valid_tab_name(name: &str) -> bool {
    (1..=30).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn check_tab_name(kind: &str, name: &str) -> Result<(), String> {
    if valid_tab_name(name) {
        return Ok(());
    }
    let shown: String = name.chars().take(40).collect();
    Err(format!("{kind}: {TAB_NAME_RULE}, not \"{shown}\""))
}

fn check_tab_wait(kind: &str, within_ms: &Option<u32>) -> Result<(), String> {
    match within_ms {
        Some(0) => Err(WITHIN_MS_ZERO.to_string()),
        Some(ms) if *ms > TAB_WAIT_MAX_MS => Err(format!("{kind} waits at most {TAB_WAIT_MAX_MS} ms, not {ms}")),
        _ => Ok(()),
    }
}

/// A script's address, as a sentence may say it: its path only, with no
/// host, query or fragment.
pub fn path_only(url: &str) -> String {
    let url = url.trim();
    let cut = url.split(['?', '#']).next().unwrap_or("");
    match cut.find("://") {
        Some(i) => {
            let rest = &cut[i + 3..];
            rest.find('/').map_or_else(|| "/".to_string(), |j| rest[j..].to_string())
        }
        None => cut.to_string(),
    }
}

/// An `expect_download`'s first row: `{ "exact": [...] }`, exactly these in
/// this order, or `{ "contains": [...] }`, each of these in any order.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum HeadersSpec {
    Exact(Vec<String>),
    Contains(Vec<String>),
}

/// One cell an `expect_download` reads: `{ "ref": "B2", "text": "...",
/// "match": "exact" | "contains" }`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CellSpec {
    #[serde(rename = "ref")]
    pub at: String,
    pub text: String,
    #[serde(rename = "match", default)]
    pub how: CellMatch,
}

/// One phrase, or a list of them: `"Total"` or `["Total", "Net pay"]`.
/// Kept as the script wrote it, so a file round-trips unchanged.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(untagged)]
pub enum Phrases {
    One(String),
    Many(Vec<String>),
}

impl Phrases {
    pub fn list(&self) -> Vec<String> {
        match self {
            Phrases::One(s) => vec![s.clone()],
            Phrases::Many(v) => v.clone(),
        }
    }
}

/// An `expect_download`'s `pdf` block: phrases the PDF contains, its page
/// count, and phrases on given pages.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PdfSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contains: Option<Phrases>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<PdfPages>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_page: Option<Vec<PdfOnPage>>,
    /// Any other key the block carried - see `Stray`.
    #[serde(flatten)]
    #[specta(skip)]
    pub stray: Stray,
}

/// The PDF's page count: exactly one of the three.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PdfPages {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equals: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_least: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_most: Option<u32>,
}

/// Phrases one page holds. `page` counts from 1, and `-1` is the last.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PdfOnPage {
    pub page: i32,
    pub contains: Phrases,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CellMatch {
    #[default]
    Exact,
    Contains,
}

/// How long an `expect_download` waits when it names no `within_ms`.
pub const DOWNLOAD_WAIT_MS: u32 = 15_000;
/// The longest an `expect_download` may wait.
pub const DOWNLOAD_WAIT_MAX_MS: u32 = 120_000;

/// The file types whose cells an `expect_download` reads, and whose text.
const SHEET_TYPES: (&[&str], &str, &str) = (&[".xlsx", ".xls", ".csv"], ".xlsx, .xls or .csv", "Template*.xlsx");
const TEXT_TYPES: (&[&str], &str, &str) = (&[".csv", ".txt"], ".csv or .txt", "errors*.csv");

/// A key `expect_download` reads only in a file of `types`, refused unless
/// the name ends in one: the type is known before the file arrives, so a
/// saved script never asks to read a workbook as text.
fn needs_type(key: &str, name: &str, (exts, said, example): (&[&str], &str, &str)) -> Result<(), String> {
    let lower = name.trim().to_lowercase();
    if exts.iter().any(|e| lower.ends_with(e)) {
        return Ok(());
    }
    Err(format!(
        "expect_download can check {key} only in a file whose name ends in {said}, and \"{}\" does not - name the file type (such as {example}) so it is known before the file arrives, or check only name and within_ms",
        name.trim()
    ))
}

/// What `validate` says about an `expect_download`.
#[allow(clippy::too_many_arguments)]
fn check_download(
    name: &str,
    within_ms: &Option<u32>,
    sheet: &Option<String>,
    headers: &Option<HeadersSpec>,
    cells: &Option<Vec<CellSpec>>,
    contains_text: &Option<Vec<String>>,
    pdf: &Option<PdfSpec>,
    stray: &Stray,
) -> Result<(), String> {
    if let Some(key) = stray.keys().next() {
        let shown: String = key.chars().take(40).collect();
        return Err(format!(
            "expect_download has no \"{shown}\" - it takes name, within_ms, sheet, headers, cells, contains_text and pdf"
        ));
    }
    if name.trim().is_empty() {
        return Err("expect_download needs a name, such as Template*.xlsx".to_string());
    }
    match within_ms {
        Some(0) => return Err(WITHIN_MS_ZERO.to_string()),
        Some(ms) if *ms > DOWNLOAD_WAIT_MAX_MS => {
            return Err(format!("within_ms is at most {DOWNLOAD_WAIT_MAX_MS} (got {ms})"))
        }
        _ => {}
    }
    if let Some(pdf) = pdf {
        if sheet.is_some() || headers.is_some() || cells.is_some() {
            return Err("a download check is either a PDF check or a spreadsheet check".to_string());
        }
        if !name.trim().to_lowercase().ends_with(".pdf") {
            return Err("pdf checks need a name ending in .pdf".to_string());
        }
        check_pdf_spec(pdf)?;
    }
    if sheet.is_some() {
        needs_type("sheet", name, SHEET_TYPES)?;
    }
    if headers.is_some() {
        needs_type("headers", name, SHEET_TYPES)?;
    }
    if cells.is_some() {
        needs_type("cells", name, SHEET_TYPES)?;
    }
    if contains_text.is_some() {
        needs_type("contains_text", name, TEXT_TYPES)?;
    }
    if let Some(s) = sheet {
        if s.trim().is_empty() {
            return Err("expect_download sheet is empty - leave it out for the first sheet".to_string());
        }
        if headers.is_none() && cells.is_none() {
            return Err(format!(
                "expect_download sheet \"{}\" is read only for headers or cells - add one, or leave sheet out",
                s.trim()
            ));
        }
    }
    if let Some(HeadersSpec::Exact(h) | HeadersSpec::Contains(h)) = headers {
        if h.is_empty() {
            return Err(
                "expect_download headers is an empty list - name at least one header, or leave headers out".to_string()
            );
        }
    }
    if let Some(cells) = cells {
        if cells.is_empty() {
            return Err("expect_download cells is an empty list - give at least one cell, or leave cells out".to_string());
        }
        for (i, c) in cells.iter().enumerate() {
            let n = i + 1;
            if crate::autorun::downloads::parse_a1(c.at.trim()).is_none() {
                return Err(format!("expect_download cells {n}: \"{}\" is not a cell reference like B2", c.at.trim()));
            }
            if c.how == CellMatch::Contains && c.text.trim().is_empty() {
                return Err(format!(
                    "expect_download cells {n}: an empty text with match contains holds for any cell - give the text to find"
                ));
            }
        }
    }
    if let Some(texts) = contains_text {
        if texts.is_empty() {
            return Err(
                "expect_download contains_text is an empty list - give at least one text, or leave contains_text out"
                    .to_string(),
            );
        }
        if let Some(i) = texts.iter().position(|t| t.trim().is_empty()) {
            return Err(format!("expect_download contains_text {} is empty - every file contains nothing", i + 1));
        }
    }
    Ok(())
}

/// What `validate` says about an `expect_download`'s `pdf` block.
fn check_pdf_spec(pdf: &PdfSpec) -> Result<(), String> {
    if let Some(key) = pdf.stray.keys().next() {
        let shown: String = key.chars().take(40).collect();
        return Err(format!("pdf has no \"{shown}\" - it takes contains, pages and on_page"));
    }
    if pdf.contains.is_none() && pdf.pages.is_none() && pdf.on_page.is_none() {
        return Err("pdf is empty - give contains, pages or on_page, or leave pdf out".to_string());
    }
    if let Some(c) = &pdf.contains {
        phrases_given("pdf contains", c)?;
    }
    if let Some(p) = &pdf.pages {
        let given = [p.equals, p.at_least, p.at_most].iter().filter(|n| n.is_some()).count();
        if given != 1 {
            return Err("pdf pages takes exactly one of equals, at_least or at_most".to_string());
        }
        if p.equals == Some(0) || p.at_least == Some(0) || p.at_most == Some(0) {
            return Err("pdf pages counts from 1".to_string());
        }
    }
    if let Some(on) = &pdf.on_page {
        if on.is_empty() {
            return Err("pdf on_page is an empty list - give at least one page, or leave on_page out".to_string());
        }
        for o in on {
            if o.page == 0 || o.page < -1 {
                return Err("on_page: page counts from 1, or -1 for the last page".to_string());
            }
            phrases_given("pdf on_page contains", &o.contains)?;
        }
    }
    Ok(())
}

/// A list of phrases holds at least one, and none is empty.
fn phrases_given(key: &str, phrases: &Phrases) -> Result<(), String> {
    let list = phrases.list();
    if list.is_empty() {
        return Err(format!("{key} is an empty list - give at least one text"));
    }
    if list.iter().any(|t| t.trim().is_empty()) {
        return Err(format!("{key} has an empty text - every PDF contains nothing"));
    }
    Ok(())
}

/// The keys `press_key` may press, as a script names them: name, the DOM
/// `key`, the DOM `code`, the Windows virtual key, and the text a key types
/// (for those that type one). Shift+Tab, once a row of its own, is Tab with
/// Shift held (`keys::parse`), and is pressed with Shift held as before.
pub const PRESS_KEYS: &[(&str, &str, &str, i64, Option<&str>)] = &[
    ("Tab", "Tab", "Tab", 9, None),
    ("Enter", "Enter", "Enter", 13, Some("\r")),
    ("Space", " ", "Space", 32, Some(" ")),
    ("Escape", "Escape", "Escape", 27, None),
    ("ArrowUp", "ArrowUp", "ArrowUp", 38, None),
    ("ArrowDown", "ArrowDown", "ArrowDown", 40, None),
    ("ArrowLeft", "ArrowLeft", "ArrowLeft", 37, None),
    ("ArrowRight", "ArrowRight", "ArrowRight", 39, None),
    ("Home", "Home", "Home", 36, None),
    ("End", "End", "End", 35, None),
];

/// How long a script's `when_visible` waits when it names no `within_ms`.
pub const WHEN_VISIBLE_MS: u32 = 2000;
/// The longest a script's `when_visible` may wait: a step that may not
/// show up costs this much on every run where it does not.
pub const WHEN_VISIBLE_MAX_MS: u32 = 10_000;
/// A `when_visible` whose target never appeared, after the target's words.
pub const NOT_SHOWN: &str = "not shown, skipped";
/// A `when_visible` (recipe or script) told to wait no time at all.
pub const WITHIN_MS_ZERO: &str = "within_ms must be more than 0";
/// A `when_visible` inside another one's `then`.
pub const NESTED_WHEN_VISIBLE: &str = "when_visible \"then\" cannot hold another when_visible";

/// What a script's `when_visible` may hold in `then`: plain actions only.
/// A guarded click is a tidy-up, not an assertion, so no check goes inside
/// one (nothing there could count toward the expected-result floor), and
/// neither does a change of who is signed in.
fn check_guarded(then: &[Action]) -> Result<(), String> {
    if then.is_empty() {
        return Err("when_visible has nothing in \"then\" - say what to do when it shows up".to_string());
    }
    for (i, a) in then.iter().enumerate() {
        if matches!(a, Action::WhenVisible { .. }) {
            return Err(NESTED_WHEN_VISIBLE.to_string());
        }
        if matches!(a, Action::SignIn { .. }) {
            return Err(
                "when_visible \"then\" cannot hold sign_in - change who is signed in as an action of its own".to_string()
            );
        }
        // Both move the whole case - out of its session, or back to its
        // area - which is never a tidy-up that may or may not happen.
        if matches!(
            a,
            Action::ExpireSession | Action::ReturnToArea { .. } | Action::OpenTab { .. } | Action::SwitchTab { .. } | Action::CloseTab { .. }
        ) {
            return Err(format!(
                "when_visible \"then\" cannot hold {} - write it as an action of its own",
                a.kind()
            ));
        }
        if a.is_check() {
            return Err(format!(
                "when_visible \"then\" cannot hold {} - a guarded step tidies up and checks nothing, so put the check after it",
                a.kind()
            ));
        }
        a.validate().map_err(|e| format!("when_visible then {}: {e}", i + 1))?;
    }
    Ok(())
}

/// Keys a script gave one of the two API checks that it does not take,
/// kept only so `validate` can refuse them: an expectation written in the
/// other kind's shape (`api_request` with `status` beside `kind`) would
/// otherwise be dropped without a word, and the step would still count as
/// a check. Empty in every valid action, so never written back out, and
/// not part of the TypeScript type. The other kinds keep ignoring a key
/// they do not know, as scripts saved by older versions rely on.
pub type Stray = BTreeMap<String, Value>;

/// What an `api_request` expects back. Mirrors the API templates'
/// `Expect` (status, then a partial JSON match), and like it refuses a key
/// it does not know: a misspelt `json` must not pass as "answered 200".
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(deny_unknown_fields)]
pub struct ApiExpect {
    #[serde(default = "ok_status")]
    pub status: u16,
    // `serde_json::Value`'s specta mapping pulls in `serde_json::Number`'s
    // i64/u64 variants, which the TypeScript exporter refuses to emit
    // (precision loss) - the same override, and reason, as
    // `api_templates::Expect::json`. The wire format is still real JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = Option<specta_typescript::Unknown>)]
    pub json: Option<Value>,
}

impl Default for ApiExpect {
    fn default() -> Self {
        ApiExpect { status: 200, json: None }
    }
}

/// An `api_request` path that is not a safe path on the page's own site -
/// not repeated, since it can be a whole address.
const UNSAFE_API_PATH: &str =
    "api_request path is not a safe path on this site - give a path such as /api/cycles/42, never an address";

const HTTP_METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// A key an API check does not take, refused with what goes where. The
/// key is the script's own word, shown cut short; its value never is.
fn refuse_stray(kind: &str, stray: &Stray) -> Result<(), String> {
    // The other kind's expectation keys first: they are the likely mistake.
    let sibling: &[&str] = match kind {
        "api_request" => &["status", "json", "method", "url_contains", "timeout_ms"],
        _ => &["expect"],
    };
    let first = sibling.iter().copied().find(|k| stray.contains_key(*k));
    let Some(key) = first.or_else(|| stray.keys().next().map(String::as_str)) else {
        return Ok(());
    };
    let shown: String = key.chars().take(40).collect();
    Err(match (kind, key) {
        ("api_request", "status" | "json") => {
            "api_request takes status and json under \"expect\", not beside \"kind\"".to_string()
        }
        ("api_request", "method" | "url_contains" | "timeout_ms") => {
            format!("api_request takes status and json under \"expect\", and has no \"{shown}\" (that is expect_response's)")
        }
        ("api_request", _) => format!("api_request has no \"{shown}\" - it takes path, query and expect"),
        (_, "expect") => "expect_response takes status and json directly, not under \"expect\"".to_string(),
        _ => format!(
            "expect_response has no \"{shown}\" - it takes method, url_contains, status, json and timeout_ms"
        ),
    })
}

fn check_status(status: u16) -> Result<(), String> {
    if (100..=599).contains(&status) {
        Ok(())
    } else {
        Err(format!("status {status} is not an HTTP status"))
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ActionOutcome {
    pub ok: bool,
    pub detail: String,
    /// A file in the autorun `shots` folder, taken when the action failed.
    /// A name, never a path and never the image: run files stay small, and
    /// the webview cannot ask for anything outside that folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
    /// True when the browser connection failed rather than the page, so
    /// callers do not ask a dead browser for a picture. Process-internal
    /// only: never crosses the IPC boundary and never lands in a saved run
    /// file.
    #[serde(skip)]
    #[specta(skip)]
    pub harness: bool,
    /// The component this action came from, when a `use_component` was
    /// expanded into it. Left out otherwise, so older run files read the
    /// same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
}

impl ActionOutcome {
    pub fn passed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: true, detail: detail.into(), screenshot: None, harness: false, component: None }
    }
    pub fn failed(detail: impl Into<String>) -> Self {
        ActionOutcome { ok: false, detail: detail.into(), screenshot: None, harness: false, component: None }
    }
}

// The words an action's own failures are reported in, beside the ones
// `input` and `expect` name for theirs - read back by `autorun::patterns`.

/// Every harness failure starts with this: the browser, not the page.
pub const BROWSER_SILENT: &str = "the browser did not answer";
/// An action refused before it reached the page (followed by why).
pub const CANNOT_RUN: &str = "this action cannot run: ";
/// `wait_for` ran out: `waited Nms and never saw <target>`.
pub const NEVER_SAW: &str = "ms and never saw ";
/// A navigation the page did not finish in time.
pub const DID_NOT_FINISH_LOADING: &str = " did not finish loading within ";
/// A navigation the browser refused outright.
pub const WOULD_NOT_LOAD: &str = " would not load: ";
/// A navigation outside the recipe's allowed origins.
pub const ALLOWED_ORIGINS: &str = "this project's allowed origins";
/// `check_text` on a page without the words.
pub const PAGE_LACKS: &str = "page does NOT contain ";
/// `check_url` - followed by the address actually showing.
pub const URL_IS: &str = "url is ";
/// What an upload's click opened instead of a file chooser.
pub const FILE_CHOOSER: &str = "file chooser";
/// Appended, in run files from before `dialogs`, when a page raised
/// dialogs during the action. Now ` (a <kind> dialog was accepted: ...)`
/// (`dialogs::accepted`); both are read back by `autorun::patterns`.
pub const DIALOG_NOTE: &str = " (the page showed ";

/// A harness failure, said plainly: the app under test did nothing wrong,
/// the browser connection did.
pub(crate) fn harness(e: CdpError) -> ActionOutcome {
    // A tab rule is the script's, not the browser's: its sentence is the
    // whole failure, and a picture can still be taken.
    if let CdpError::Tab(sentence) = e {
        return ActionOutcome::failed(sentence);
    }
    let mut out = ActionOutcome::failed(format!("{BROWSER_SILENT}: {e}"));
    out.harness = true;
    out
}

/// A wait loop's deadline ran out and not one look ever completed. Shared
/// by `expect`'s loop and `wait_for` below, so their wording can never
/// drift apart; `wait_ready` (which returns a `Blocked`, not an
/// `ActionOutcome`) builds the equivalent `Blocked::Harness` itself from
/// the same `browser_silent` wording.
pub(crate) fn harness_timeout(waited_ms: u64, target: &str) -> ActionOutcome {
    let mut out = ActionOutcome::failed(browser_silent(waited_ms, target));
    out.harness = true;
    out
}

pub(crate) fn blocked(b: Blocked) -> ActionOutcome {
    match b {
        Blocked::Page(why) => ActionOutcome::failed(why),
        Blocked::Harness(why) => {
            let mut out = ActionOutcome::failed(format!("{BROWSER_SILENT}: {why}"));
            out.harness = true;
            out
        }
    }
}

/// The same division `input::blame` draws, for the sites that produce an
/// `ActionOutcome` directly: a refusal came from the PAGE (a navigation
/// landing mid-action yields "Cannot find context with specified id"),
/// anything else is the browser connection. It matters twice over -
/// "the browser did not answer" about a browser that is alive and well is
/// simply wrong, and `harness` also suppresses the failure screenshot.
pub(crate) fn failed_by(e: CdpError) -> ActionOutcome {
    match e {
        CdpError::Protocol { message, .. } => {
            ActionOutcome::failed(format!("{}{message}", input::PAGE_REFUSED))
        }
        other => harness(other),
    }
}

/// Where an authored `navigate` may go. `None` is no restriction, which is
/// what a project with no sign-in recipe gets.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Policy {
    pub allowed_origins: Option<Vec<String>>,
}

impl Policy {
    pub fn open() -> Self {
        Policy { allowed_origins: None }
    }
    pub fn only(origins: Vec<String>) -> Self {
        Policy { allowed_origins: Some(origins.into_iter().map(|o| o.to_ascii_lowercase()).collect()) }
    }
    pub fn allows(&self, url: &str) -> bool {
        match &self.allowed_origins {
            None => true,
            Some(list) => crate::autorun::recipe::origin_of(url).is_some_and(|o| list.contains(&o)),
        }
    }
}

/// An address the browser can be sent to. `file://` is allowed on
/// purpose: the live fixture is a local file and this tab is a
/// development-only one. Which of these an authored `navigate` may
/// actually use is `Policy`'s job, not this function's.
fn is_page_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("file://")
}

/// Does this start with a URI scheme (RFC 3986: a letter, then letters,
/// digits, `+`, `-` or `.`, then `:`)? Anything that does NOT is a
/// relative reference, which the page resolves at run time.
fn has_scheme(url: &str) -> bool {
    let mut chars = url.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    for c in chars {
        if c == ':' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return false;
        }
    }
    false
}

/// What `navigate` accepts when a script is SAVED: a page address, or a
/// relative reference like `/dashboard`. Scripts have always been able to
/// write the latter, and because saving validates every action, refusing
/// one here would refuse a whole bundle for containing a single such
/// script. Anything carrying another scheme (`javascript:`, `data:`,
/// `about:`, `chrome:`) is still refused.
fn is_navigable(url: &str) -> bool {
    let u = url.trim();
    !u.is_empty() && (is_page_url(u) || !has_scheme(u))
}

impl Action {
    /// What can be known to be wrong before a browser is involved. Run on
    /// save, so a bad script is refused where it is written, and again on
    /// execute, so nothing invalid reaches the page.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Action::Navigate { url } if !is_navigable(url) => Err(not_an_address("navigate", url)),
            Action::Navigate { .. } => Ok(()),
            Action::UseComponent { component, .. } if component.trim().is_empty() => {
                Err("use_component names no component".to_string())
            }
            Action::UseComponent { .. } => Ok(()),
            Action::Click { selector }
            | Action::Fill { selector, .. }
            | Action::WaitFor { selector, .. }
            | Action::ExpectVisible { selector, .. }
            | Action::ExpectHidden { selector, .. }
            | Action::ExpectText { selector, .. }
            | Action::ExpectCount { selector, .. } => selector.validate(),
            Action::ExpectContainsText { selector, value, .. } => {
                if value.trim().is_empty() {
                    return Err("expect_contains_text has an empty value - everything contains nothing".to_string());
                }
                selector.validate()
            }
            Action::ExpectAttribute { selector, name, .. } => {
                if name.trim().is_empty() {
                    return Err("expect_attribute has an empty name".to_string());
                }
                selector.validate()
            }
            Action::CheckText { value } if value.trim().is_empty() => {
                Err("check_text has an empty value".to_string())
            }
            Action::CheckUrl { contains } if contains.trim().is_empty() => {
                Err("check_url has an empty value".to_string())
            }
            Action::CheckText { .. } | Action::CheckUrl { .. } => Ok(()),
            Action::SignIn { account } if !crate::autorun::accounts::valid_key(account) => {
                Err(format!("sign_in names \"{account}\", which is not a usable account key"))
            }
            Action::SignIn { .. } => Ok(()),
            Action::Upload { file, .. } if !crate::test_files::valid_test_file_name(file) => {
                Err(format!("upload: {}", crate::test_files::bad_name(file)))
            }
            Action::Upload { selector, .. } => selector.validate(),
            Action::ExpectResponse { method, url_contains, status, stray, .. } => {
                refuse_stray("expect_response", stray)?;
                if url_contains.trim().is_empty() {
                    return Err("expect_response needs url_contains".to_string());
                }
                // The record holds no host: a whole address could never match.
                if url_contains.contains("://") {
                    return Err("url_contains is a path fragment, not a full address".to_string());
                }
                if let Some(m) = method {
                    if !HTTP_METHODS.iter().any(|k| k.eq_ignore_ascii_case(m.trim())) {
                        return Err(format!("expect_response method \"{m}\" is not an HTTP method"));
                    }
                }
                check_status(*status)
            }
            Action::ApiRequest { path, expect, stray, .. } => {
                refuse_stray("api_request", stray)?;
                // A refusal repeats only a safe path on this site: never
                // what follows a `?` (it can be a token), never an address
                // (it names a host).
                let before = path.find(['?', '#']).map(|i| &path[..i]);
                if !crate::api_templates::is_safe_relative_path(before.unwrap_or(path)) {
                    return Err(UNSAFE_API_PATH.to_string());
                }
                // The query goes only through `query`.
                if let Some(before) = before {
                    return Err(format!(
                        "api_request path \"{before}\" must not contain ? or # - put the query in \"query\""
                    ));
                }
                check_status(expect.status)
            }
            Action::WhenVisible { selector, within_ms, then } => {
                match within_ms {
                    Some(0) => return Err(WITHIN_MS_ZERO.to_string()),
                    Some(ms) if *ms > WHEN_VISIBLE_MAX_MS => {
                        return Err(format!("when_visible waits at most {WHEN_VISIBLE_MAX_MS} ms, not {ms}"))
                    }
                    _ => {}
                }
                selector.validate()?;
                check_guarded(then)
            }
            Action::Reload | Action::ExpireSession | Action::ReturnToArea { .. } => Ok(()),
            Action::PressKey { key, times } => keys::check(key, *times),
            Action::ExpectRow { table, cells, exact, .. } | Action::ExpectNoRow { table, cells, exact, .. } => {
                let kind = self.kind();
                if cells.0.is_empty() {
                    return Err(format!("{kind} needs at least one cell"));
                }
                if cells.0.iter().any(|(c, _)| c.trim().is_empty()) {
                    return Err(format!("{kind} has a cell with an empty column name"));
                }
                if let Some((c, _)) = cells.0.iter().find(|(_, t)| t.trim().is_empty() && !exact) {
                    return Err(format!(
                        "{kind}: an empty text for \"{}\" holds for any cell - give the text, or set exact for an empty cell",
                        c.trim()
                    ));
                }
                table.validate()
            }
            Action::ExpectSorted { table, column, sort_as, .. } => {
                if column.trim().is_empty() {
                    return Err("expect_sorted needs a column".to_string());
                }
                if let Some(table::SortAs::Format { date }) = sort_as {
                    if !table::is_date_format(date) {
                        return Err(format!(
                            "expect_sorted: \"{date}\" is not a date format - use letters such as dd/MM/yyyy, MM/dd/yyyy, yyyy-MM-dd or d MMM yyyy"
                        ));
                    }
                }
                table.validate()
            }
            Action::ExpectRowCount { table, equals, at_least, at_most, .. } => {
                table::row_count(*equals, *at_least, *at_most)?;
                table.validate()
            }
            Action::ExpectDialog { text, contains, answer, prompt_text, within_ms } => {
                if text.is_some() && contains.is_some() {
                    return Err(dialogs::TEXT_OR_CONTAINS.to_string());
                }
                if contains.as_deref().is_some_and(|c| c.trim().is_empty()) {
                    return Err(
                        "expect_dialog has an empty contains - every message contains nothing; leave it out to take any message"
                            .to_string(),
                    );
                }
                if prompt_text.is_some() && *answer == DialogAnswer::Dismiss {
                    return Err(dialogs::PROMPT_NEEDS_ACCEPT.to_string());
                }
                match within_ms {
                    Some(0) => Err(WITHIN_MS_ZERO.to_string()),
                    Some(ms) if *ms > dialogs::DIALOG_WAIT_MAX_MS => {
                        Err(format!("expect_dialog waits at most {} ms, not {ms}", dialogs::DIALOG_WAIT_MAX_MS))
                    }
                    _ => Ok(()),
                }
            }
            Action::Drag { from, to, within_ms, .. } => {
                from.validate().map_err(|e| format!("drag from: {e}"))?;
                to.validate().map_err(|e| format!("drag to: {e}"))?;
                match within_ms {
                    Some(0) => Err(WITHIN_MS_ZERO.to_string()),
                    Some(ms) if *ms > drag::DRAG_WAIT_MAX_MS => {
                        Err(format!("drag waits at most {} ms, not {ms}", drag::DRAG_WAIT_MAX_MS))
                    }
                    _ => Ok(()),
                }
            }
            Action::ExpectFocused { selector, .. } => selector.validate(),
            Action::ExpectDownload { name, within_ms, sheet, headers, cells, contains_text, pdf, stray } => {
                check_download(name, within_ms, sheet, headers, cells, contains_text, pdf, stray)
            }
            Action::ExpectTab { name, url_contains, within_ms } => {
                check_tab_name("expect_tab", name)?;
                if name == MAIN_TAB {
                    return Err(tab_taken(name));
                }
                if url_contains.as_deref().is_some_and(|u| u.trim().is_empty()) {
                    return Err("expect_tab has an empty url_contains - leave it out to take the new tab wherever it is".to_string());
                }
                check_tab_wait("expect_tab", within_ms)
            }
            Action::OpenTab { name, url } => {
                check_tab_name("open_tab", name)?;
                if name == MAIN_TAB {
                    return Err(tab_taken(name));
                }
                if !is_navigable(url) {
                    return Err(not_an_address("open_tab", url));
                }
                Ok(())
            }
            Action::SwitchTab { name } => check_tab_name("switch_tab", name),
            Action::CloseTab { name } => {
                check_tab_name("close_tab", name)?;
                if name == MAIN_TAB {
                    return Err(MAIN_CANNOT_CLOSE.to_string());
                }
                Ok(())
            }
            Action::ExpectTabClosed { name, within_ms } => {
                check_tab_name("expect_tab_closed", name)?;
                if name == MAIN_TAB {
                    return Err(MAIN_CANNOT_CLOSE.to_string());
                }
                check_tab_wait("expect_tab_closed", within_ms)
            }
        }
    }

    /// One of the five tab actions: the ones that still run when the
    /// current tab has closed by itself, because they do not act in it.
    pub fn is_tab_action(&self) -> bool {
        matches!(
            self,
            Action::ExpectTab { .. }
                | Action::OpenTab { .. }
                | Action::SwitchTab { .. }
                | Action::CloseTab { .. }
                | Action::ExpectTabClosed { .. }
        )
    }

    /// The script's own word for this action: `"click"`, `"when_visible"`.
    pub fn kind(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v["kind"].as_str().map(str::to_string))
            .unwrap_or_default()
    }

    /// This action, then every action a `when_visible` guards - for a rule
    /// that has to see an action wherever in a step it is written.
    pub fn each(&self) -> Vec<&Action> {
        match self {
            Action::WhenVisible { then, .. } => std::iter::once(self).chain(then.iter()).collect(),
            _ => vec![self],
        }
    }

    /// Every target this action points at, wherever it is written: inside a
    /// `when_visible`, both ends of a `drag`, a table action's `table`.
    /// Matched without a catch-all, so a new action kind will not compile
    /// until it is classified here.
    pub fn targets(&self) -> Vec<&Target> {
        match self {
            Action::Click { selector }
            | Action::Fill { selector, .. }
            | Action::WaitFor { selector, .. }
            | Action::ExpectVisible { selector, .. }
            | Action::ExpectHidden { selector, .. }
            | Action::ExpectText { selector, .. }
            | Action::ExpectContainsText { selector, .. }
            | Action::ExpectCount { selector, .. }
            | Action::ExpectAttribute { selector, .. }
            | Action::Upload { selector, .. }
            | Action::ExpectFocused { selector, .. } => vec![selector],
            Action::WhenVisible { selector, then, .. } => {
                std::iter::once(selector).chain(then.iter().flat_map(Action::targets)).collect()
            }
            Action::Drag { from, to, .. } => vec![from, to],
            Action::ExpectRow { table, .. }
            | Action::ExpectNoRow { table, .. }
            | Action::ExpectSorted { table, .. }
            | Action::ExpectRowCount { table, .. } => vec![table],
            Action::Navigate { .. }
            | Action::UseComponent { .. }
            | Action::CheckText { .. }
            | Action::CheckUrl { .. }
            | Action::SignIn { .. }
            | Action::ExpectResponse { .. }
            | Action::ApiRequest { .. }
            | Action::Reload
            | Action::ExpireSession
            | Action::ReturnToArea { .. }
            | Action::PressKey { .. }
            | Action::ExpectDownload { .. }
            | Action::ExpectTab { .. }
            | Action::OpenTab { .. }
            | Action::SwitchTab { .. }
            | Action::CloseTab { .. }
            | Action::ExpectTabClosed { .. }
            | Action::ExpectDialog { .. } => Vec::new(),
        }
    }

    /// `targets`, to change in place: every target this action points at,
    /// in the same order.
    pub fn targets_mut(&mut self) -> Vec<&mut Target> {
        match self {
            Action::Click { selector }
            | Action::Fill { selector, .. }
            | Action::WaitFor { selector, .. }
            | Action::ExpectVisible { selector, .. }
            | Action::ExpectHidden { selector, .. }
            | Action::ExpectText { selector, .. }
            | Action::ExpectContainsText { selector, .. }
            | Action::ExpectCount { selector, .. }
            | Action::ExpectAttribute { selector, .. }
            | Action::Upload { selector, .. }
            | Action::ExpectFocused { selector, .. } => vec![selector],
            Action::WhenVisible { selector, then, .. } => {
                std::iter::once(selector).chain(then.iter_mut().flat_map(Action::targets_mut)).collect()
            }
            Action::Drag { from, to, .. } => vec![from, to],
            Action::ExpectRow { table, .. }
            | Action::ExpectNoRow { table, .. }
            | Action::ExpectSorted { table, .. }
            | Action::ExpectRowCount { table, .. } => vec![table],
            Action::Navigate { .. }
            | Action::UseComponent { .. }
            | Action::CheckText { .. }
            | Action::CheckUrl { .. }
            | Action::SignIn { .. }
            | Action::ExpectResponse { .. }
            | Action::ApiRequest { .. }
            | Action::Reload
            | Action::ExpireSession
            | Action::ReturnToArea { .. }
            | Action::PressKey { .. }
            | Action::ExpectDownload { .. }
            | Action::ExpectTab { .. }
            | Action::OpenTab { .. }
            | Action::SwitchTab { .. }
            | Action::CloseTab { .. }
            | Action::ExpectTabClosed { .. }
            | Action::ExpectDialog { .. } => Vec::new(),
        }
    }

    /// The text a `fill` types; `None` for any other action.
    pub fn typed_value(&self) -> Option<&str> {
        match self {
            Action::Fill { value, .. } => Some(value),
            _ => None,
        }
    }

    /// The area a `return_to_area` names, trimmed; `None` for any other
    /// action, and for one that names none or a blank one - both go to the
    /// case's own area, as a script's blank `area` means its default one.
    pub fn area_named(&self) -> Option<&str> {
        match self {
            Action::ReturnToArea { area } => area.as_deref().map(str::trim).filter(|a| !a.is_empty()),
            _ => None,
        }
    }

    /// Does this action JUDGE the page (rather than drive it or wait for it)?
    pub fn is_check(&self) -> bool {
        matches!(
            self,
            Action::CheckText { .. }
                | Action::CheckUrl { .. }
                | Action::ExpectVisible { .. }
                | Action::ExpectHidden { .. }
                | Action::ExpectText { .. }
                | Action::ExpectContainsText { .. }
                | Action::ExpectCount { .. }
                | Action::ExpectAttribute { .. }
                | Action::ExpectFocused { .. }
                | Action::ExpectResponse { .. }
                | Action::ApiRequest { .. }
                | Action::ExpectDownload { .. }
                | Action::ExpectTab { .. }
                | Action::ExpectTabClosed { .. }
                | Action::ExpectDialog { .. }
                | Action::ExpectRow { .. }
                | Action::ExpectNoRow { .. }
                | Action::ExpectSorted { .. }
                | Action::ExpectRowCount { .. }
        )
    }
}

/// What `navigate`, and `open_tab` after it, say about an address that is
/// not one a browser can be sent to.
fn not_an_address(kind: &str, url: &str) -> String {
    format!("{kind} needs an http, https or file address, not {url:?}")
}

/// `this` is the element about to be touched.
pub const HIGHLIGHT_JS: &str = r#"function() {
  const prev = this.style.outline;
  this.style.outline = '3px solid #7c5cff';
  setTimeout(() => { this.style.outline = prev; }, 1200);
  return true;
}"#;

/// `this` is the document. Argument: the words to look for.
///
/// "Anywhere on the page" includes what same-origin frames show, at any
/// depth: a page's own `innerText` stops at each iframe. A frame no one can
/// see is skipped (a hidden frame's document is not rendered, and its
/// `innerText` would hand back every word in it), and so is a frame from
/// another site, whose document cannot be read (null, or a throw).
pub const CHECK_TEXT_JS: &str = r#"function(want) {
  const needle = String(want).toLowerCase();
  const has = (doc) => {
    const hay = (doc.body ? doc.body.innerText : '') || '';
    if (hay.toLowerCase().includes(needle)) return true;
    for (const f of doc.querySelectorAll('iframe, frame')) {
      const r = f.getBoundingClientRect();
      if (!f.checkVisibility({ visibilityProperty: true }) || r.width <= 0 || r.height <= 0) continue;
      let inner = null;
      try { inner = f.contentDocument; } catch (e) { inner = null; }
      if (inner && has(inner)) return true;
    }
    return false;
  };
  return has(this);
}"#;

/// `this` is the document. Argument: the relative reference. Resolved in
/// the PAGE against its own address, with the script's value passed as an
/// argument and never concatenated into this source.
pub const RESOLVE_URL_JS: &str = r#"function(rel) { return new URL(rel, location.href).href; }"#;

async fn point_and_pause<D: Driver>(
    d: &mut D,
    ready: &input::Ready,
    timing: &Timing,
) -> Result<(), CdpError> {
    page::call_value(d, &ready.handle, HIGHLIGHT_JS, &[]).await?;
    if timing.highlight_ms > 0 {
        d.idle(Duration::from_millis(timing.highlight_ms)).await;
    }
    Ok(())
}

/// A page address as given, or a relative reference made absolute against
/// the page's own address. What comes back has to be an address a browser
/// can be sent to in its own right: `new URL("javascript:x", base)` keeps
/// that scheme, so the check is applied again to the RESULT.
async fn absolute<D: Driver>(d: &mut D, url: &str) -> Result<String, ActionOutcome> {
    if is_page_url(url) {
        return Ok(url.to_string());
    }
    let doc = page::document(d).await.map_err(failed_by)?;
    let resolved = page::call_value(d, &doc, RESOLVE_URL_JS, &[json!(url)])
        .await
        .map_err(failed_by)?;
    let resolved = resolved.as_str().unwrap_or("").to_string();
    if !is_page_url(&resolved) {
        return Err(ActionOutcome::failed(format!(
            "navigate needs an http, https or file address, not {resolved:?}"
        )));
    }
    Ok(resolved)
}

/// A script's address made absolute and checked against `policy`: where
/// a `navigate` or an `open_tab` may go, or the outcome that says why not.
async fn reachable<D: Driver>(d: &mut D, url: &str, policy: &Policy) -> Result<String, ActionOutcome> {
    let url = absolute(d, url).await?;
    if !policy.allows(&url) {
        // `origin_of` returning `None` here (rather than an origin outside
        // the list) means the address itself cannot be trusted to go
        // where it reads as - naming a made-up origin for it would be
        // worse than not naming one.
        let detail = match crate::autorun::recipe::origin_of(&url) {
            Some(origin) => format!(
                "{origin} is not one of {ALLOWED_ORIGINS} - add it to the sign-in recipe if the test really goes there"
            ),
            None => format!("this address is not one that can be checked against {ALLOWED_ORIGINS} - it does not read as a usable http, https or file address"),
        };
        return Err(ActionOutcome::failed(detail));
    }
    Ok(url)
}

async fn navigate<D: Driver>(d: &mut D, url: &str, timing: &Timing, policy: &Policy) -> ActionOutcome {
    let url = match reachable(d, url, policy).await {
        Ok(u) => u,
        Err(out) => return out,
    };
    let url = url.as_str();
    // Older lifecycle events would satisfy the wait below before this
    // page has even started.
    d.forget_events();
    let reply = match d.call("Page.navigate", json!({ "url": url })).await {
        Ok(r) => r,
        Err(e) => return failed_by(e),
    };
    if let Some(err) = reply["errorText"].as_str() {
        return ActionOutcome::failed(format!("{url}{WOULD_NOT_LOAD}{err}"));
    }
    // No loaderId means the same document (a #fragment): nothing loads.
    let Some(loader_id) = reply["loaderId"].as_str().map(str::to_string) else {
        return ActionOutcome::passed(format!("moved to {url}"));
    };
    let frame_id = reply["frameId"].as_str().map(str::to_string);
    let deadline = Instant::now() + Duration::from_millis(timing.nav_ms);
    let timed_out = || {
        ActionOutcome::failed(format!("{url}{DID_NOT_FINISH_LOADING}{}ms", timing.nav_ms))
    };
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        // The `frameId`/`loaderId` filter can only be applied here: the
        // driver hands back events by method name alone, so a lifecycle
        // event for a stale navigation or a sub-frame is still received
        // and has to be told apart from this navigation's own "load".
        let ev = match d.wait_event("Page.lifecycleEvent", remaining).await {
            Ok(ev) => ev,
            Err(CdpError::Timeout { .. }) => return timed_out(),
            Err(e) => return failed_by(e),
        };
        let is_this_navigation = ev.params["loaderId"].as_str() == Some(loader_id.as_str())
            && ev.params["name"].as_str() == Some("load")
            && frame_id.as_deref().map_or(true, |f| ev.params["frameId"].as_str() == Some(f));
        if is_this_navigation {
            return ActionOutcome::passed(format!("loaded {url}"));
        }
        if Instant::now() >= deadline {
            return timed_out();
        }
    }
}

/// The deadline is pushed down into the driver so no single protocol call
/// can outlive this wait's budget, and cleared on EVERY path out - which
/// is why the loop is a separate function rather than an early `return`
/// away from a `set_deadline(None)`.
async fn wait_for<D: Driver>(d: &mut D, target: &Target, timeout_ms: u32, timing: &Timing) -> ActionOutcome {
    let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
    d.set_deadline(Some(deadline));
    let out = keep_waiting(d, target, timeout_ms, timing, deadline).await;
    d.set_deadline(None);
    out
}

async fn keep_waiting<D: Driver>(
    d: &mut D,
    target: &Target,
    timeout_ms: u32,
    timing: &Timing,
    deadline: Instant,
) -> ActionOutcome {
    // A frame the chain could not enter is the reason it never appeared, and
    // says so instead of "never saw".
    let mut frame: Option<String> = None;
    let gave_up = |frame: &Option<String>| match frame {
        Some(why) => ActionOutcome::failed(format!("waited {timeout_ms}ms: {} {why}", target.describe())),
        None => ActionOutcome::failed(format!("waited {timeout_ms}{NEVER_SAW}{}", target.describe())),
    };
    // Whether any call has actually come back - a page that is merely slow
    // to show the element is not the same failure as a browser that has
    // stopped answering, and the two must not be reported the same way.
    let mut looked = false;
    loop {
        page::release(d).await;
        match resolve_explained(d, target).await {
            Ok(found) if !found.handles.is_empty() => {
                return ActionOutcome::passed(format!("found {}", target.describe()));
            }
            Ok(found) => {
                looked = true;
                frame = found.unreachable_frame;
            }
            // The page refusing mid-navigation is the page answering, just
            // between two documents - it counts as a completed look.
            Err(e) if e.is_transient() => looked = true,
            // No new information about the page: the browser did not
            // answer THIS call. Keep going; only the deadline decides
            // whether that is the wait ending or the browser's silence.
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return harness(e),
        }
        if Instant::now() >= deadline {
            return if looked { gave_up(&frame) } else { harness_timeout(u64::from(timeout_ms), &target.describe()) };
        }
        d.idle(Duration::from_millis(timing.poll_ms)).await;
    }
}

fn wait(own: &Option<u32>, timing: &Timing) -> u64 {
    own.map(u64::from).unwrap_or(timing.expect_ms)
}

async fn run<D: Driver>(d: &mut D, action: &Action, timing: &Timing, policy: &Policy) -> ActionOutcome {
    match action {
        Action::Navigate { url } => navigate(d, url.trim(), timing, policy).await,
        Action::Click { selector } => {
            let ready = match input::wait_ready(d, selector, false, timing).await {
                Ok(r) => r,
                Err(b) => return blocked(b),
            };
            if let Err(e) = point_and_pause(d, &ready, timing).await {
                return failed_by(e);
            }
            match input::click(d, &ready).await {
                Ok(()) => ActionOutcome::passed(format!("clicked {}", selector.describe())),
                Err(b) => blocked(b),
            }
        }
        Action::Fill { selector, value } => {
            let ready = match input::wait_ready(d, selector, true, timing).await {
                Ok(r) => r,
                Err(b) => return blocked(b),
            };
            if let Err(e) = point_and_pause(d, &ready, timing).await {
                return failed_by(e);
            }
            match input::fill(d, &ready, value).await {
                Ok(()) => ActionOutcome::passed(format!("filled {}", selector.describe())),
                Err(b) => blocked(b),
            }
        }
        Action::WaitFor { selector, timeout_ms } => wait_for(d, selector, *timeout_ms, timing).await,
        Action::CheckText { value } => {
            let doc = match page::document(d).await {
                Ok(h) => h,
                Err(e) => return failed_by(e),
            };
            match page::call_value(d, &doc, CHECK_TEXT_JS, &[json!(value)]).await {
                Ok(v) if v.as_bool().unwrap_or(false) => {
                    ActionOutcome::passed(format!("page contains {value}"))
                }
                Ok(_) => ActionOutcome::failed(format!("{PAGE_LACKS}{value}")),
                Err(e) => failed_by(e),
            }
        }
        Action::CheckUrl { contains } => match page::eval_value(d, "location.href").await {
            Ok(v) => {
                let href = v.as_str().unwrap_or("");
                let detail = format!("{URL_IS}{href}");
                if href.contains(contains.as_str()) {
                    ActionOutcome::passed(detail)
                } else {
                    ActionOutcome::failed(detail)
                }
            }
            Err(e) => failed_by(e),
        },
        Action::ExpectVisible { selector, timeout_ms } => {
            expect::expect(d, selector, Check::Visible, wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectHidden { selector, timeout_ms } => {
            expect::expect(d, selector, Check::Hidden, wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectText { selector, equals, timeout_ms } => {
            expect::expect(d, selector, Check::Text(equals), wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectContainsText { selector, value, timeout_ms } => {
            expect::expect(d, selector, Check::ContainsText(value), wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectCount { selector, equals, timeout_ms } => {
            expect::expect(d, selector, Check::Count(*equals), wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectAttribute { selector, name, equals, timeout_ms } => {
            expect::expect(d, selector, Check::Attribute { name, equals }, wait(timeout_ms, timing), timing.poll_ms).await
        }
        // The runner intercepts `sign_in` before it ever reaches this
        // executor (it alone has the tester's accounts and the project's
        // recipe). Reaching here means a caller forgot to.
        Action::SignIn { .. } => ActionOutcome::failed("sign_in is carried out by the runner"),
        // The same for `upload`: only the runner knows the project, and so
        // where its Test files are. It calls `upload_in` with the path.
        Action::Upload { .. } => ActionOutcome::failed("upload is carried out by the runner"),
        // The runner holds the network record and runs the page's own
        // requests; this driver has neither.
        Action::ExpectResponse { .. } => ActionOutcome::failed("expect_response is carried out by the runner"),
        Action::ApiRequest { .. } => ActionOutcome::failed("api_request is carried out by the runner"),
        // Its `then` may hold an `upload`, which only the runner can place.
        Action::WhenVisible { .. } => ActionOutcome::failed("when_visible is carried out by the runner"),
        Action::UseComponent { .. } => ActionOutcome::failed("components are expanded before they run"),
        Action::Reload => reload(d, timing).await,
        Action::ExpireSession => expire_session(d).await,
        Action::PressKey { key, times } => press_key(d, key, times.unwrap_or(1), timing).await,
        Action::ExpectRow { table, cells, exact, timeout_ms } => {
            let check = table::TableCheck::Row { cells: &cells.0, exact: *exact };
            table::expect_table(d, table, check, wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectNoRow { table, cells, exact, timeout_ms } => {
            let check = table::TableCheck::NoRow { cells: &cells.0, exact: *exact };
            table::expect_table(d, table, check, wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectSorted { table, column, order, sort_as, timeout_ms } => {
            let text = table::SortAs::Kind(table::SortKind::Text);
            let check = table::TableCheck::Sorted { column, order: *order, sort_as: sort_as.as_ref().unwrap_or(&text) };
            table::expect_table(d, table, check, wait(timeout_ms, timing), timing.poll_ms).await
        }
        Action::ExpectRowCount { table, equals, at_least, at_most, timeout_ms } => match table::row_count(*equals, *at_least, *at_most) {
            Ok(want) => table::expect_table(d, table, table::TableCheck::Count(want), wait(timeout_ms, timing), timing.poll_ms).await,
            Err(why) => ActionOutcome::failed(why),
        },
        // Run on its own (a try, a recipe), it arms itself: it can only
        // catch a dialog that opens from now on.
        Action::ExpectDialog { .. } => {
            let Some(want) = expectation(action) else {
                return ActionOutcome::failed("expect_dialog could not be read");
            };
            let plan = dialogs::plan_of(STANDALONE_DIALOG, want.answer, &want.prompt_text.map(str::to_string));
            if let Some(book) = d.dialog_book() {
                book.take_seen();
                book.arm(vec![plan]);
            }
            let out = dialogs::judge(d, STANDALONE_DIALOG, &want, timing).await;
            if let Some(book) = d.dialog_book() {
                book.disarm();
            }
            out
        }
        Action::Drag { from, to, position, within_ms } => {
            let within = within_ms.unwrap_or(drag::DRAG_WAIT_MS);
            drag::drag(d, from, to, position.unwrap_or_default(), within, timing).await
        }
        Action::ExpectFocused { selector, timeout_ms } => {
            expect::expect(d, selector, Check::Focused, wait(timeout_ms, timing), timing.poll_ms).await
        }
        // Only the runner knows the case's area and the recipe's home.
        Action::ReturnToArea { .. } => ActionOutcome::failed("return_to_area is carried out by the runner"),
        // Only the runner knows where the step began, and so which
        // download is the step's.
        Action::ExpectDownload { .. } => ActionOutcome::failed("expect_download is carried out by the runner"),
        Action::ExpectTab { name, url_contains, within_ms } => {
            let within = Duration::from_millis(u64::from(within_ms.unwrap_or(TAB_WAIT_MS)));
            match d.expect_tab(name, url_contains.as_deref().map(str::trim), within).await {
                Ok(address) => ActionOutcome::passed(format!("a new tab opened at {}; it is called \"{name}\"", path_only(&address))),
                Err(e) => failed_by(e),
            }
        }
        Action::OpenTab { name, url } => open_tab(d, name, url.trim(), timing, policy).await,
        Action::SwitchTab { name } => match d.switch_tab(name).await {
            Ok(()) => ActionOutcome::passed(format!("switched to the \"{name}\" tab")),
            Err(e) => failed_by(e),
        },
        Action::CloseTab { name } => match d.close_tab(name).await {
            Ok(true) => ActionOutcome::passed(format!("closed the \"{name}\" tab; {MAIN_TAB} is the current tab now")),
            Ok(false) => ActionOutcome::passed(format!("closed the \"{name}\" tab")),
            Err(e) => failed_by(e),
        },
        Action::ExpectTabClosed { name, within_ms } => {
            let within = Duration::from_millis(u64::from(within_ms.unwrap_or(TAB_WAIT_MS)));
            match d.expect_tab_closed(name, within).await {
                Ok(()) => ActionOutcome::passed(format!("the \"{name}\" tab closed")),
                Err(e) => failed_by(e),
            }
        }
    }
}

/// `open_tab`: the address is made absolute in the tab the step is in and
/// held to `navigate`'s rules first, so a refused one opens nothing. Then a
/// blank tab is opened and set up (guarded first in a no-save case), made
/// current, and sent there the way `navigate` sends a page.
async fn open_tab<D: Driver>(d: &mut D, name: &str, url: &str, timing: &Timing, policy: &Policy) -> ActionOutcome {
    let url = match reachable(d, url, policy).await {
        Ok(u) => u,
        Err(out) => return out,
    };
    if let Err(e) = d.open_tab(name).await {
        return failed_by(e);
    }
    let loaded = navigate(d, &url, timing, policy).await;
    if !loaded.ok {
        return loaded;
    }
    ActionOutcome::passed(format!("opened the \"{name}\" tab at {}", path_only(&url)))
}

/// What a reload that never finished loading says, after the address.
pub const RELOAD_DID_NOT_FINISH: &str = " did not finish loading after the reload within ";

/// `reload`: the main frame loaded afresh, as F5 does. Lifecycle events are
/// matched to the MAIN frame only - a sub-frame's "load" is not the page's -
/// and older ones are forgotten first, so the wait is for this reload's own.
/// A "Leave site?" prompt the reload raises is answered by the driver and
/// shown in the outcome like any other dialog.
async fn reload<D: Driver>(d: &mut D, timing: &Timing) -> ActionOutcome {
    let before = page::eval_value(d, "location.href").await.ok();
    let before = before.as_ref().and_then(|v| v.as_str()).unwrap_or("the page").to_string();
    let tree = match d.call("Page.getFrameTree", json!({})).await {
        Ok(t) => t,
        Err(e) => return failed_by(e),
    };
    let main = tree["frameTree"]["frame"]["id"].as_str().map(str::to_string);
    d.forget_events();
    if let Err(e) = d.call("Page.reload", json!({ "ignoreCache": false })).await {
        return failed_by(e);
    }
    let deadline = Instant::now() + Duration::from_millis(timing.nav_ms);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let ev = match d.wait_event("Page.lifecycleEvent", remaining).await {
            Ok(ev) => ev,
            Err(CdpError::Timeout { .. }) => {
                return ActionOutcome::failed(format!("{before}{RELOAD_DID_NOT_FINISH}{}ms", timing.nav_ms))
            }
            Err(e) => return failed_by(e),
        };
        let is_main = main.as_deref().map_or(true, |m| ev.params["frameId"].as_str() == Some(m));
        if is_main && ev.params["name"].as_str() == Some("load") {
            break;
        }
        if Instant::now() >= deadline {
            return ActionOutcome::failed(format!("{before}{RELOAD_DID_NOT_FINISH}{}ms", timing.nav_ms));
        }
    }
    // Where it landed, which may not be where it was: a page that sends a
    // reload home says so here, and the script's next action follows on.
    let after = page::eval_value(d, "location.href").await.ok();
    match after.as_ref().and_then(|v| v.as_str()) {
        Some(now) if now != before => ActionOutcome::passed(format!("reloaded {before}; the page is now {now}")),
        _ => ActionOutcome::passed(format!("reloaded {before}")),
    }
}

/// `this` is the document. Where the focus is, in words: the deepest
/// focused element, followed into same-origin frames and open shadow roots,
/// as its tag, id and accessible-ish name. Empty when nothing but the page
/// itself has it. Only for a person to read - never matched against.
pub const FOCUS_NOW_JS: &str = r#"function() {
  let doc = this, a = doc.activeElement;
  for (;;) {
    if (a && a.shadowRoot && a.shadowRoot.activeElement) { a = a.shadowRoot.activeElement; continue; }
    let inner = null;
    if (a && (a.tagName === 'IFRAME' || a.tagName === 'FRAME')) { try { inner = a.contentDocument; } catch (e) { inner = null; } }
    if (inner && inner.activeElement) { doc = inner; a = inner.activeElement; continue; }
    break;
  }
  if (!a || a === doc.body || a === doc.documentElement) return '';
  const words = (a.getAttribute('aria-label') || a.innerText || a.value || '').replace(/\s+/g, ' ').trim().slice(0, 60);
  return a.tagName.toLowerCase() + (a.id ? '#' + a.id : '') + (words ? ' "' + words + '"' : '');
}"#;

/// The focus, said for an outcome: `the focus is on button "Save"`.
pub(crate) async fn focus_now<D: Driver>(d: &mut D) -> String {
    let doc = match page::document(d).await {
        Ok(h) => h,
        Err(_) => return "the focus could not be read".to_string(),
    };
    match page::call_value(d, &doc, FOCUS_NOW_JS, &[]).await {
        Ok(v) => match v.as_str() {
            Some(s) if !s.is_empty() => format!("the focus is on {s}"),
            _ => "nothing on the page has the focus".to_string(),
        },
        Err(_) => "the focus could not be read".to_string(),
    }
}

/// `press_key`: the combination down and up (`keys`), `times` times, sent
/// to the page as a keyboard sends it, so its default does what a person's
/// would - Tab moves the focus, Enter submits, Space presses a button. Says
/// where the focus went after.
async fn press_key<D: Driver>(d: &mut D, key: &str, times: u8, timing: &Timing) -> ActionOutcome {
    let combo = match keys::parse(key) {
        Ok(c) => c,
        Err(why) => return ActionOutcome::failed(why),
    };
    for _ in 0..times {
        if let Err(e) = combo.press(d).await {
            return failed_by(e);
        }
    }
    // A moment for the page to move the focus before saying where it is.
    d.idle(Duration::from_millis(timing.poll_ms.min(200))).await;
    let pressed = if times > 1 { format!("{} {times} times", combo.name()) } else { combo.name() };
    ActionOutcome::passed(format!("pressed {pressed}; {}", focus_now(d).await))
}

/// What `expire_session` says when the site has no cookies to drop.
pub const NO_SESSION_TO_END: &str =
    "there was no session to end: the browser holds no cookies for this site - is anyone signed in?";

/// `expire_session`: every cookie the browser would send to the current
/// page's address, dropped - session and sign-in cookies included, HttpOnly
/// ones too (the protocol sees them; the page could not). Only the count is
/// said, never a cookie's name or value: a cookie is a credential.
async fn expire_session<D: Driver>(d: &mut D) -> ActionOutcome {
    let href = match page::eval_value(d, "location.href").await {
        Ok(v) => v.as_str().unwrap_or("").to_string(),
        Err(e) => return failed_by(e),
    };
    if !is_page_url(&href) || href.starts_with("file:") {
        return ActionOutcome::failed(format!("expire_session needs a page from a website, not {href:?}"));
    }
    let site = crate::autorun::recipe::origin_of(&href).unwrap_or_else(|| "this site".to_string());
    let reply = match d.call("Network.getCookies", json!({ "urls": [href] })).await {
        Ok(r) => r,
        Err(e) => return failed_by(e),
    };
    let cookies = reply["cookies"].as_array().cloned().unwrap_or_default();
    if cookies.is_empty() {
        return ActionOutcome::failed(NO_SESSION_TO_END);
    }
    for c in &cookies {
        let params = json!({
            "name": c["name"],
            "domain": c["domain"],
            "path": c["path"],
        });
        if let Err(e) = d.call("Network.deleteCookies", params).await {
            return failed_by(e);
        }
    }
    ActionOutcome::passed(format!(
        "ended the session: dropped {} cookie(s) for {site} - the site sees no session from its next request",
        cookies.len()
    ))
}

/// The least a `when_visible` waits, whatever its `within_ms` says. One
/// protocol call may take `cdp::MIN_CALL_TIMEOUT` (250 ms) even at the edge
/// of a budget, so a shorter wait on a slow page could end before a single
/// look had completed - which reads as the browser having stopped
/// answering, a harness failure, for a page that was only slow.
pub const WHEN_VISIBLE_FLOOR_MS: u32 = 500;

/// Did `target` become visible within `within_ms` (never less than
/// `WHEN_VISIBLE_FLOOR_MS`)? The one look a `when_visible` takes, recipe or
/// script. Any visible match counts, two or more included: the guarded
/// actions then run and an ambiguous click says "matched N" itself. `Err`
/// is the browser failing to answer, as the outcome to report.
pub async fn shows_up<D: Driver>(
    d: &mut D,
    target: &Target,
    within_ms: u32,
    timing: &Timing,
) -> Result<bool, ActionOutcome> {
    let wait = u64::from(within_ms.max(WHEN_VISIBLE_FLOOR_MS));
    let seen = expect::expect(d, target, Check::Shown, wait, timing.poll_ms).await;
    if seen.harness {
        return Err(seen);
    }
    Ok(seen.ok)
}

/// Is `target` showing right now? One look, no waiting - how the prompts a
/// signed-in page may show are watched together (`autorun::signin`), and
/// how a sign-in prompt is checked once the login form is already up.
/// `Ok(None)` is a look the browser did not finish in time: no answer
/// either way, for the caller's own deadline to weigh. `Err` is the
/// browser failing outright, as the outcome to report. A page that
/// refused mid-navigation is between two documents: not showing.
pub async fn shown_now<D: Driver>(d: &mut D, target: &Target) -> Result<Option<bool>, ActionOutcome> {
    page::release(d).await;
    match expect::look(d, target, &Check::Shown).await {
        Ok(found) => Ok(Some(found.is_ok())),
        Err(e) if e.is_transient() => Ok(Some(false)),
        Err(CdpError::Timeout { .. }) => Ok(None),
        Err(e) => Err(harness(e)),
    }
}

/// `this` is the element. Is it a file input, and may it be used?
pub const FILE_INPUT_JS: &str = r#"function() {
  return {
    file: this instanceof HTMLInputElement && this.type === 'file',
    disabled: !!this.disabled || !!this.closest('fieldset[disabled]'),
  };
}"#;

/// What `upload` says when a click opened no file chooser.
pub fn no_chooser(target: &str) -> String {
    format!(
        "clicking {target} did not open a {FILE_CHOOSER} - point upload at the page's file input or the button that opens it"
    )
}

/// Puts the file at `file` into the page through `selector`, and says so
/// with `shown` (the file's name and size, as the person reads them):
/// `uploaded "cv.pdf" (12.0 KB) to <target>`.
///
/// The ONE element the selector names is found first, by the same rules a
/// click or a fill finds theirs (the only match, waited for up to
/// `action_ms`). A file input gets the file directly
/// (`DOM.setFileInputFiles`, which raises the input's own `input` and
/// `change` events). Anything else - the button a page draws over its
/// hidden input - is clicked the way `click` clicks, with the browser told
/// to hand the file chooser to this driver rather than show it
/// (`Page.setInterceptFileChooserDialog`); the chooser's input then gets
/// the file. That interception is switched off again on every way out.
///
/// The caller has already checked the file exists and is within the cap,
/// before anything here touches the page. The dialogs a page raised are
/// reported the way `execute_in` reports them.
pub async fn upload_in<D: Driver>(
    d: &mut D,
    selector: &Target,
    file: &std::path::Path,
    shown: &str,
    timing: &Timing,
) -> ActionOutcome {
    let path = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf()).to_string_lossy().into_owned();
    let mut out = upload(d, selector, &path, shown, timing).await;
    append_dialogs(d, &mut out);
    out
}

async fn upload<D: Driver>(d: &mut D, selector: &Target, path: &str, shown: &str, timing: &Timing) -> ActionOutcome {
    let handle = match find_one(d, selector, timing).await {
        Ok(h) => h,
        Err(out) => return out,
    };
    let kind = match page::call_value(d, &handle, FILE_INPUT_JS, &[]).await {
        Ok(v) => v,
        Err(e) => return failed_by(e),
    };
    let done = || ActionOutcome::passed(format!("uploaded {shown} to {}", selector.describe()));
    if kind["file"].as_bool() != Some(true) {
        let (result, switched_off) = through_chooser(d, selector, path, timing).await;
        let mut out = match result {
            Ok(()) => done(),
            Err(out) => out,
        };
        if !switched_off {
            out.detail.push_str(CHOOSER_STILL_HELD);
        }
        return out;
    }
    if kind["disabled"].as_bool() == Some(true) {
        return ActionOutcome::failed(format!("{} {}", selector.describe(), input::DISABLED));
    }
    let backend = match page::backend_id(d, &handle).await {
        Ok(id) => id,
        Err(e) => return failed_by(e),
    };
    match d.call("DOM.setFileInputFiles", json!({ "files": [path], "backendNodeId": backend })).await {
        Ok(_) => done(),
        Err(e) => failed_by(e),
    }
}

/// Added to an upload's outcome when the browser would not stop
/// intercepting file choosers afterwards.
pub const CHOOSER_STILL_HELD: &str =
    " (the browser did not confirm it stopped holding back file choosers - if a file chooser does not open, close the browser and open it again)";

/// Clicks `selector` with the file chooser intercepted, and gives the
/// chooser's input the file. Interception is switched off again whatever
/// happened in between - a page left intercepting would swallow the
/// person's own next chooser. The second value is whether switching it off
/// worked; a failure is logged.
async fn through_chooser<D: Driver>(
    d: &mut D,
    selector: &Target,
    path: &str,
    timing: &Timing,
) -> (Result<(), ActionOutcome>, bool) {
    let out = match d.call("Page.setInterceptFileChooserDialog", json!({ "enabled": true })).await {
        // Switched off below anyway: a browser that refused may still have
        // switched it on.
        Err(e) => Err(failed_by(e)),
        Ok(_) => choose(d, selector, path, timing).await,
    };
    let switched_off = match d.call("Page.setInterceptFileChooserDialog", json!({ "enabled": false })).await {
        Ok(_) => true,
        Err(e) => {
            crate::applog::warn(format!("upload: file chooser interception could not be switched off: {e}"));
            false
        }
    };
    (out, switched_off)
}

async fn choose<D: Driver>(d: &mut D, selector: &Target, path: &str, timing: &Timing) -> Result<(), ActionOutcome> {
    let ready = input::wait_ready(d, selector, false, timing).await.map_err(blocked)?;
    point_and_pause(d, &ready, timing).await.map_err(failed_by)?;
    // A chooser event left over from an earlier upload must not stand in
    // for the one this click opens.
    d.forget_events();
    input::click(d, &ready).await.map_err(blocked)?;
    let ev = match d.wait_event("Page.fileChooserOpened", Duration::from_millis(timing.action_ms)).await {
        Ok(ev) => ev,
        Err(CdpError::Timeout { .. }) => return Err(ActionOutcome::failed(no_chooser(&selector.describe()))),
        Err(e) => return Err(failed_by(e)),
    };
    let Some(backend) = ev.params["backendNodeId"].as_i64() else {
        return Err(ActionOutcome::failed(format!(
            "clicking {} opened a {FILE_CHOOSER} the browser did not tie to a file input - point upload at the page's file input",
            selector.describe()
        )));
    };
    d.call("DOM.setFileInputFiles", json!({ "files": [path], "backendNodeId": backend }))
        .await
        .map_err(failed_by)?;
    Ok(())
}

/// The one element `target` names, waited for up to `action_ms` the way
/// `wait_ready` waits for its element - but not for it to be usable, since
/// a file input is set rather than clicked. Several matches are refused
/// as a click refuses them (a legacy string selector takes the first). The
/// deadline is cleared on every way out.
async fn find_one<D: Driver>(d: &mut D, target: &Target, timing: &Timing) -> Result<page::Handle, ActionOutcome> {
    let deadline = Instant::now() + Duration::from_millis(timing.action_ms);
    d.set_deadline(Some(deadline));
    let out = keep_finding(d, target, timing, deadline).await;
    d.set_deadline(None);
    out
}

async fn keep_finding<D: Driver>(
    d: &mut D,
    target: &Target,
    timing: &Timing,
    deadline: Instant,
) -> Result<page::Handle, ActionOutcome> {
    let mut looked = false;
    let mut last = input::STILL_LOOKING.to_string();
    loop {
        page::release(d).await;
        match resolve_explained(d, target).await {
            Ok(r) if r.handles.len() == 1 || (!r.handles.is_empty() && target.is_legacy()) => {
                return Ok(r.handles.into_iter().next().expect("checked non-empty"));
            }
            Ok(r) => {
                looked = true;
                last = if !r.handles.is_empty() {
                    input::matched_many(r.handles.len())
                } else {
                    r.unreachable_frame.unwrap_or_else(|| input::NOT_FOUND.to_string())
                };
            }
            Err(e) if e.is_transient() => {
                looked = true;
                last = e.to_string();
            }
            Err(CdpError::Timeout { .. }) => {}
            Err(e) => return Err(harness(e)),
        }
        if Instant::now() >= deadline {
            return Err(if looked {
                ActionOutcome::failed(format!("waited {}ms: {} {last}", timing.action_ms, target.describe()))
            } else {
                harness_timeout(timing.action_ms, &target.describe())
            });
        }
        d.idle(Duration::from_millis(timing.poll_ms)).await;
    }
}

/// The id an `expect_dialog` run on its own arms itself under.
const STANDALONE_DIALOG: u32 = u32::MAX;

/// What an `expect_dialog` checks, or `None` for any other action.
pub fn expectation(action: &Action) -> Option<dialogs::Expectation<'_>> {
    match action {
        Action::ExpectDialog { text, contains, answer, prompt_text, within_ms } => Some(dialogs::Expectation {
            text: text.as_deref(),
            contains: contains.as_deref(),
            answer: *answer,
            prompt_text: prompt_text.as_deref(),
            within_ms: within_ms.unwrap_or(dialogs::DIALOG_WAIT_MS),
        }),
        _ => None,
    }
}

/// A dialog nobody expected, accepted while this action ran, is said on
/// it: ` (a confirm dialog was accepted: "Leave?")`. One raised BETWEEN two
/// actions is reported with the NEXT one: the client only reads frames off
/// the socket while a call is in flight, so nothing is noticed until
/// something asks again. (Run files from before said
/// ` (the page showed confirm: Leave? and it was accepted)` - `DIALOG_NOTE`.)
fn append_dialogs<D: Driver>(d: &mut D, out: &mut ActionOutcome) {
    let dialogs = d.take_dialogs();
    if !dialogs.is_empty() {
        let said: Vec<String> = dialogs
            .iter()
            .map(|s| match s.split_once(": ") {
                Some((kind, message)) => dialogs::accepted(kind, message),
                None => dialogs::accepted("dialog", s),
            })
            .collect();
        out.detail.push_str(&format!(" ({})", said.join("; ")));
    }
}

/// An action that acts in the current tab, when that tab has closed by
/// itself: `there is no tab <name>`, at once. The tab actions still run.
pub fn in_missing_tab<D: Driver>(d: &D, action: &Action) -> Option<ActionOutcome> {
    if action.is_tab_action() {
        return None;
    }
    d.missing_tab().map(|name| ActionOutcome::failed(no_tab(&name)))
}

/// Run one action with the standard waits.
pub async fn execute<D: Driver>(d: &mut D, action: &Action) -> ActionOutcome {
    execute_with(d, action, &Timing::default()).await
}

pub async fn execute_with<D: Driver>(d: &mut D, action: &Action, timing: &Timing) -> ActionOutcome {
    execute_in(d, action, timing, &Policy::open()).await
}

pub async fn execute_in<D: Driver>(
    d: &mut D,
    action: &Action,
    timing: &Timing,
    policy: &Policy,
) -> ActionOutcome {
    if let Err(why) = action.validate() {
        return ActionOutcome::failed(format!("{CANNOT_RUN}{why}"));
    }
    if let Some(gone) = in_missing_tab(d, action) {
        return gone;
    }
    let mut out = run(d, action, timing, policy).await;
    append_dialogs(d, &mut out);
    out
}
