//! One Auto Run script, as the raw `.spec.ts` the `/gen-test` stage of the
//! PHR-PLAYWRIGHT-AUTOMATION repo starts from - or the reason it cannot be
//! one.
//!
//! The app acts on the FIRST match of a selector while Playwright's strict
//! mode throws when an action or a single-element expectation matches
//! several, so every such locator ends in `.first()` unless the selector
//! already picks one (`nth`, or a Legacy `text=` which ends in `.last()`).
//! Counting (`expect_count`) never gets one.

use super::table_ts::tables_ts;
use super::ts::{lit, locator};
use crate::autorun::recipe::{origin_of, RecipeStep};
use crate::autorun::runner::WATCHED_DOWNLOAD_WAIT_MS;
use crate::autorun::CaseScript;
use crate::browser::actions::{page_goals, url_rule, Action, DropAt, TAB_WAIT_MS, WHEN_VISIBLE_FLOOR_MS, WHEN_VISIBLE_MS};
use crate::browser::expect::{READ_TEXT_JS, WRITTEN_TEXT_JS};
use crate::browser::keys;
use crate::browser::locator::Target;
use crate::browser::position::READ_BOX_JS;
use crate::browser::table::{self, RowCount, SortAs, SortKind, TableCheck, SETTLE_MS, SETTLE_POLL_MS};
use crate::browser::timing::Timing;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Why a case cannot be exported, as a sentence for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untranslatable(pub String);

impl std::fmt::Display for Untranslatable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Untranslatable {}

type Res<T> = Result<T, Untranslatable>;

fn no<T>(s: impl Into<String>) -> Res<T> {
    Err(Untranslatable(s.into()))
}

pub struct RawSpecInput<'a> {
    pub script: &'a CaseScript,
    /// `suites/<seg>/test-cases/<stem>.md`
    pub md_path: String,
    pub feature_title: String,
    pub after_sign_in: &'a [RecipeStep],
    pub area_clicks: &'a [Target],
    /// For `return_to_area { area }`.
    pub area_clicks_by_name: &'a BTreeMap<String, Vec<Target>>,
    /// The case's own step action text, by step number.
    pub step_texts: &'a BTreeMap<i32, String>,
    /// The project's origins as `Recipe::origins()` gives them: the start
    /// address's first (it is Playwright's baseURL), then the allowed ones.
    pub origins: &'a [String],
    /// Why the case is left as `test.fixme`: a divergence waits for a
    /// person's ruling (`divergences::needs_ruling`). The body is still
    /// written, so the spec can be reviewed.
    pub fixme: Option<String>,
}

/// Every action of the script can be written as Playwright. It does not
/// see the sign-in recipe's `after_sign_in` (the script does not carry it);
/// `raw_spec` checks that too, and the named areas of `return_to_area`.
pub fn check(script: &CaseScript, origins: &[String]) -> Res<()> {
    for step in &script.steps {
        for a in &step.actions {
            check_action(a, origins)?;
        }
    }
    Ok(())
}

fn check_action(a: &Action, origins: &[String]) -> Res<()> {
    match a {
        Action::Navigate { url } => goto_target("navigate", url, origins).map(|_| ()),
        Action::CheckText { .. }
        | Action::RememberUrl { .. }
        | Action::Reload
        | Action::ExpireSession
        | Action::ReturnToArea { .. }
        | Action::ExpectTab { .. }
        | Action::SwitchTab { .. }
        | Action::ExpectTabClosed { .. } => Ok(()),
        // An `equals` goes through the same address rules as a navigation.
        Action::CheckUrl { equals: Some(want), .. } => goto_target("check_url", want, origins).map(|_| ()),
        Action::CheckUrl { .. } => Ok(()),
        Action::PressKey { key, .. } => key_name(key).map(|_| ()),
        Action::ExpectEach { selector, child, .. } => {
            check_target(selector)?;
            child.as_ref().map_or(Ok(()), check_target)
        }
        Action::ExpectOrder { selector, .. } => check_target(selector),
        Action::ExpectPosition { selector, relative_to, .. } => {
            check_target(selector)?;
            check_target(relative_to)
        }
        Action::PageUntil { next, until, until_all, .. } => {
            check_target(next)?;
            page_goals(until, until_all).into_iter().try_for_each(check_target)
        }
        Action::Click { selector }
        | Action::Fill { selector, .. }
        | Action::WaitFor { selector, .. }
        | Action::ExpectVisible { selector, .. }
        | Action::ExpectHidden { selector, .. }
        | Action::ExpectText { selector, .. }
        | Action::ExpectContainsText { selector, .. }
        | Action::ExpectCount { selector, .. }
        | Action::ExpectAttribute { selector, .. }
        | Action::ExpectStyle { selector, .. }
        | Action::ExpectEnabled { selector, .. }
        | Action::ExpectDisabled { selector, .. }
        | Action::ExpectChecked { selector, .. }
        | Action::ExpectFocused { selector, .. } => check_target(selector),
        Action::Drag { from, to, position, .. } => {
            if matches!(position, Some(DropAt::Before | DropAt::After)) {
                return no("drag before/after has no Playwright equivalent");
            }
            check_target(from)?;
            check_target(to)
        }
        Action::WhenVisible { selector, then, .. } | Action::RepeatWhileVisible { selector, then, .. } => {
            check_target(selector)?;
            then.iter().try_for_each(|a| check_action(a, origins))
        }
        Action::ExpectResponse { json, .. } => match json {
            Some(v) => match_json(v, "expect_response").map(|_| ()),
            None => Ok(()),
        },
        Action::ApiRequest { expect, .. } => match &expect.json {
            Some(v) => match_json(v, "api_request").map(|_| ()),
            None => Ok(()),
        },
        Action::OpenTab { name, url } => {
            if name == "main" {
                return no("open_tab of the main tab cannot be exported");
            }
            goto_target("open_tab", url, origins).map(|_| ())
        }
        Action::CloseTab { name } => {
            if name == "main" {
                return no("close_tab of the main tab cannot be exported");
            }
            Ok(())
        }
        Action::ExpectDownload { sheet, headers, cells, contains_text, pdf, .. } => {
            let field = if sheet.is_some() {
                Some("sheet")
            } else if headers.is_some() {
                Some("headers")
            } else if cells.is_some() {
                Some("cells")
            } else if contains_text.is_some() {
                Some("contains_text")
            } else if pdf.is_some() {
                Some("pdf")
            } else {
                None
            };
            match field {
                Some(f) => no(format!(
                    "expect_download with {f} cannot be exported: Playwright here checks only the downloaded file's name"
                )),
                None => Ok(()),
            }
        }
        Action::Upload { .. } => no("upload cannot be exported: the generated spec has no access to the project's Test files"),
        Action::SignIn { .. } => no("sign_in cannot be exported: the generated spec signs in through the repo's own auth setup"),
        // The export expands every use before translating, as the runner does.
        Action::UseComponent { component, .. } => no(format!("use_component \"{component}\" cannot be exported unexpanded")),
        Action::ExpectDialog { .. } => {
            no("expect_dialog cannot be exported: Playwright answers dialogs with a handler, not a check at a place in the steps")
        }
        Action::ExpectRow { table, .. } | Action::ExpectNoRow { table, .. } | Action::ExpectSorted { table, .. } => {
            check_target(table)
        }
        // The app refuses any other mix of the three, so the export does too.
        Action::ExpectRowCount { table, equals, at_least, at_most, .. } => {
            table::row_count(*equals, *at_least, *at_most).map_err(Untranslatable)?;
            check_target(table)
        }
    }
}

fn check_target(t: &Target) -> Res<()> {
    if matches!(t, Target::Chain(v) if v.is_empty()) {
        return no("an empty selector chain cannot be exported");
    }
    Ok(())
}

