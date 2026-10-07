//! One Auto Run script, as the raw `.spec.ts` the `/gen-test` stage of the
//! PHR-PLAYWRIGHT-AUTOMATION repo starts from - or the reason it cannot be
//! one.
//!
//! The app acts on the FIRST match of a selector while Playwright's strict
//! mode throws when an action or a single-element expectation matches
//! several, so every such locator ends in `.first()` unless the selector
//! already picks one (`nth`, or a Legacy `text=` which ends in `.last()`).
//! Counting (`expect_count`) never gets one.

use super::ts::{lit, locator};
use crate::autorun::recipe::RecipeStep;
use crate::autorun::runner::WATCHED_DOWNLOAD_WAIT_MS;
use crate::autorun::CaseScript;
use crate::browser::actions::{Action, DropAt, TAB_WAIT_MS};
use crate::browser::locator::Target;
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
}

/// Every action of the script can be written as Playwright.
pub fn check(script: &CaseScript) -> Res<()> {
    for step in &script.steps {
        for a in &step.actions {
            check_action(a)?;
        }
    }
    Ok(())
}

fn check_action(a: &Action) -> Res<()> {
    match a {
        Action::Navigate { .. }
        | Action::CheckText { .. }
        | Action::CheckUrl { .. }
        | Action::Reload
        | Action::ExpireSession
        | Action::ReturnToArea { .. }
        | Action::ExpectTab { .. }
        | Action::SwitchTab { .. }
        | Action::ExpectTabClosed { .. }
        | Action::PressKey { .. } => Ok(()),
        Action::Click { selector }
        | Action::Fill { selector, .. }
        | Action::WaitFor { selector, .. }
        | Action::ExpectVisible { selector, .. }
        | Action::ExpectHidden { selector, .. }
        | Action::ExpectText { selector, .. }
        | Action::ExpectContainsText { selector, .. }
        | Action::ExpectCount { selector, .. }
        | Action::ExpectAttribute { selector, .. }
        | Action::ExpectFocused { selector, .. } => check_target(selector),
        Action::Drag { from, to, position, .. } => {
            if matches!(position, Some(DropAt::Before | DropAt::After)) {
                return no("drag before/after has no Playwright equivalent");
            }
            check_target(from)?;
            check_target(to)
        }
        Action::WhenVisible { selector, then, .. } => {
            check_target(selector)?;
            then.iter().try_for_each(check_action)
        }
        Action::ExpectResponse { json, .. } => match json {
            Some(v) => ts_json(v, "expect_response").map(|_| ()),
            None => Ok(()),
        },
        Action::ApiRequest { expect, .. } => match &expect.json {
            Some(v) => ts_json(v, "api_request").map(|_| ()),
            None => Ok(()),
        },
        Action::OpenTab { name, .. } | Action::CloseTab { name } => {
            if name == "main" {
                return no("open_tab/close_tab of the main tab cannot be exported");
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
        Action::ExpectDialog { .. } => {
            no("expect_dialog cannot be exported: Playwright answers dialogs with a handler, not a check at a place in the steps")
        }
        Action::ExpectRow { .. } => no("expect_row cannot be exported: Playwright has no table-row check to translate it to"),
        Action::ExpectNoRow { .. } => no("expect_no_row cannot be exported: Playwright has no table-row check to translate it to"),
        Action::ExpectSorted { .. } => no("expect_sorted cannot be exported: Playwright has no column-order check to translate it to"),
        Action::ExpectRowCount { .. } => {
            no("expect_row_count cannot be exported: Playwright has no table-row check to translate it to")
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
    check(input.script)?;
    let mut em = Em { out: String::new(), input, scope: String::new(), cnt: Counters::default(), blocks: 0 };

    let _ = write!(em.out, "// spec: {}\n", one_line(&input.md_path));
    em.out.push_str("// seed: suites/_generated/seed.spec.ts\n\n");
    em.out.push_str("import { test, expect } from '@playwright/test';\n\n");
    let _ = write!(em.out, "test.describe({}, () => {{\n", lit(&input.feature_title));
    let _ = write!(em.out, "  test({}, async ({{ page }}) => {{\n", lit(&input.script.title));

    em.line(4, "let cur = page;");
    for name in tab_names(input.script) {
        em.line(4, &format!("let tab_{name}: typeof page;"));
    }
    em.line(4, "await page.goto('/');");
    em.landing(4, input.area_clicks, "page")?;

    for step in &input.script.steps {
        em.out.push('\n');
        let n = step.step_number;
        match input.step_texts.get(&n) {
            Some(t) => em.line(4, &format!("// {n}. {}", one_line(t))),
            None => em.line(4, &format!("// {n}.")),
        }
        if let Some(why) = &step.unchecked {
            em.line(4, &format!("// Not checked: {}", one_line(why)));
        }
        em.block(n.to_string(), &step.actions, 4)?;
    }

    em.out.push_str("  });\n});\n");
    Ok(em.out)
}

/// Comment text must stay on its line: a line terminator would end the
/// comment (U+2028 and U+2029 are line terminators in JavaScript).
fn one_line(s: &str) -> String {
    s.chars().map(|c| if matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}') { ' ' } else { c }).collect()
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

/// Tabs the script gives a name to (`expect_tab`, `open_tab`), once each,
/// in the order they first appear.
fn tab_names(script: &CaseScript) -> Vec<String> {
    fn walk(a: &Action, out: &mut Vec<String>) {
        match a {
            Action::ExpectTab { name, .. } | Action::OpenTab { name, .. } => {
                let s = sanitize(name);
                if !out.contains(&s) {
                    out.push(s);
                }
            }
            Action::WhenVisible { then, .. } => then.iter().for_each(|a| walk(a, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in &script.steps {
        s.actions.iter().for_each(|a| walk(a, &mut out));
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

fn wait_opts(t: &Target, timeout: u32) -> String {
    if matches_hidden(t) {
        format!("{{ state: 'attached', timeout: {timeout} }}")
    } else {
        format!("{{ timeout: {timeout} }}")
    }
}

fn call(name: &str, args: &[String], timeout: Option<u32>) -> String {
    let mut all: Vec<String> = args.to_vec();
    if let Some(t) = timeout {
        all.push(format!("{{ timeout: {t} }}"));
    }
    format!("{name}({})", all.join(", "))
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

/// A script's address as a page address: a full address keeps only its
/// path, query and fragment, so the spec runs against whichever
/// environment the repo points at.
fn goto_target(url: &str) -> String {
    let u = url.trim();
    if let Some(i) = u.find("://") {
        let rest = &u[i + 3..];
        return match rest.find(['/', '?', '#']) {
            Some(j) if rest[j..].starts_with('/') => rest[j..].to_string(),
            Some(j) => format!("/{}", &rest[j..]),
            None => "/".to_string(),
        };
    }
    u.to_string()
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

fn key_name(key: &str) -> String {
    key.split('+').map(|p| if p == "Ctrl" { "Control" } else { p }).collect::<Vec<_>>().join("+")
}

// ---------------------------------------------------------------------
// Emission

#[derive(Default)]
struct Counters {
    resp: usize,
    dl: usize,
    tab: usize,
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

    fn arm(&mut self, actions: &[Action], indent: usize, n: &mut Counters) {
        for a in actions {
            let sc = self.scope.clone();
            match a {
                Action::ExpectResponse { method, url_contains, timeout_ms, .. } => {
                    let mut test = format!("r.url().toLowerCase().includes({})", lit(&url_contains.to_lowercase()));
                    if let Some(m) = method {
                        let _ = write!(test, " && r.request().method() === {}", lit(&m.to_uppercase()));
                    }
                    let opt = timeout_ms.map(|t| format!(", {{ timeout: {t} }}")).unwrap_or_default();
                    let i = n.resp;
                    n.resp += 1;
                    self.line(indent, &format!("const resp{sc}_{i} = cur.waitForResponse(r => {test}{opt});"));
                }
                Action::ExpectDownload { within_ms, .. } => {
                    let i = n.dl;
                    n.dl += 1;
                    let ms = within_ms.unwrap_or(WATCHED_DOWNLOAD_WAIT_MS);
                    self.line(indent, &format!("const dl{sc}_{i} = cur.waitForEvent('download', {{ timeout: {ms} }});"));
                }
                Action::ExpectTab { within_ms, .. } => {
                    let i = n.tab;
                    n.tab += 1;
                    let ms = within_ms.unwrap_or(TAB_WAIT_MS);
                    self.line(indent, &format!("const tabp{sc}_{i} = cur.context().waitForEvent('page', {{ timeout: {ms} }});"));
                }
                Action::WhenVisible { then, .. } => self.arm(then, indent, n),
                _ => {}
            }
        }
    }

    fn action(&mut self, a: &Action, indent: usize) -> Res<()> {
        check_action(a)?;
        let sc = self.scope.clone();
        match a {
            Action::Navigate { url } => self.line(indent, &format!("await cur.goto({});", lit(&goto_target(url)))),
            Action::Click { selector } => self.line(indent, &format!("await {}.click();", one(selector))),
            Action::Fill { selector, value } => {
                self.line(indent, &format!("await {}.fill({});", one(selector), lit(value)))
            }
            Action::WaitFor { selector, timeout_ms } => {
                self.line(indent, &format!("await {}.waitFor({});", one(selector), wait_opts(selector, *timeout_ms)))
            }
            Action::CheckText { value } => self.line(
                indent,
                &format!("await expect(cur.locator('body')).toContainText({}, {{ ignoreCase: true }});", lit(value)),
            ),
            Action::CheckUrl { contains } => self.line(
                indent,
                &format!("await expect(cur).toHaveURL(new RegExp({}));", lit(&escape_regex(contains))),
            ),
            Action::ExpectVisible { selector, timeout_ms } => {
                self.line(indent, &format!("await expect({}).{};", one(selector), call("toBeVisible", &[], *timeout_ms)))
            }
            Action::ExpectHidden { selector, timeout_ms } => {
                self.line(indent, &format!("await expect({}).{};", one(selector), call("toBeHidden", &[], *timeout_ms)))
            }
            Action::ExpectText { selector, equals, timeout_ms } => self.line(
                indent,
                &format!("await expect({}).{};", one(selector), call("toHaveText", &[lit(equals)], *timeout_ms)),
            ),
            Action::ExpectContainsText { selector, value, timeout_ms } => self.line(
                indent,
                &format!("await expect({}).{};", one(selector), call("toContainText", &[lit(value)], *timeout_ms)),
            ),
            Action::ExpectCount { selector, equals, timeout_ms } => self.line(
                indent,
                &format!(
                    "await expect({}).{};",
                    locator("cur", selector),
                    call("toHaveCount", &[equals.to_string()], *timeout_ms)
                ),
            ),
            Action::ExpectAttribute { selector, name, equals, timeout_ms } => self.line(
                indent,
                &format!(
                    "await expect({}).{};",
                    one(selector),
                    call("toHaveAttribute", &[lit(name), lit(equals)], *timeout_ms)
                ),
            ),
            Action::ExpectFocused { selector, timeout_ms } => {
                self.line(indent, &format!("await expect({}).{};", one(selector), call("toBeFocused", &[], *timeout_ms)))
            }
            Action::WhenVisible { selector, within_ms, then } => {
                let w = within_ms.unwrap_or(2000);
                self.line(
                    indent,
                    &format!(
                        "if (await {}.waitFor({}).then(() => true, () => false)) {{",
                        one(selector),
                        wait_opts(selector, w)
                    ),
                );
                for a in then {
                    self.action(a, indent + 2)?;
                }
                self.line(indent, "}");
            }
            Action::ExpectResponse { status, json, .. } => {
                let i = self.cnt.resp;
                self.cnt.resp += 1;
                self.line(indent, &format!("const r{sc}_{i} = await resp{sc}_{i};"));
                self.line(indent, &format!("expect(r{sc}_{i}.status()).toBe({status});"));
                if let Some(j) = json {
                    self.line(
                        indent,
                        &format!("expect(await r{sc}_{i}.json()).toMatchObject({});", ts_json(j, "expect_response")?),
                    );
                }
            }
            Action::ApiRequest { path, query, expect, .. } => {
                let i = self.cnt.api;
                self.cnt.api += 1;
                let params = if query.is_empty() {
                    String::new()
                } else {
                    let kv: Vec<String> = query.iter().map(|(k, v)| format!("{}: {}", lit(k), lit(v))).collect();
                    format!(", {{ params: {{ {} }} }}", kv.join(", "))
                };
                self.line(indent, &format!("const a{sc}_{i} = await cur.request.get({}{params});", lit(path)));
                self.line(indent, &format!("expect(a{sc}_{i}.status()).toBe({});", expect.status));
                if let Some(j) = &expect.json {
                    self.line(
                        indent,
                        &format!("expect(await a{sc}_{i}.json()).toMatchObject({});", ts_json(j, "api_request")?),
                    );
                }
            }
            Action::Reload => self.line(indent, "await cur.reload();"),
            Action::ExpireSession => self.line(indent, "await cur.context().clearCookies();"),
            Action::ReturnToArea { area } => {
                let clicks: Vec<Target> = match area {
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
                    self.line(indent, &format!("await cur.keyboard.press({});", lit(&key_name(key))));
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
            Action::ExpectTab { name, url_contains, .. } => {
                let k = self.cnt.tab;
                self.cnt.tab += 1;
                let t = tab_ref(name);
                self.line(indent, &format!("{t} = await tabp{sc}_{k};"));
                if let Some(u) = url_contains {
                    self.line(indent, &format!("await expect({t}).toHaveURL(new RegExp({}));", lit(&escape_regex(u))));
                }
            }
            Action::OpenTab { name, url } => {
                let t = tab_ref(name);
                self.line(indent, &format!("{t} = await cur.context().newPage();"));
                self.line(indent, &format!("await {t}.goto({});", lit(&goto_target(url))));
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
            // Refused by `check_action` above; written out so that a new
            // action has to be decided here.
            Action::SignIn { .. }
            | Action::Upload { .. }
            | Action::ExpectDialog { .. }
            | Action::ExpectRow { .. }
            | Action::ExpectNoRow { .. }
            | Action::ExpectSorted { .. }
            | Action::ExpectRowCount { .. } => return check_action(a),
        }
        Ok(())
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