/// The whole raw spec for one case.
pub fn raw_spec(input: &RawSpecInput) -> Res<String> {
    check(input.script, input.origins)?;
    let mut em = Em { out: String::new(), input, scope: String::new(), cnt: Counters::default(), blocks: 0, tab_from: "0".into() };
    // `open_tab` marks its page claimed, so it needs the collector too.
    let tabs = uses(input, &|a| matches!(a, Action::ExpectTab { .. } | Action::OpenTab { .. }));

    let _ = write!(em.out, "// spec: {}\n", one_line(&input.md_path));
    em.out.push_str("// seed: suites/_generated/seed.spec.ts\n\n");
    em.out.push_str("import { test, expect } from '@playwright/test';\n\n");
    let _ = write!(em.out, "test.describe({}, () => {{\n", lit(&input.feature_title));
    let _ = write!(em.out, "  test({}, async ({{ page }}) => {{\n", lit(&input.script.title));
    // Playwright's conditional form: a single-string `test.fixme('...')`
    // is not a valid call signature in TypeScript.
    if let Some(reason) = &input.fixme {
        em.line(4, &format!("test.fixme(true, {});", lit(reason)));
    }

    em.line(4, "let cur = page;");
    // The addresses a `remember_url` notes, for a later `same_as`.
    if uses(input, &|a| matches!(a, Action::RememberUrl { .. })) {
        em.line(4, "const notedUrls: Record<string, string> = {};");
    }
    // The app's text reader: `expect_text`, `expect_contains_text`, an
    // `expect_each` that reads text and an `expect_order`.
    if uses(input, &|a| {
        matches!(
            a,
            Action::ExpectText { .. }
                | Action::ExpectContainsText { .. }
                | Action::ExpectOrder { .. }
                | Action::ExpectEach { attribute: None, visible: false, .. }
        )
    }) {
        em.read_text();
    }
    // The text as written in the page, for the two checks that accept it
    // when CSS draws the words in other capitals.
    if uses(input, &|a| matches!(a, Action::ExpectText { equals: Some(_), .. } | Action::ExpectContainsText { .. })) {
        em.read_written();
    }
    if uses(input, &|a| {
        matches!(a, Action::ExpectRow { .. } | Action::ExpectNoRow { .. } | Action::ExpectSorted { .. } | Action::ExpectRowCount { .. })
    }) {
        for l in tables_ts(4) {
            em.out.push_str(&l);
            em.out.push('\n');
        }
    }
    if uses(input, &|a| matches!(a, Action::ExpectCount { at_most: Some(_), .. })) {
        em.still_count();
    }
    // The three checks that pass only once they have held still.
    if uses(input, &|a| matches!(a, Action::ExpectEach { .. } | Action::ExpectOrder { .. } | Action::ExpectPosition { .. })) {
        em.settle_helper();
    }
    if uses(input, &|a| matches!(a, Action::ExpectEach { attribute: None, visible: false, .. })) {
        em.saw();
    }
    if uses(input, &|a| matches!(a, Action::ExpectPosition { .. })) {
        em.placed();
    }
    if tabs {
        em.collector();
    }
    for name in tab_names(input) {
        em.line(4, &format!("let tab_{name}: typeof page;"));
    }
    em.line(4, "await page.goto('/');");
    em.landing(4, input.area_clicks, "page")?;

    // A clean-up step runs whether the case's steps passed or failed, as
    // the app runs it: the steps go in a `try`, the clean-up in its
    // `finally`, in a `try` of its own whose error is noted, not thrown.
    let cleanup = input.script.cleanup_step();
    let at = if cleanup.is_some() { 6 } else { 4 };
    if cleanup.is_some() {
        em.out.push('\n');
        em.line(4, "try {");
    }
    let mut prev_mark: Option<String> = None;
    for (i, step) in input.script.case_steps().enumerate() {
        if cleanup.is_none() || i > 0 {
            em.out.push('\n');
        }
        let n = step.step_number;
        match input.step_texts.get(&n) {
            Some(t) => em.line(at, &format!("// {n}. {}", one_line(t))),
            None => em.line(at, &format!("// {n}.")),
        }
        if let Some(why) = &step.unchecked {
            em.line(at, &format!("// Not checked: {}", one_line(why)));
        }
        if tabs {
            let mark = format!("mark{n}");
            em.line(at, &format!("const {mark} = opened.length;"));
            em.tab_from = prev_mark.clone().unwrap_or_else(|| mark.clone());
            prev_mark = Some(mark);
        }
        em.block(n.to_string(), &step.actions, at)?;
    }
    if let Some(step) = cleanup {
        let n = step.step_number;
        em.line(4, "} finally {");
        match &step.unchecked {
            Some(why) => em.line(6, &format!("// Clean-up: {}", one_line(why))),
            None => em.line(6, "// Clean-up"),
        }
        if tabs {
            // The marks of the steps above are the `try`'s own: the
            // clean-up counts the tabs it claims from its own start.
            let mark = format!("mark{n}");
            em.line(6, &format!("const {mark} = opened.length;"));
            em.tab_from = mark;
        }
        // Its own error never replaces the steps' and never fails a spec
        // whose steps passed: the app never fails a case on its clean-up.
        em.line(6, "try {");
        em.block(n.to_string(), &step.actions, 8)?;
        em.line(6, "} catch (e) {");
        em.line(8, "test.info().annotations.push({ type: 'cleanup-failed', description: String(e) });");
        em.line(6, "}");
        em.line(4, "}");
    }

    em.out.push_str("  });\n});\n");
    Ok(em.out)
}

/// Comment text must stay on its line: a line terminator would end the
/// comment (U+2028 and U+2029 are line terminators in JavaScript).
fn one_line(s: &str) -> String {
    s.chars().map(|c| if matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') { ' ' } else { c }).collect()
}

/// Every action the spec can play: the recipe's `after_sign_in`, then the
/// script's steps, with what a `when_visible` guards included.
fn every_action(input: &RawSpecInput) -> Vec<Action> {
    fn walk(a: &Action, out: &mut Vec<Action>) {
        out.push(a.clone());
        if let Action::WhenVisible { then, .. } | Action::RepeatWhileVisible { then, .. } = a {
            then.iter().for_each(|t| walk(t, out));
        }
    }
    let mut out = Vec::new();
    for a in input.after_sign_in.iter().map(recipe_action) {
        walk(&a, &mut out);
    }
    for s in &input.script.steps {
        s.actions.iter().for_each(|a| walk(a, &mut out));
    }
    out
}

fn uses(input: &RawSpecInput, pred: &dyn Fn(&Action) -> bool) -> bool {
    every_action(input).iter().any(pred)
}

/// The app's own text reader (`READ_TEXT_JS`) as a TypeScript function of
/// the element, one line per entry: the body is the constant's, word for
/// word; only `this` is given a type, so the spec type-checks.
pub fn read_text_ts(indent: usize) -> Vec<String> {
    let typed = READ_TEXT_JS.replacen("function() {", "function (this: HTMLElement) {", 1);
    assert!(typed != READ_TEXT_JS, "READ_TEXT_JS no longer starts with `function() {{`");
    let pad = " ".repeat(indent);
    let mut lines: Vec<String> = typed.lines().map(|l| format!("{pad}{l}")).collect();
    if let Some(first) = lines.first_mut() {
        *first = format!("{pad}const readText = (e: Element) => ({}", first.trim_start());
    }
    if let Some(last) = lines.last_mut() {
        last.push_str(").call(e as HTMLElement);");
    }
    lines
}

/// The text as written in the page (`WRITTEN_TEXT_JS`: textContent, which
/// CSS `text-transform` does not change) as a TypeScript function of the
/// element, built as [`read_text_ts`] builds `readText`.
pub fn read_written_ts(indent: usize) -> Vec<String> {
    let typed = WRITTEN_TEXT_JS.replacen("function() {", "function (this: HTMLElement) {", 1);
    assert!(typed != WRITTEN_TEXT_JS, "WRITTEN_TEXT_JS no longer starts with `function() {{`");
    let pad = " ".repeat(indent);
    let mut lines: Vec<String> = typed.lines().map(|l| format!("{pad}{l}")).collect();
    if let Some(first) = lines.first_mut() {
        *first = format!("{pad}const readWritten = (e: Element) => ({}", first.trim_start());
    }
    if let Some(last) = lines.last_mut() {
        last.push_str(").call(e as HTMLElement);");
    }
    lines
}

/// The app's rule for the text a check compares, in TypeScript
/// (`expect::text_as_case`): the drawn text when it holds; else the text as
/// written in the page when only its letter case differs from the drawn one
/// and it holds; else the drawn text. `want` is already collapsed.
pub const AS_CASE_TS: &str = "const asCase = (drawn: string, written: string, want: string, whole: boolean): string => {\n  const holds = (t: string) => (whole ? t === want : t.includes(want));\n  if (holds(drawn)) return drawn;\n  return written.toLowerCase() === drawn.toLowerCase() && holds(written) ? written : drawn;\n};";

/// The app's own box reader (`READ_BOX_JS`) as a TypeScript function of the
/// element, one line per entry: the body is the constant's, word for word
/// (its frame walk - each frame element's corner, border and padding - is
/// what puts a box in the top window); only `this` and the frame walk's
/// `win` are given a type, so the spec type-checks. Playwright's own
/// `boundingBox()` is not used: it moves a box by the frame element's
/// border box, leaving the frame's border and padding out.
pub fn read_box_ts(indent: usize) -> Vec<String> {
    let typed = READ_BOX_JS.replacen("function() {", "function (this: Element) {", 1);
    assert!(typed != READ_BOX_JS, "READ_BOX_JS no longer starts with `function() {{`");
    let walked = typed.replacen(
        "let win = this.ownerDocument.defaultView;",
        "let win: Window | null = this.ownerDocument.defaultView;",
        1,
    );
    assert!(walked != typed, "READ_BOX_JS no longer starts its frame walk with `let win = ...defaultView;`");
    let pad = " ".repeat(indent);
    let mut lines: Vec<String> = walked.lines().map(|l| format!("{pad}{l}")).collect();
    if let Some(first) = lines.first_mut() {
        *first = format!("{pad}const readBox = (e: Element) => ({}", first.trim_start());
    }
    if let Some(last) = lines.last_mut() {
        last.push_str(").call(e);");
    }
    lines
}

/// `expect_position`'s rule, written once per spec that uses it: the same
/// as `browser::position::holds` and the element checks around it in
/// `browser::expect` - one element each (none and several are different
/// failures), a box of no size is nowhere, a pixel of give (`t`), and a
/// corner is being inside with the centre in that quarter, a centre within
/// a pixel of a midline counting for both halves. Answers why not, or what
/// it read: both boxes, which must stay within a pixel of where the hold
/// began (`settle`). Needs `readBox` above it.
pub const PLACED_TS: &str = r#"const placed = async (a: ReturnType<typeof page.locator>, b: ReturnType<typeof page.locator>, relation: string): Promise<string | { read: string; boxes: number[] }> => {
  const found: { x: number; y: number; w: number; h: number }[] = [];
  for (const [who, loc] of [['the element', a], ['the reference element', b]] as const) {
    const n = await loc.count();
    if (n === 0) return `${who} is not on the page`;
    if (n > 1) return `${who} matched ${n} elements, not one`;
    const box = await loc.evaluate(readBox, undefined, { timeout: left() });
    if (box.w <= 0 || box.h <= 0) return `${who} is drawn at no size, so it has no place`;
    found.push(box);
  }
  const [p, q] = found;
  const t = 1;
  const inside = p.x >= q.x - t && p.y >= q.y - t && p.x + p.w <= q.x + q.w + t && p.y + p.h <= q.y + q.h + t;
  const cx = p.x + p.w / 2;
  const cy = p.y + p.h / 2;
  const mx = q.x + q.w / 2;
  const my = q.y + q.h / 2;
  let ok = false;
  switch (relation) {
    case 'left_of':
      ok = p.x + p.w <= q.x + t;
      break;
    case 'right_of':
      ok = p.x >= q.x + q.w - t;
      break;
    case 'above':
      ok = p.y + p.h <= q.y + t;
      break;
    case 'below':
      ok = p.y >= q.y + q.h - t;
      break;
    case 'inside':
      ok = inside;
      break;
    case 'in_top_left':
      ok = inside && cx <= mx + t && cy <= my + t;
      break;
    case 'in_top_right':
      ok = inside && cx >= mx - t && cy <= my + t;
      break;
    case 'in_bottom_left':
      ok = inside && cx <= mx + t && cy >= my - t;
      break;
    case 'in_bottom_right':
      ok = inside && cx >= mx - t && cy >= my - t;
      break;
  }
  if (!ok) {
    const at = (r: { x: number; y: number; w: number; h: number }) => `(${Math.round(r.x)}, ${Math.round(r.y)}) ${Math.round(r.w)}x${Math.round(r.h)}`;
    return `${relation} does not hold: it is at ${at(p)}, and the reference at ${at(q)}`;
  }
  return { read: '', boxes: [p.x, p.y, p.w, p.h, q.x, q.y, q.w, q.h] };
};"#;

/// Whitespace runs as one space, ends trimmed - what the app does to both
/// sides of a text check.
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn sanitize(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect()
}

fn tab_ref(name: &str) -> String {
    if name == "main" {
        "page".to_string()
    } else {
        format!("tab_{}", sanitize(name))
    }
}

/// Tabs the script or the recipe's `after_sign_in` gives a name to
/// (`expect_tab`, `open_tab`), once each, in the order they first appear.
fn tab_names(input: &RawSpecInput) -> Vec<String> {
    let mut out = Vec::new();
    for a in every_action(input) {
        if let Action::ExpectTab { name, .. } | Action::OpenTab { name, .. } = a {
            let s = sanitize(&name);
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------
// Locators

fn wants_first(t: &Target) -> bool {
    match t {
        Target::Legacy(c) => !c.starts_with("text="),
        Target::One(s) => s.nth.is_none(),
        Target::Chain(v) => v.last().is_some_and(|s| s.nth.is_none()),
    }
}

/// A locator for an action or a single-element expectation.
fn one(t: &Target) -> String {
    let base = locator("cur", t);
    if wants_first(t) {
        format!("{base}.first()")
    } else {
        base
    }
}

/// A selector that also matches what cannot be seen is waited for as
/// merely there.
fn matches_hidden(t: &Target) -> bool {
    match t {
        Target::Legacy(_) => false,
        Target::One(s) => s.visible == Some(false),
        Target::Chain(v) => v.last().is_some_and(|s| s.visible == Some(false)),
    }
}

/// The same selector, with its last step made to match only what can be seen.
fn visible_only(t: &Target) -> Target {
    let mut t = t.clone();
    match &mut t {
        Target::Legacy(_) => {}
        Target::One(s) => s.visible = None,
        Target::Chain(v) => {
            if let Some(s) = v.last_mut() {
                s.visible = None;
            }
        }
    }
    t
}

fn wait_opts(t: &Target, timeout: u32) -> String {
    if matches_hidden(t) {
        format!("{{ state: 'attached', timeout: {timeout} }}")
    } else {
        format!("{{ timeout: {timeout} }}")
    }
}

/// How long an expectation looks: its own `timeout_ms`, else the app's
/// default (`Timing::expect_ms`, what `actions::wait` and `expect_response`
/// fall back to) - never Playwright's shorter 5 s default.
fn expect_timeout(own: Option<u32>) -> u64 {
    own.map(u64::from).unwrap_or_else(|| Timing::default().expect_ms)
}

fn call(name: &str, args: &[String], timeout: Option<u32>) -> String {
    let mut all: Vec<String> = args.to_vec();
    all.push(format!("{{ timeout: {} }}", expect_timeout(timeout)));
    format!("{name}({})", all.join(", "))
}

fn poll_opts(timeout: Option<u32>) -> String {
    format!(", {{ timeout: {} }}", expect_timeout(timeout))
}

/// What `toHaveCSS` compares an `expect_style` value with, as Auto Run
/// compares it (`browser::style`): a colour as the rgb() the browser
/// reports; a quoted value as written; any other value as a pattern that
/// ignores case - the whole value for `equals` and `not_equals`
/// (`whole`), anywhere in it for `contains`. As Auto Run makes each run
/// of spaces one, the pattern takes any run of spaces where the value has
/// one, and any spaces around a comma.
fn css_value(want: &str, whole: bool) -> String {
    let normal = crate::browser::style::normal(want);
    let colour = crate::browser::style::colour(want).is_some();
    if (colour || normal.contains(['"', '\''])) && whole {
        return lit(&normal);
    }
    let mut pattern = String::new();
    let mut after_comma = false;
    for c in normal.chars() {
        match c {
            ',' => pattern.push_str(r"\s*,"),
            ' ' if after_comma => pattern.push_str(r"\s*"),
            ' ' => pattern.push_str(r"\s+"),
            c => pattern.push_str(&escape_regex(&c.to_string())),
        }
        after_comma = c == ',';
    }
    let source = if whole { format!("^{pattern}$") } else { pattern };
    let flags = if normal.contains(['"', '\'']) { "" } else { ", 'i'" };
    format!("new RegExp({}{flags})", lit(&source))
}

/// Escape for a RegExp source built with `new RegExp(<lit>)`.
fn escape_regex(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if "\\^$.*+?()[]{}|/-".contains(c) {
            o.push('\\');
        }
        o.push(c);
    }
    o
}

fn glob_regex(s: &str) -> String {
    let mut o = String::from("^");
    for c in s.chars() {
        if c == '*' {
            o.push_str(".*");
        } else {
            o.push_str(&escape_regex(&c.to_string()));
        }
    }
    o.push('$');
    o
}

/// A script's address as a page address. A `/` path stays as it is; a full
/// address on the project's start origin (Playwright's baseURL, the first of
/// `origins`) becomes its path, query and hash; one on another allowed origin
/// stays a full address (`page.goto` takes one). Anything else - an origin
/// the project does not list, `//host`, `file:` or another scheme, a bare
/// relative address - would behave differently in the exported spec.
fn goto_target(kind: &str, url: &str, origins: &[String]) -> Res<String> {
    let u = url.trim();
    if u.starts_with('/') && !u.starts_with("//") {
        return Ok(u.to_string());
    }
    if u.contains("://") && !u.starts_with("//") {
        let Some(o) = origin_of(u) else {
            return no(format!("{kind} to \"{u}\" cannot be exported: it is not an http(s) address"));
        };
        if !origins.contains(&o) {
            return no(format!("{kind} to \"{u}\" cannot be exported: {o} is not one of the project's origins"));
        }
        if origins.first() != Some(&o) {
            return Ok(u.to_string());
        }
        let rest = &u[u.find("://").unwrap_or(0) + 3..];
        return Ok(match rest.find(['/', '?', '#']) {
            Some(j) if rest[j..].starts_with('/') => rest[j..].to_string(),
            Some(j) => format!("/{}", &rest[j..]),
            None => "/".to_string(),
        });
    }
    if u.starts_with("//") || u.to_ascii_lowercase().starts_with("file:") {
        return no(format!(
            "{kind} to \"{u}\" cannot be exported: only an http(s) address on the project's origins or a path starting with / can"
        ));
    }
    no(format!("{kind} to \"{u}\" cannot be exported: only a path starting with / or a full address on the project's origins can"))
}

/// The expected body of a `toMatchObject`, which compares objects and lists
/// only: a single value at the top cannot be written as one.
fn match_json(v: &Value, kind: &str) -> Res<String> {
    if !(v.is_object() || v.is_array()) {
        return no(format!(
            "{kind} json that is a single value ({v}) cannot be exported: Playwright's toMatchObject compares only objects and lists"
        ));
    }
    ts_json(v, kind)
}

/// A JSON value as a TypeScript object literal; every string through `lit`.
fn ts_json(v: &Value, kind: &str) -> Res<String> {
    Ok(match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => lit(s),
        Value::Array(items) => {
            if items.iter().any(Value::is_object) {
                return no(format!("{kind} json with a list of objects is compared differently by Playwright"));
            }
            let parts: Res<Vec<String>> = items.iter().map(|i| ts_json(i, kind)).collect();
            format!("[{}]", parts?.join(", "))
        }
        Value::Object(m) => {
            if m.is_empty() {
                "{}".into()
            } else {
                let parts: Res<Vec<String>> =
                    m.iter().map(|(k, v)| ts_json(v, kind).map(|t| format!("{}: {t}", lit(k)))).collect();
                format!("{{ {} }}", parts?.join(", "))
            }
        }
    })
}

/// The combination as Playwright names it, read the way the app reads it
/// (`keys::parse`: parts trimmed, modifiers in any case).
fn key_name(key: &str) -> Res<String> {
    let combo = keys::parse(key).map_err(Untranslatable)?;
    let mut parts: Vec<&str> =
        combo.modifiers.iter().map(|m| if m.name() == "Ctrl" { "Control" } else { m.name() }).collect();
    parts.push(combo.key_name());
    Ok(parts.join("+"))
}

// ---------------------------------------------------------------------
// Emission

#[derive(Default)]
struct Counters {
    resp: usize,
    dl: usize,
    api: usize,
}

struct Em<'a> {
    out: String,
    input: &'a RawSpecInput<'a>,
    /// Names the variables of the block being written: the step number, or
    /// `x<k>` for what is not a step.
    scope: String,
    cnt: Counters,
    blocks: usize,
    /// The mark an `expect_tab` counts opened tabs from: the start of the
    /// step before the current one (the app claims the newest unnamed tab
    /// opened since the PREVIOUS step began), or of the current step for
    /// the first.
    tab_from: String,
}

impl Em<'_> {
    fn line(&mut self, indent: usize, s: &str) {
        for _ in 0..indent {
            self.out.push(' ');
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// After a visit of the home page: the URL check, the recipe's
    /// `after_sign_in`, then the area's menu clicks.
    fn landing(&mut self, indent: usize, area: &[Target], on: &str) -> Res<()> {
        self.line(indent, &format!("await expect({on}).toHaveURL(/\\/hr\\/home\\/index/);"));
        let actions: Vec<Action> = self.input.after_sign_in.iter().map(recipe_action).collect();
        self.blocks += 1;
        let scope = format!("x{}", self.blocks);
        self.block(scope, &actions, indent)?;
        for t in area {
            self.line(indent, &format!("await {}.click();", one(t)));
        }
        Ok(())
    }

    /// The app's text reader, for `expect_text` and `expect_contains_text`.
    fn read_text(&mut self) {
        for l in read_text_ts(4) {
            self.out.push_str(&l);
            self.out.push('\n');
        }
    }

    /// The text as written in the page, and the app's rule for choosing
    /// between it and the drawn text, for `expect_text` (equals) and
    /// `expect_contains_text`. `asCase` goes out verbatim (`AS_CASE_TS`).
    fn read_written(&mut self) {
        for l in read_written_ts(4) {
            self.out.push_str(&l);
            self.out.push('\n');
        }
        self.line(4, AS_CASE_TS);
    }

    /// `expect_count` with `at_most` passes on too few, so it is not a poll:
    /// the app's own loop (`expect::keep_looking`) - the count must stay at
    /// or under the bound, the same, for `SETTLE_MS`, or time runs out on a
    /// pass still held - written once as `stillCount`, which answers the
    /// count the spec then compares.
    fn still_count(&mut self) {
        let poll = SETTLE_POLL_MS.to_string();
        let settle = SETTLE_MS.to_string();
        for l in [
            "const stillCount = async (loc: ReturnType<typeof page.locator>, most: number, timeout: number): Promise<number> => {",
            "  const end = Date.now() + timeout;",
            "  let held = -1;",
            "  let since = 0;",
            "  for (;;) {",
            "    const n = await loc.count();",
            "    if (n > most) {",
            "      held = -1;",
            "    } else {",
            "      if (n !== held) {",
            "        held = n;",
            "        since = Date.now();",
            "      }",
            "      if (Date.now() - since >= SETTLE) return n;",
            "    }",
            "    if (Date.now() >= end) return held === -1 ? n : held;",
            "    await new Promise((r) => setTimeout(r, Math.min(POLL, Math.max(0, end - Date.now()))));",
            "  }",
            "};",
        ] {
            self.line(4, &l.replace("SETTLE", &settle).replace("POLL", &poll));
        }
    }

    /// `expect_each`, `expect_order` and `expect_position` pass only once
    /// the same pass has held for `SETTLE_MS` (`expect::keep_looking`), so
    /// they are not a poll: `settle` looks every `SETTLE_POLL_MS`, a look
    /// that fails (or throws, an item re-rendered under it) starts the hold
    /// over, a look that differs from the one the hold began with - the
    /// item count, the texts read, a box more than a pixel off - restarts
    /// it too, and a pass still held at the deadline passes - also when the
    /// look that reached the deadline was cut off by it, as the app's loop
    /// keeps a pass through a look that ran out of time. It answers why not,
    /// or null. `left` is what every call of a look may take: two seconds,
    /// or what the check has left, so a page that stopped answering cannot
    /// carry a look of many items far past the check's timeout.
    fn settle_helper(&mut self) {
        let poll = SETTLE_POLL_MS.to_string();
        let settle = SETTLE_MS.to_string();
        for l in [
            "let lookEnd = 0;",
            "const left = () => Math.max(1, Math.min(2000, lookEnd - Date.now()));",
            "type Held = { read: string; boxes?: number[] };",
            "const settle = async (look: () => Promise<string | Held>, timeout: number): Promise<string | null> => {",
            "  const end = Date.now() + timeout;",
            "  lookEnd = end;",
            "  let held: Held | null = null;",
            "  let since = 0;",
            "  let last = 'it was not looked at';",
            "  for (;;) {",
            "    let cut = false;",
            "    const got = await look().catch((e: Error) => {",
            "      cut = Date.now() >= end;",
            "      return String(e.message).split('\\n')[0];",
            "    });",
            "    if (typeof got === 'string') {",
            "      if (!(cut && held !== null)) {",
            "        held = null;",
            "        last = got;",
            "      }",
            "    } else {",
            "      const was = held;",
            "      const before = was?.boxes ?? [];",
            "      const boxes = got.boxes ?? [];",
            "      const same = was !== null && was.read === got.read && before.length === boxes.length && before.every((v, i) => Math.abs(v - boxes[i]) <= 1);",
            "      if (!same) {",
            "        held = got;",
            "        since = Date.now();",
            "      }",
            "      if (Date.now() - since >= SETTLE) return null;",
            "    }",
            "    if (Date.now() >= end) return held === null ? last : null;",
            "    await new Promise((r) => setTimeout(r, Math.min(POLL, Math.max(0, end - Date.now()))));",
            "  }",
            "};",
        ] {
            self.line(4, &l.replace("SETTLE", &settle).replace("POLL", &poll));
        }
    }

    /// What a failure of an `expect_each` that reads text shows of the text:
    /// never a form control's value (a password's included) - the run names
    /// such an item by its label instead.
    fn saw(&mut self) {
        for l in [
            "const saw = async (el: ReturnType<typeof page.locator>, got: string | null): Promise<string> =>",
            "  (await el.evaluate((e: Element) => e instanceof HTMLInputElement || e instanceof HTMLTextAreaElement, undefined, { timeout: left() })) ? '(a field)' : JSON.stringify(got);",
        ] {
            self.line(4, l);
        }
    }

    /// The app's box reader, then `expect_position`'s rule.
    fn placed(&mut self) {
        for l in read_box_ts(4) {
            self.out.push_str(&l);
            self.out.push('\n');
        }
        for l in PLACED_TS.lines() {
            self.line(4, l);
        }
    }

    /// The tabs the page opens, as they open, for `expect_tab` to claim.
    fn collector(&mut self) {
        for l in [
            "const opened: (typeof page)[] = [];",
            "const claimed = new Set<typeof page>();",
            "page.context().on('page', p => { opened.push(p); });",
            "const nextTab = async (from: number, timeout: number) => {",
            "  const end = Date.now() + timeout;",
            "  for (;;) {",
            "    const fresh = opened.slice(from).filter(p => !claimed.has(p));",
            "    if (fresh.length > 0) {",
            "      const p = fresh[fresh.length - 1];",
            "      claimed.add(p);",
            "      return p;",
            "    }",
            "    if (Date.now() >= end) {",
            "      throw new Error(`No new tab opened within ${timeout} ms`);",
            "    }",
            "    await page.waitForTimeout(100);",
            "  }",
            "};",
        ] {
            self.line(4, l);
        }
    }

    /// A list of actions, with what they expect armed first.
    fn block(&mut self, scope: String, actions: &[Action], indent: usize) -> Res<()> {
        let saved_scope = std::mem::replace(&mut self.scope, scope);
        let saved_cnt = std::mem::take(&mut self.cnt);
        let mut armed = Counters::default();
        self.arm(actions, indent, &mut armed);
        let mut r = Ok(());
        for a in actions {
            r = self.action(a, indent);
            if r.is_err() {
                break;
            }
        }
        self.scope = saved_scope;
        self.cnt = saved_cnt;
        r
    }

    /// Every armed wait gets a no-op rejection handler at once: one whose
    /// check sits in a `when_visible` that is skipped is never awaited, and
    /// its timeout must not surface as an unhandled rejection. Awaiting the
    /// wait itself still rejects when its check does run.
    fn settle(&mut self, indent: usize, var: &str) {
        self.line(indent, &format!("{var}.catch(() => {{}});"));
    }

    fn arm(&mut self, actions: &[Action], indent: usize, n: &mut Counters) {
        for a in actions {
            let sc = self.scope.clone();
            match a {
                Action::ExpectResponse { method, url_contains, timeout_ms, .. } => {
                    // waitForResponse takes the FIRST match after arming while the app
                    // judges the latest one; url and method are trimmed as the app does.
                    let mut test = format!("r.url().toLowerCase().includes({})", lit(&url_contains.trim().to_lowercase()));
                    if let Some(m) = method.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
                        let _ = write!(test, " && r.request().method() === {}", lit(&m.to_uppercase()));
                    }
                    let opt = format!(", {{ timeout: {} }}", expect_timeout(*timeout_ms));
                    let i = n.resp;
                    n.resp += 1;
                    self.line(indent, &format!("const resp{sc}_{i} = cur.waitForResponse(r => {test}{opt});"));
                    self.settle(indent, &format!("resp{sc}_{i}"));
                }
                Action::ExpectDownload { within_ms, .. } => {
                    let i = n.dl;
                    n.dl += 1;
                    let ms = within_ms.unwrap_or(WATCHED_DOWNLOAD_WAIT_MS);
                    self.line(indent, &format!("const dl{sc}_{i} = cur.waitForEvent('download', {{ timeout: {ms} }});"));
                    self.settle(indent, &format!("dl{sc}_{i}"));
                }
                Action::WhenVisible { then, .. } | Action::RepeatWhileVisible { then, .. } => self.arm(then, indent, n),
                _ => {}
            }
        }
    }

    /// A `repeat_while_visible` as the same bounded loop: each pass reads
    /// the visible matches once, sets aside the ones whose row (the app's
    /// own `repeat::ROW_OF_JS`) reads a protected text (`repeat::KEPT_OF_JS`),
    /// marks the first other one's row with `data-autorun-row` and runs the
    /// `then` actions, whose row-scoped locators resolve inside it. It ends
    /// when none is left and throws once `max` passes left some.
    fn repeat(&mut self, selector: &Target, max: Option<u32>, except: &[String], then: &[Action], indent: usize) -> Res<()> {
        use crate::browser::repeat::{KEPT_OF_JS, ROW_OF_JS, ROW_SCOPE};
        let Some(max) = max else {
            return no("repeat_while_visible without max cannot be exported");
        };
        let except_js = serde_json::to_string(except).map_err(|e| Untranslatable(e.to_string()))?;
        let all = locator("cur", &visible_only(selector));
        self.line(indent, "{");
        let i = indent + 2;
        self.line(i, &format!("const except = {except_js};"));
        self.line(i, &format!("const pick = () => {all}.evaluateAll((els, except) => {{"));
        let mut js = |name: &str, body: &str| {
            let mut lines = body.lines();
            if let Some(first) = lines.next() {
                self.line(i + 2, &format!("const {name} = {first}"));
            }
            for l in lines {
                self.line(i + 2, l);
            }
        };
        js("rowOf", &format!("{ROW_OF_JS};"));
        js("keptOf", &format!("{KEPT_OF_JS};"));
        for l in [
            "for (const d of document.querySelectorAll('[data-autorun-row]')) d.removeAttribute('data-autorun-row');",
            "let left = 0;",
            "let first = null;",
            "for (const e of els) {",
            "  const row = rowOf(e);",
            "  if (except.length && !row) throw new Error('a match is not inside a list row');",
            "  if (row && except.length && keptOf(row, except) !== null) continue;",
            "  left++;",
            "  if (!first) first = row;",
            "}",
            "if (first) first.setAttribute('data-autorun-row', '');",
            "return left;",
        ] {
            self.line(i + 2, l);
        }
        self.line(i, "}, except);");
        self.line(i, "for (let pass = 0; ; pass++) {");
        self.line(i + 2, "const left = await pick();");
        self.line(i + 2, "if (left === 0) break;");
        self.line(
            i + 2,
            &format!("if (pass >= {max}) throw new Error('still ' + left + ' rows after max {max} passes');"),
        );
        for a in then {
            self.action(a, i + 2)?;
        }
        self.line(
            i + 2,
            &format!(
                "await expect({}).toHaveCount(0, {{ timeout: {} }}).catch(() => {{}});",
                locator("cur", &Target::Legacy(ROW_SCOPE.to_string())),
                crate::autorun::runner::ROW_GONE_WAIT_MS
            ),
        );
        self.line(i, "}");
        self.line(
            i,
            "await cur.evaluate(() => document.querySelectorAll('[data-autorun-row]').forEach(e => e.removeAttribute('data-autorun-row')));",
        );
        self.line(indent, "}");
        Ok(())
    }

    /// `expect_each`: the list, then each item, as `each::look_each` reads
    /// them - more than `EACH_MAX` items is a failure, fewer than `min`
    /// too; the `child` is the one element the locator finds in the item;
    /// the text is the app's (`readText`, already collapsed), an attribute
    /// is read raw. What it passes on is the item count, which must hold.
    #[allow(clippy::too_many_arguments)]
    fn each(
        &mut self,
        indent: usize,
        selector: &Target,
        min: Option<u32>,
        child: Option<&Target>,
        attribute: Option<&str>,
        visible: bool,
        equals: &Option<String>,
        contains: &Option<String>,
        matches: &Option<String>,
        not_empty: bool,
        timeout_ms: Option<u32>,
    ) -> Res<()> {
        let min = min.unwrap_or(1);
        let max = crate::browser::each::EACH_MAX;
        let at = |extra: usize| indent + 2 + extra;
        self.line(indent, "expect(await settle(async () => {");
        self.line(at(0), &format!("const items = {};", locator("cur", selector)));
        self.line(at(0), "const n = await items.count();");
        self.line(at(0), &format!("if (n > {max}) return `matched ${{n}} items, more than the {max} expect_each reads`;"));
        self.line(at(0), &format!("if (n < {min}) return `expected at least {min}, counted ${{n}}`;"));
        self.line(at(0), "for (let i = 0; i < n; i++) {");
        self.line(at(2), "const item = items.nth(i);");
        match child {
            Some(c) => {
                self.line(at(2), &format!("const el = {};", locator("item", c)));
                self.line(at(2), "const found = await el.count();");
                self.line(
                    at(2),
                    &format!("if (found !== 1) return `item ${{i + 1}} of ${{n}}: found ${{found}} of ` + {};", lit(&c.describe())),
                );
                // A selector written to match what cannot be seen is judged
                // visible by the run once it has found the one element.
                if visible && matches_hidden(c) {
                    self.line(
                        at(2),
                        &format!(
                            "if (!(await el.isVisible())) return `item ${{i + 1}} of ${{n}}: ` + {};",
                            lit(&format!("{} {}", c.describe(), crate::browser::expect::HIDDEN))
                        ),
                    );
                }
            }
            None => self.line(at(2), "const el = item;"),
        }
        if !visible {
            // `validate` holds the four to exactly one; `want` is how a
            // value fails, collapsed for text as the run collapses it.
            let text = attribute.is_none();
            let fix = |w: &str| if text { collapse(w) } else { w.to_string() };
            let fail = if text {
                "return `item ${i + 1} of ${n}: saw ${await saw(el, got)}`;"
            } else {
                "return `item ${i + 1} of ${n}: saw ${JSON.stringify(got)}`;"
            };
            let judge = match (equals, contains, matches, not_empty) {
                (Some(w), _, _, _) => format!("got !== {}", lit(&fix(w))),
                (_, Some(w), _, _) => format!("got === null || !got.includes({})", lit(&fix(w))),
                (_, _, Some(p), _) => format!("got === null || !new RegExp({}).test(got)", lit(p)),
                (_, _, _, true) => "!got || !got.trim()".to_string(),
                _ => return no("expect_each takes exactly one of visible, equals, contains, matches or not_empty"),
            };
            match attribute {
                Some(name) => self.line(at(2), &format!("const got = await el.getAttribute({}, {{ timeout: left() }});", lit(name))),
                None => self.line(at(2), "const got = await el.evaluate(readText, undefined, { timeout: left() });"),
            }
            self.line(at(2), &format!("if ({judge}) {fail}"));
        }
        self.line(at(0), "}");
        // An item the page took away mid-read says nothing about the page now.
        self.line(at(0), "if ((await items.count()) !== n) return 'the list changed while it was read';");
        self.line(at(0), "return { read: String(n) };");
        self.line(indent, &format!("}}, {})).toBeNull();", expect_timeout(timeout_ms)));
        Ok(())
    }

    /// `expect_order`: every item's text read as the run reads it, then each
    /// wanted text (collapsed) matched by the earliest item after the one
    /// that held the text before it. What it passes on is the texts read.
    /// A failure names the wanted texts only - never what a field holds.
    fn order(&mut self, indent: usize, selector: &Target, texts: &[String], timeout_ms: Option<u32>) {
        let max = crate::browser::order::ORDER_MAX;
        let order = texts.iter().map(|t| lit(&collapse(t))).collect::<Vec<_>>().join(", ");
        let at = |extra: usize| indent + 2 + extra;
        self.line(indent, "expect(await settle(async () => {");
        self.line(at(0), &format!("const items = {};", locator("cur", selector)));
        self.line(at(0), "const n = await items.count();");
        self.line(at(0), &format!("if (n === 0) return {};", lit(crate::browser::expect::NOT_ON_PAGE)));
        self.line(at(0), &format!("if (n > {max}) return `matched ${{n}} items, more than the {max} expect_order reads`;"));
        self.line(at(0), "const texts: string[] = [];");
        self.line(
            at(0),
            "for (let i = 0; i < n; i++) texts.push(await items.nth(i).evaluate(readText, undefined, { timeout: left() }));",
        );
        self.line(at(0), "if ((await items.count()) !== n) return 'the list changed while it was read';");
        self.line(at(0), &format!("const order = [{order}];"));
        self.line(at(0), "let from = 0;");
        self.line(at(0), "for (const want of order) {");
        self.line(at(2), "const at = texts.findIndex((t, i) => i >= from && t.includes(want));");
        self.line(
            at(2),
            "if (at === -1) return `expected text order ${JSON.stringify(order)} but ${JSON.stringify(want)} has no place after the one before it`;",
        );
        self.line(at(2), "from = at + 1;");
        self.line(at(0), "}");
        self.line(at(0), "return { read: texts.join('\\u001f') };");
        self.line(indent, &format!("}}, {})).toBeNull();", expect_timeout(timeout_ms)));
    }

    /// `page_until`, written as the walk the app makes: look for what is
    /// not seen yet, stop when all of it has been, fail at `max_pages` or
    /// when `next` is gone or disabled (its `disabled` property,
    /// `aria-disabled="true"` or a class token `disabled`), otherwise take
    /// the page's text, click it and wait until that text differs (never
    /// `networkidle`, which an in-page click has always reached).
    fn page_until(&mut self, indent: usize, next: &Target, until: &[&Target], max_pages: u32) {
        let locators: Vec<String> = until.iter().map(|u| locator("cur", u)).collect();
        let names: Vec<String> = until.iter().map(|u| lit(&u.describe())).collect();
        self.line(indent, "{");
        self.line(indent + 2, &format!("const until = [{}];", locators.join(", ")));
        self.line(indent + 2, &format!("const names = [{}];", names.join(", ")));
        self.line(indent + 2, "const seen: number[] = until.map(() => 0);");
        self.line(indent + 2, "const signature = () => cur.evaluate(() => { const b = document.body || document.documentElement; return b ? b.innerText : ''; });");
        self.line(indent + 2, "for (let p = 1; ; p++) {");
        self.line(indent + 4, "for (const [k, l] of until.entries()) if (!seen[k] && (await l.count()) > 0) seen[k] = p;");
        self.line(indent + 4, "if (seen.every((s) => s > 0)) break;");
        self.line(indent + 4, "const missing = names.filter((_, k) => !seen[k]).join(' and ');");
        self.line(
            indent + 4,
            &format!("if (p >= {max_pages}) throw new Error('walked ' + p + ' pages and never saw ' + missing);"),
        );
        self.line(indent + 4, &format!("const next = {};", one(next)));
        self.line(
            indent + 4,
            "if (!(await next.isVisible()) || !(await next.isEnabled()) || (await next.evaluate((e) => e.classList.contains('disabled')))) throw new Error('walked ' + p + ' pages and never saw ' + missing + ': the next control is gone or disabled');",
        );
        self.line(indent + 4, "const before = await signature();");
        self.line(indent + 4, "await next.click();");
        self.line(
            indent + 4,
            &format!(
                "await expect.poll(signature, {{ timeout: {} }}).not.toBe(before).catch(() => {{ throw new Error('Next did not turn the page (page ' + p + ').'); }});",
                Timing::default().action_ms
            ),
        );
        self.line(indent + 2, "}");
        self.line(indent, "}");
    }

    fn action(&mut self, a: &Action, indent: usize) -> Res<()> {
        check_action(a, self.input.origins)?;
        let sc = self.scope.clone();
        match a {
            Action::Navigate { url } => self.line(indent, &format!("await cur.goto({});", lit(&goto_target("navigate", url, self.input.origins)?))),
            Action::Click { selector } => self.line(indent, &format!("await {}.click();", one(selector))),
            Action::Fill { selector, value } => {
                self.line(indent, &format!("await {}.fill({});", one(selector), lit(value)))
            }
            Action::WaitFor { selector, timeout_ms } => {
                self.line(indent, &format!("await {}.waitFor({});", one(selector), wait_opts(selector, *timeout_ms)))
            }
            Action::CheckText { value } => self.line(
                indent,
                &format!("await expect(cur.locator('body')).toContainText({}, {{ ignoreCase: true, useInnerText: true }});", lit(value)),
            ),
            Action::CheckUrl { equals: Some(want), timeout_ms, .. } => {
                // The same rule the app applies (`UrlRule`): never a full
                // address turned into a path. The address must still be one
                // the export can reach.
                goto_target("check_url", want, self.input.origins)?;
                let rule = url_rule(want);
                let t = expect_timeout(*timeout_ms);
                let origin = if rule.origin.is_some() { "u.origin + " } else { "" };
                let hash = if rule.fragment.is_some() { " + u.hash" } else { "" };
                self.line(
                    indent,
                    &format!(
                        "await expect.poll(() => {{ const u = new URL(cur.url()); return {origin}u.pathname + u.search{hash}; }}, {{ timeout: {t} }}).toBe({});",
                        lit(&rule.expected())
                    ),
                );
            }
            Action::CheckUrl { same_as: Some(name), timeout_ms, .. } => self.line(
                indent,
                &format!(
                    "await expect.poll(() => cur.url(), {{ timeout: {} }}).toBe(notedUrls[{}]);",
                    expect_timeout(*timeout_ms),
                    lit(name)
                ),
            ),
            Action::CheckUrl { contains, timeout_ms, .. } => self.line(
                indent,
                &format!(
                    "await expect(cur).toHaveURL(new RegExp({}), {{ timeout: {} }});",
                    lit(&escape_regex(contains)),
                    expect_timeout(*timeout_ms)
                ),
            ),
            Action::RememberUrl { name } => self.line(indent, &format!("notedUrls[{}] = cur.url();", lit(name))),
            Action::ExpectVisible { selector, timeout_ms } => {
                self.line(indent, &format!("await expect({}).{};", one(selector), call("toBeVisible", &[], *timeout_ms)))
            }
            Action::ExpectHidden { selector, timeout_ms } => {
                self.line(indent, &format!("await expect({}).{};", one(selector), call("toBeHidden", &[], *timeout_ms)))
            }
            // The app reads a field's value, a list's chosen label, else the
            // rendered text (READ_TEXT_JS), collapses whitespace on both sides
            // and compares case-sensitively: whole, or as a substring.
            Action::ExpectText { selector, equals, matches, timeout_ms } => {
                // `validate` holds the two to exactly one.
                if let Some(equals) = equals {
                    // The drawn text, or the text as written in the page when
                    // only the letter case differs (`asCase`).
                    let w = lit(&collapse(equals));
                    let l = one(selector);
                    self.line(
                        indent,
                        &format!(
                            "await expect.poll(async () => asCase(await {l}.evaluate(readText), await {l}.evaluate(readWritten), {w}, true){}).toBe({w});",
                            poll_opts(*timeout_ms)
                        ),
                    );
                    return Ok(());
                }
                let want = match matches {
                    Some(p) => format!("toMatch(new RegExp({}))", lit(p)),
                    _ => return no("expect_text takes exactly one of equals or matches"),
                };
                self.line(
                    indent,
                    &format!(
                        "await expect.poll(() => {}.evaluate(readText){}).{want};",
                        one(selector),
                        poll_opts(*timeout_ms)
                    ),
                )
            }
            Action::ExpectContainsText { selector, value, timeout_ms } => {
                let w = lit(&collapse(value));
                let l = one(selector);
                self.line(
                    indent,
                    &format!(
                        "await expect.poll(async () => asCase(await {l}.evaluate(readText), await {l}.evaluate(readWritten), {w}, false){}).toContain({w});",
                        poll_opts(*timeout_ms)
                    ),
                )
            }
            Action::ExpectCount { selector, equals, at_least, at_most, timeout_ms } => {
                match table::row_count(*equals, *at_least, *at_most).map_err(Untranslatable)? {
                    table::RowCount::Equals(n) => self.line(
                        indent,
                        &format!(
                            "await expect({}).{};",
                            locator("cur", selector),
                            call("toHaveCount", &[n.to_string()], *timeout_ms)
                        ),
                    ),
                    table::RowCount::AtMost(n) => self.line(
                        indent,
                        &format!(
                            "expect(await stillCount({}, {n}, {})).toBeLessThanOrEqual({n});",
                            locator("cur", selector),
                            expect_timeout(*timeout_ms)
                        ),
                    ),
                    table::RowCount::AtLeast(n) => self.line(
                        indent,
                        &format!(
                            "await expect.poll(() => {}.count(){}).toBeGreaterThanOrEqual({n});",
                            locator("cur", selector),
                            poll_opts(*timeout_ms)
                        ),
                    ),
                }
            }
            Action::ExpectEach { selector, min, child, attribute, visible, equals, contains, matches, not_empty, timeout_ms } => {
                self.each(indent, selector, *min, child.as_ref(), attribute.as_deref(), *visible, equals, contains, matches, *not_empty, *timeout_ms)?
            }
            Action::ExpectOrder { selector, texts, timeout_ms } => self.order(indent, selector, texts, *timeout_ms),
            Action::PageUntil { next, until, until_all, max_pages } => {
                self.page_until(indent, next, &page_goals(until, until_all), *max_pages)
            }
            Action::ExpectPosition { selector, relative_to, relation, timeout_ms } => {
                // The relation's serde name is what the helper switches on.
                let name = serde_json::to_value(relation).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
                self.line(
                    indent,
                    &format!(
                        "expect(await settle(() => placed({}, {}, {}), {})).toBeNull();",
                        locator("cur", selector),
                        locator("cur", relative_to),
                        lit(&name),
                        expect_timeout(*timeout_ms)
                    ),
                )
            }
            Action::ExpectAttribute { selector, name, matches: Some(p), timeout_ms, .. } => self.line(
                indent,
                &format!(
                    "await expect({}).{};",
                    one(selector),
                    call("toHaveAttribute", &[lit(name), format!("new RegExp({})", lit(p))], *timeout_ms)
                ),
            ),
            Action::ExpectAttribute { selector, name, equals: Some(equals), timeout_ms, .. } => self.line(
                indent,
                &format!(
                    "await expect({}).{};",
                    one(selector),
                    call("toHaveAttribute", &[lit(name), lit(equals)], *timeout_ms)
                ),
            ),
            Action::ExpectAttribute { selector, name, equals: None, timeout_ms, .. } => self.line(
                indent,
                &format!("await expect({}).not.{};", one(selector), call("toHaveAttribute", &[lit(name)], *timeout_ms)),
            ),
            Action::ExpectStyle { selector, property, equals, contains, not_equals, timeout_ms } => {
                // `validate` holds the three to exactly one.
                let picked = match (equals, contains, not_equals) {
                    (Some(w), _, _) => Some(("", css_value(w, true))),
                    (_, Some(w), _) => Some(("", css_value(w, false))),
                    (_, _, Some(w)) => Some((".not", css_value(w, true))),
                    _ => None,
                };
                if let Some((not, arg)) = picked {
                    self.line(
                        indent,
                        &format!(
                            "await expect({}){not}.{};",
                            one(selector),
                            call("toHaveCSS", &[lit(property.trim()), arg], *timeout_ms)
                        ),
                    )
                }
            }
            Action::ExpectEnabled { selector, timeout_ms } => self.line(
                indent,
                &format!("await expect({}).{};", one(selector), call("toBeEnabled", &[], *timeout_ms)),
            ),
            Action::ExpectDisabled { selector, timeout_ms } => self.line(
                indent,
                &format!("await expect({}).{};", one(selector), call("toBeDisabled", &[], *timeout_ms)),
            ),
            // Clear is not partly ticked: a native box reads its own state,
            // and an aria widget saying "mixed" fails, as it does in a run.
            Action::ExpectChecked { selector, checked: true, timeout_ms } => self.line(
                indent,
                &format!("await expect({}).{};", one(selector), call("toBeChecked", &[], *timeout_ms)),
            ),
            Action::ExpectChecked { selector, checked: false, timeout_ms } => {
                self.line(indent, &format!("await expect({}).not.{};", one(selector), call("toBeChecked", &[], *timeout_ms)));
                self.line(
                    indent,
                    &format!(
                        "await expect({}).not.{};",
                        one(selector),
                        call("toHaveAttribute", &[lit("aria-checked"), lit("mixed")], *timeout_ms)
                    ),
                );
            }
            Action::ExpectFocused { selector, timeout_ms } => {
                self.line(
                    indent,
                    &format!(
                        "await expect({}.and(cur.locator(':focus-within'))).{};",
                        one(selector),
                        call("toHaveCount", &["1".to_string()], *timeout_ms)
                    ),
                )
            }
            Action::WhenVisible { selector, within_ms, then } => {
                // The app's look needs a VISIBLE match whatever the selector's
                // own visible flag says, and waits at least the floor.
                let w = within_ms.unwrap_or(WHEN_VISIBLE_MS).max(WHEN_VISIBLE_FLOOR_MS);
                self.line(
                    indent,
                    &format!(
                        "if (await {}.waitFor({{ timeout: {w} }}).then(() => true, () => false)) {{",
                        one(&visible_only(selector)),
                    ),
                );
                for a in then {
                    self.action(a, indent + 2)?;
                }
                self.line(indent, "}");
            }
            Action::RepeatWhileVisible { selector, max, except, then } => self.repeat(selector, *max, except, then, indent)?,
            Action::ExpectResponse { status, json, .. } => {
                let i = self.cnt.resp;
                self.cnt.resp += 1;
                self.line(indent, &format!("const r{sc}_{i} = await resp{sc}_{i};"));
                self.line(indent, &format!("expect(r{sc}_{i}.status()).toBe({status});"));
                if let Some(j) = json {
                    self.line(
                        indent,
                        &format!("expect(await r{sc}_{i}.json()).toMatchObject({});", match_json(j, "expect_response")?),
                    );
                }
            }
            Action::ApiRequest { path, query, expect, .. } => {
                let i = self.cnt.api;
                self.cnt.api += 1;
                // maxRedirects 0: the app fails an answer redirected elsewhere
                // (a sign-in page answers 200 too), so a 302 must show as 302.
                let params = if query.is_empty() {
                    ", { maxRedirects: 0 }".to_string()
                } else {
                    let kv: Vec<String> = query.iter().map(|(k, v)| format!("{}: {}", lit(k), lit(v))).collect();
                    format!(", {{ params: {{ {} }}, maxRedirects: 0 }}", kv.join(", "))
                };
                self.line(indent, &format!("const a{sc}_{i} = await cur.request.get({}{params});", lit(path)));
                self.line(indent, &format!("expect(a{sc}_{i}.status()).toBe({});", expect.status));
                if let Some(j) = &expect.json {
                    self.line(
                        indent,
                        &format!("expect(await a{sc}_{i}.json()).toMatchObject({});", match_json(j, "api_request")?),
                    );
                }
            }
            Action::Reload => self.line(indent, "await cur.reload();"),
            Action::ExpireSession => self.line(indent, "await cur.context().clearCookies();"),
            Action::ReturnToArea { .. } => {
                let clicks: Vec<Target> = match a.area_named() {
                    None => self.input.area_clicks.to_vec(),
                    Some(name) => match self.input.area_clicks_by_name.get(name) {
                        Some(c) => c.clone(),
                        None => {
                            return no(format!(
                                "return_to_area names the area \"{name}\", which has no recorded menu path"
                            ))
                        }
                    },
                };
                self.line(indent, "await cur.goto('/');");
                self.landing(indent, &clicks, "cur")?;
            }
            Action::PressKey { key, times } => {
                for _ in 0..times.unwrap_or(1).max(1) {
                    self.line(indent, &format!("await cur.keyboard.press({});", lit(&key_name(key)?)));
                }
            }
            Action::Drag { from, to, .. } => {
                self.line(indent, &format!("await {}.dragTo({});", one(from), one(to)))
            }
            Action::ExpectDownload { name, .. } => {
                let j = self.cnt.dl;
                self.cnt.dl += 1;
                self.line(indent, &format!("const d{sc}_{j} = await dl{sc}_{j};"));
                self.line(
                    indent,
                    &format!("expect(d{sc}_{j}.suggestedFilename()).toMatch(new RegExp({}, 'i'));", lit(&glob_regex(name))),
                );
            }
            Action::ExpectTab { name, url_contains, within_ms } => {
                let t = tab_ref(name);
                let ms = within_ms.unwrap_or(TAB_WAIT_MS);
                let from = self.tab_from.clone();
                self.line(indent, &format!("{t} = await nextTab({from}, {ms});"));
                if let Some(u) = url_contains {
                    self.line(indent, &format!("await expect({t}).toHaveURL(new RegExp({}));", lit(&escape_regex(u))));
                }
            }
            Action::OpenTab { name, url } => {
                let t = tab_ref(name);
                self.line(indent, &format!("{t} = await cur.context().newPage();"));
                self.line(indent, &format!("await {t}.goto({});", lit(&goto_target("open_tab", url, self.input.origins)?)));
                self.line(indent, &format!("claimed.add({t});"));
                self.line(indent, &format!("cur = {t};"));
            }
            Action::SwitchTab { name } => self.line(indent, &format!("cur = {};", tab_ref(name))),
            Action::CloseTab { name } => {
                let t = tab_ref(name);
                self.line(indent, &format!("await {t}.close();"));
                self.line(indent, &format!("if (cur === {t}) {{ cur = page; }}"));
            }
            Action::ExpectTabClosed { name, within_ms } => {
                let t = tab_ref(name);
                let ms = within_ms.unwrap_or(TAB_WAIT_MS);
                self.line(indent, &format!("await {t}.waitForEvent('close', {{ timeout: {ms} }});"));
                self.line(indent, &format!("if (cur === {t}) {{ cur = page; }}"));
            }
            // The table checks poll the `tables` helper, which reads the table
            // the way the app does and answers null when the check holds.
            Action::ExpectRow { table, cells, exact, timeout_ms } => self.line(
                indent,
                &format!(
                    "await expect.poll(() => tables.row({}, {}, {exact}){}).toBeNull();",
                    one(table),
                    ts_cells(&cells.0),
                    poll_opts(*timeout_ms)
                ),
            ),
            // An absence must also hold still, and a table still changing
            // when time runs out is judged on its last read: the helper runs
            // the app's own loop with the step's timeout, once.
            Action::ExpectNoRow { table, cells, exact, timeout_ms } => self.line(
                indent,
                &format!(
                    "expect(await tables.noRow({}, {}, {exact}, {})).toBeNull();",
                    one(table),
                    ts_cells(&cells.0),
                    expect_timeout(*timeout_ms)
                ),
            ),
            Action::ExpectSorted { table, column, order, sort_as, timeout_ms } => self.line(
                indent,
                &format!(
                    "await expect.poll(() => tables.sorted({}, {}, {}, {}){}).toBeNull();",
                    one(table),
                    lit(column),
                    lit(order.word()),
                    ts_sort_as(sort_as.as_ref()),
                    poll_opts(*timeout_ms)
                ),
            ),
            Action::ExpectRowCount { table, equals, at_least, at_most, timeout_ms } => {
                let want = table::row_count(*equals, *at_least, *at_most).map_err(Untranslatable)?;
                let matcher = match want {
                    RowCount::Equals(n) => format!("toBe({n})"),
                    RowCount::AtLeast(n) => format!("toBeGreaterThanOrEqual({n})"),
                    RowCount::AtMost(n) => format!("toBeLessThanOrEqual({n})"),
                };
                let line = if TableCheck::Count(want).needs_settling() {
                    // Passes on too few rows: the app's settling loop, as for
                    // noRow. Only `equals` 0 and `at_most` settle.
                    let most = match want {
                        RowCount::AtMost(n) => n,
                        _ => 0,
                    };
                    format!("expect(await tables.stillRows({}, {most}, {})).{matcher};", one(table), expect_timeout(*timeout_ms))
                } else {
                    format!("await expect.poll(() => tables.rows({}){}).{matcher};", one(table), poll_opts(*timeout_ms))
                };
                self.line(indent, &line)
            }
            // Refused by `check_action` above; written out so that a new
            // action has to be decided here.
            Action::SignIn { .. }
            | Action::UseComponent { .. }
            | Action::Upload { .. }
            | Action::ExpectDialog { .. } => {
                return check_action(a, self.input.origins)
            }
        }
        Ok(())
    }
}

/// The cells a row check looks for, in the script's order, as
/// `[['Column', 'text'], ...]`.
fn ts_cells(cells: &[(String, String)]) -> String {
    let pairs: Vec<String> = cells.iter().map(|(c, t)| format!("[{}, {}]", lit(c), lit(t))).collect();
    format!("[{}]", pairs.join(", "))
}

/// How `expect_sorted` reads the column; text when the script says nothing,
/// as in the app.
fn ts_sort_as(sort_as: Option<&SortAs>) -> String {
    match sort_as {
        None | Some(SortAs::Kind(SortKind::Text)) => "'text'".to_string(),
        Some(SortAs::Kind(SortKind::Number)) => "'number'".to_string(),
        Some(SortAs::Kind(SortKind::Date)) => "'date'".to_string(),
        Some(SortAs::Format { date }) => format!("{{ date: {} }}", lit(date)),
    }
}

fn recipe_action(s: &RecipeStep) -> Action {
    match s {
        RecipeStep::Do(a) => a.clone(),
        RecipeStep::WhenVisible(w) => Action::WhenVisible {
            selector: w.selector.clone(),
            within_ms: Some(w.within_ms),
            then: w.then.clone(),
        },
    }
}
