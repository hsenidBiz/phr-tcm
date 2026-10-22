//! One run as a single HTML page a person can keep, print or attach to an
//! email: when it ran, where, what each case came to, and for each case
//! that failed or was blocked, the step that stopped it with its pictures.
//!
//! The page is opened in the browser from `<root>/reports/`: inline CSS, no
//! script, nothing fetched. Every case has its own collapsible section with
//! every step the run recorded, and the pictures are linked, not embedded -
//! `../shots/<name>`, beside the reports folder - so a long unattended run
//! stays a small file. Every value from the run is escaped: a case title is
//! whatever someone typed into Azure DevOps.
//!
//! What never goes in: passwords, the values a script typed (`fill`'s
//! value), cookies, headers, or any file's bytes. A picture is linked only
//! for a name `store`'s screenshot guard accepts and that is in the shots
//! folder; any other name is a note, never a link. The accounts are their
//! keys, as the run recorded them, never logins. A case's downloads are a
//! `Downloads:` line of names and sizes: never a link, never the folder.

use super::components::{ran_actions, ComponentFile, Ran};
use super::replay::{MODULE_STEP, SIGN_IN_STEP};
use super::divergences::{Divergence, DivergenceStatus, RuledBy, Ruling};
use super::unchecked::{count_words, expectations, line as unchecked_line};
use super::{CaseRecord, CaseScript, LocalRun, ResetRecord, StepRecord};
use crate::browser::actions::{Action, ActionOutcome, DialogAnswer};
use crate::html_escape::esc_attr;

/// The result buckets, in the order the report (and the Auto Run screen's
/// filter row) lists them.
pub const BUCKETS: [&str; 4] = ["Passed", "Failed", "Blocked", "Not run"];

/// Which bucket a case's result falls in: the person's confirmed `verdict`
/// when set, else the machine's `proposed`; exactly "Passed", "Failed" or
/// "Blocked" is its own bucket, anything else (nothing proposed, or a word
/// that is none of the three) is "Not run".
///
/// The Auto Run screen filters with its own copy of this rule
/// (`resultBucket` in src/screens/AutoRun/verdicts.ts). Both are held to
/// one table, tests/fixtures/verdict_buckets.json, so the screen and this
/// report can never count a case differently.
pub fn bucket(verdict: &str, proposed: &str) -> &'static str {
    let word = if verdict.is_empty() { proposed } else { verdict };
    match word {
        "Passed" => "Passed",
        "Failed" => "Failed",
        "Blocked" => "Blocked",
        _ => "Not run",
    }
}

/// `bucket` for one case.
pub fn case_bucket(case: &CaseRecord) -> &'static str {
    bucket(&case.verdict, &case.proposed)
}

/// How many of the run's cases fall in each bucket, in `BUCKETS` order.
pub fn counts(run: &LocalRun) -> [usize; 4] {
    let mut out = [0usize; 4];
    for case in &run.cases {
        let b = case_bucket(case);
        if let Some(i) = BUCKETS.iter().position(|x| *x == b) {
            out[i] += 1;
        }
    }
    out
}

/// What a step's line is called - the same words the review dialog uses.
pub fn step_label(step_number: i32) -> String {
    match step_number {
        SIGN_IN_STEP => "Sign in".to_string(),
        MODULE_STEP => "Module".to_string(),
        n => format!("Step {n}"),
    }
}

/// An action said in words. Never a `fill`'s value - that is what the
/// script typed, and may be anything; the field it went into is enough.
pub fn action_words(action: &Action) -> String {
    match action {
        Action::Navigate { url } => format!("go to {}", without_query(url)),
        Action::Click { selector } => format!("click {}", selector.describe()),
        Action::Fill { selector, .. } => format!("fill in {}", selector.describe()),
        Action::WaitFor { selector, .. } => format!("wait for {}", selector.describe()),
        Action::CheckText { value } => format!("check the page says \"{value}\""),
        Action::CheckUrl { equals: Some(want), .. } => {
            format!("expect the address to be \"{}\"", crate::browser::actions::path_only(want))
        }
        Action::CheckUrl { same_as: Some(name), .. } => {
            format!("expect the address to be the one noted as \"{name}\"")
        }
        Action::CheckUrl { contains, .. } => format!("check the address contains \"{contains}\""),
        Action::RememberUrl { name } => format!("note the address as \"{name}\""),
        Action::ExpectVisible { selector, .. } => format!("expect {} to be visible", selector.describe()),
        Action::ExpectHidden { selector, .. } => format!("expect {} to be hidden", selector.describe()),
        Action::ExpectText { selector, matches: Some(p), .. } => {
            format!("expect {} to match {}", selector.describe(), crate::browser::pattern::shown(p))
        }
        Action::ExpectText { selector, equals, .. } => {
            format!("expect {} to read \"{}\"", selector.describe(), equals.as_deref().unwrap_or(""))
        }
        Action::ExpectContainsText { selector, value, .. } => {
            format!("expect {} to contain \"{value}\"", selector.describe())
        }
        Action::ExpectCount { selector, at_least: Some(n), .. } => {
            format!("expect at least {n} of {}", selector.describe())
        }
        Action::ExpectCount { selector, at_most: Some(n), .. } => {
            format!("expect at most {n} of {}", selector.describe())
        }
        Action::ExpectCount { selector, equals, .. } => {
            format!("expect {} of {}", equals.unwrap_or(0), selector.describe())
        }
        Action::ExpectAttribute { selector, name, matches: Some(p), .. } => {
            format!("expect {}'s {name} to match {}", selector.describe(), crate::browser::pattern::shown(p))
        }
        Action::ExpectAttribute { selector, name, equals: Some(equals), .. } => {
            format!("expect {}'s {name} to be \"{equals}\"", selector.describe())
        }
        Action::ExpectAttribute { selector, name, equals: None, .. } => {
            format!("expect {} to have no {name}", selector.describe())
        }
        Action::ExpectStyle { selector, property, equals, contains, not_equals, .. } => {
            let how = match (equals, contains, not_equals) {
                (Some(v), _, _) => format!("to be \"{v}\""),
                (_, Some(v), _) => format!("to contain \"{v}\""),
                (_, _, Some(v)) => format!("not to be \"{v}\""),
                _ => "to be checked".to_string(),
            };
            format!("expect {}'s {} {how}", selector.describe(), property.trim())
        }
        Action::ExpectEnabled { selector, .. } => format!("expect {} to be enabled", selector.describe()),
        Action::ExpectDisabled { selector, .. } => format!("expect {} to be disabled", selector.describe()),
        Action::ExpectChecked { selector, checked, .. } => {
            format!("expect {} to be {}", selector.describe(), if *checked { "checked" } else { "not checked" })
        }
        Action::SignIn { account } => format!("sign in as {account}"),
        Action::Upload { selector, file } => format!("upload {file} into {}", selector.describe()),
        Action::ExpectResponse { method, url_contains, status, .. } => {
            let which = method.as_deref().map(|m| format!("{} ", m.trim().to_ascii_uppercase())).unwrap_or_default();
            format!("expect a {which}request to {} to answer {status}", without_query(url_contains))
        }
        Action::ApiRequest { path, expect, .. } => {
            format!("ask {} and expect {}", without_query(path), expect.status)
        }
        Action::WhenVisible { selector, then, .. } => {
            let what: Vec<String> = then.iter().map(action_words).collect();
            format!("if {} shows up, {}", selector.describe(), what.join(", then "))
        }
        Action::RepeatWhileVisible { selector, max, except, then } => {
            let what: Vec<String> = then.iter().map(action_words).collect();
            let keep = if except.is_empty() { String::new() } else { format!(" (keeping {})", except.join(", ")) };
            format!(
                "while {} shows{keep}, {} - at most {} times",
                selector.describe(),
                what.join(", then "),
                max.unwrap_or(0)
            )
        }
        Action::UseComponent { component, .. } => format!("run the component \"{}\"", component.trim()),
        Action::Reload => "reload the page".to_string(),
        Action::ExpireSession => "end the session".to_string(),
        Action::ReturnToArea { .. } => match action.area_named() {
            Some(area) => format!("go to the {area} area"),
            None => "go back to the case's area".to_string(),
        },
        Action::PressKey { key, times } => match times {
            Some(n) if *n > 1 => format!("press {} {n} times", key.trim()),
            _ => format!("press {}", key.trim()),
        },
        Action::ExpectEach { selector, min, child, attribute, visible, equals, contains, matches, .. } => {
            let name = attribute.as_deref().unwrap_or("").trim();
            let to = if *visible {
                "be visible".to_string()
            } else if let Some(w) = equals {
                match name {
                    "" => format!("read \"{w}\""),
                    _ => format!("have {name}=\"{w}\""),
                }
            } else if let Some(w) = contains {
                match name {
                    "" => format!("contain \"{w}\""),
                    _ => format!("have {name} containing \"{w}\""),
                }
            } else if let Some(p) = matches {
                let p = crate::browser::pattern::shown(p);
                match name {
                    "" => format!("match {p}"),
                    _ => format!("have {name} matching {p}"),
                }
            } else if name.is_empty() {
                "have text".to_string()
            } else {
                format!("have a {name}")
            };
            let at_least = match min {
                Some(n) if *n > 1 => format!(" (at least {n})"),
                _ => String::new(),
            };
            match child {
                Some(c) => format!("expect {} in each of {} to {to}{at_least}", c.describe(), selector.describe()),
                None => format!("expect each of {} to {to}{at_least}", selector.describe()),
            }
        }
        Action::ExpectOrder { selector, texts, .. } => {
            let texts: Vec<String> = texts.iter().map(|t| format!("\"{t}\"")).collect();
            format!("expect {} to list {} in that order", selector.describe(), texts.join(", "))
        }
        Action::ExpectPosition { selector, relative_to, relation, .. } => {
            format!("expect {} to be {} {}", selector.describe(), relation.words(), relative_to.describe())
        }
        Action::PageUntil { next, until, until_all, max_pages } => {
            let names: Vec<String> = crate::browser::actions::page_goals(until, until_all).iter().map(|u| u.describe()).collect();
            format!("page with {} until {} show, at most {max_pages} pages", next.describe(), names.join(" and "))
        }
        Action::ExpectDialog { text, contains, answer, prompt_text, .. } => {
            let said = match (text, contains) {
                (Some(t), _) => format!(" saying \"{t}\""),
                (None, Some(c)) => format!(" containing \"{c}\""),
                (None, None) => String::new(),
            };
            let then = match (answer, prompt_text) {
                (DialogAnswer::Accept, Some(p)) => format!(", type \"{p}\" and press OK"),
                (DialogAnswer::Accept, None) => " and press OK".to_string(),
                (DialogAnswer::Dismiss, _) => " and press Cancel".to_string(),
            };
            format!("expect a dialog{said}{then}")
        }
        Action::ExpectRow { table, cells, .. } => {
            format!("expect a row in {} with {}", table.describe(), crate::browser::table::cells_words(&cells.0))
        }
        Action::ExpectNoRow { table, cells, .. } => {
            format!("expect no row in {} with {}", table.describe(), crate::browser::table::cells_words(&cells.0))
        }
        Action::ExpectSorted { table, column, order, .. } => {
            format!("expect {} sorted by {}, {}", table.describe(), column.trim(), order.word())
        }
        Action::ExpectRowCount { table, equals, at_least, at_most, .. } => {
            let n = match (equals, at_least, at_most) {
                (Some(n), _, _) => n.to_string(),
                (None, Some(n), _) => format!("at least {n}"),
                (None, None, Some(n)) => format!("at most {n}"),
                _ => "some".to_string(),
            };
            format!("expect {} to have {n} rows", table.describe())
        }
        Action::Drag { from, to, position, .. } => format!(
            "drag {} {} {}",
            from.describe(),
            position.unwrap_or_default().word(),
            to.describe()
        ),
        Action::ExpectFocused { selector, .. } => format!("expect {} to have the focus", selector.describe()),
        Action::ExpectDownload { name, .. } => format!("expect a download named \"{}\"", name.trim()),
        Action::ExpectTab { name, .. } => format!("wait for a new tab and call it \"{name}\""),
        Action::OpenTab { name, url } => {
            format!("open a new tab \"{name}\" at {}", crate::browser::actions::path_only(url))
        }
        Action::SwitchTab { name } => format!("switch to the \"{name}\" tab"),
        Action::CloseTab { name } => format!("close the \"{name}\" tab"),
        Action::ExpectTabClosed { name, .. } => format!("check the \"{name}\" tab closes"),
    }
}

/// An address without its query string or fragment: a script's URL can
/// carry a token there, and the report is a file people pass around.
pub fn without_query(url: &str) -> &str {
    match url.find(['?', '#']) {
        Some(i) => &url[..i],
        None => url,
    }
}

/// A sentence the run recorded, with the query string and fragment taken
/// off every `http://`, `https://` or `file://` address in it. The runner
/// writes whole addresses into some of its sentences ("loaded <url>"), and a
/// query string can carry a token; the report is a file people pass around.
pub fn scrub_urls(text: &str) -> String {
    const SCHEMES: [&str; 3] = ["http://", "https://", "file://"];
    // ASCII lowercasing keeps every byte offset, so indexes carry over.
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let next = SCHEMES.iter().filter_map(|s| lower[i..].find(s)).min();
        let Some(rel) = next else {
            out.push_str(&text[i..]);
            break;
        };
        let start = i + rel;
        out.push_str(&text[i..start]);
        let end = text[start..]
            .find(|c: char| c.is_whitespace() || "\"'<>)]}".contains(c))
            .map_or(text.len(), |n| start + n);
        out.push_str(without_query(&text[start..end]));
        i = end;
    }
    out
}

/// `started_at` (epoch milliseconds as a string) as a UTC time - only for
/// when the webview sent no local time to print instead.
fn utc_time(started_at: &str) -> String {
    let Ok(ms) = started_at.trim().parse::<i64>() else {
        return "unknown time".to_string();
    };
    if ms <= 0 {
        return "unknown time".to_string();
    }
    let secs = ms / 1000;
    let (y, mo, d) = crate::applog::civil(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!("{y:04}-{mo:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}

/// Where the case stopped: the first action that failed and was not merely
/// skipped because an earlier one had - its step, its index in that step,
/// and what the page said.
fn stopping_point(case: &CaseRecord) -> Option<(&StepRecord, usize, &ActionOutcome)> {
    for step in &case.steps {
        for (i, o) in step.outcomes.iter().enumerate() {
            if !o.ok && !o.detail.starts_with("not run:") {
                return Some((step, i, o));
            }
        }
    }
    None
}

/// The words for the action that stopped a case. The sign-in and the trip
/// to the module are the runner's own, not the script's.
fn stopped_action(
    case: &CaseRecord,
    step: &StepRecord,
    index: usize,
    script: Option<&CaseScript>,
    components: &ComponentFile,
) -> String {
    match step.step_number {
        SIGN_IN_STEP => match &case.account {
            Some(a) => format!("sign in as {a}"),
            None => "sign in".to_string(),
        },
        MODULE_STEP => "go to the case's module".to_string(),
        n => match script {
            None => "(the script for this case is no longer on this machine)".to_string(),
            // Paired with what ran, a component's actions expanded.
            Some(s) => {
                let ran = s
                    .steps
                    .iter()
                    .find(|st| st.step_number == n)
                    .map(|st| ran_actions(&st.actions, &step.outcomes, components, &step.components))
                    .unwrap_or_default();
                match ran.get(index) {
                    Some(Ran { action: Some(a), component: Some(c) }) => format!("{c}: {}", action_words(a)),
                    Some(Ran { action: Some(a), component: None }) => action_words(a),
                    Some(Ran { action: None, component: Some(c) }) => {
                        format!("{c}: action {} (the component on this machine has changed since the run)", index + 1)
                    }
                    _ => format!("action {} (the script on this machine has changed since the run)", index + 1),
                }
            }
        },
    }
}

/// One picture as a linked `<img>`, or a line saying why it is not there.
/// The page lives in `<root>/reports/` and the pictures in `<root>/shots/`,
/// so a picture is `../shots/<name>`. `exists` is only ever asked about a
/// name the screenshot guard accepts - a name that could point anywhere but
/// the shots folder is never linked and never looked up. `alt` says which
/// step (and action) the picture is of.
fn picture(name: &str, alt: &str, exists: &dyn Fn(&str) -> bool) -> String {
    if !super::store::safe_shot_name(name) {
        return format!(
            "<p class=\"note\">A picture was skipped: {} is not a screenshot name.</p>",
            esc_attr(name)
        );
    }
    if !exists(name) {
        return format!(
            "<p class=\"note\">The picture {} was not found - it is no longer on this machine.</p>",
            esc_attr(name)
        );
    }
    format!(
        "<figure><img loading=\"lazy\" src=\"../shots/{}\" alt=\"{}\"><figcaption>{}</figcaption></figure>",
        esc_attr(name),
        esc_attr(alt),
        esc_attr(name)
    )
}

/// The "step that stopped it" summary at the top of a Failed or Blocked
/// case's section: why, the step and action in words, the page's message.
/// Its pictures are not repeated here - the step list below shows them.
fn stopped_summary(case: &CaseRecord, script: Option<&CaseScript>, components: &ComponentFile) -> String {
    let mut h = String::new();
    if !case.reason.is_empty() {
        h.push_str(&format!("<p><strong>Why:</strong> {}</p>", esc_attr(&scrub_urls(&case.reason))));
    }
    match stopping_point(case) {
        Some((step, index, outcome)) => {
            h.push_str("<dl>");
            h.push_str(&format!("<dt>Stopped at</dt><dd>{}</dd>", esc_attr(&step_label(step.step_number))));
            h.push_str(&format!(
                "<dt>Action</dt><dd>{}</dd>",
                esc_attr(&stopped_action(case, step, index, script, components))
            ));
            h.push_str(&format!("<dt>Message</dt><dd>{}</dd>", esc_attr(&scrub_urls(&outcome.detail))));
            h.push_str("</dl>");
        }
        None => h.push_str(
            "<p class=\"note\">No step failed on its own - this result was decided by the person reviewing the run.</p>",
        ),
    }
    h
}

/// Milliseconds as `2.3 s`.
fn seconds(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// Every step of a case in the order it ran: its label, each action's
/// outcome as a line marked with a tick or a cross and the sentence the run
/// recorded (never anything from the script), then the step's picture and
/// any picture an action took on failing. A picture named twice in one step
/// is shown once. The case's clean-up comes last, named Clean-up.
fn steps_block(case: &CaseRecord, exists: &dyn Fn(&str) -> bool) -> String {
    if case.steps.is_empty() {
        return "<p class=\"note\">No steps were recorded for this case.</p>".to_string();
    }
    let mut h = String::new();
    let labelled = case.steps.iter().map(|s| (step_label(s.step_number), s));
    let tidy = case.cleanup.iter().map(|s| (super::CLEANUP_LABEL.to_string(), s));
    for (label, step) in labelled.chain(tidy) {
        h.push_str("<div class=\"step\">");
        // How long the step took, when the run timed it.
        let took = step.duration_ms.map(|ms| format!(" <span class=\"took\">{}</span>", seconds(ms))).unwrap_or_default();
        h.push_str(&format!("<h4>{}{took}</h4>", esc_attr(&label)));
        if step.outcomes.is_empty() {
            h.push_str("<p class=\"note\">No actions were recorded for this step.</p>");
        } else {
            h.push_str("<ul class=\"actions\">");
            for o in &step.outcomes {
                let (class, mark) = if o.ok { ("ok", "\u{2713}") } else { ("bad", "\u{2717}") };
                // An action a component ran says which component.
                let from = o.component.as_deref().map(|c| format!("{}: ", esc_attr(c))).unwrap_or_default();
                h.push_str(&format!("<li class=\"{class}\">{mark} {from}{}</li>", esc_attr(&scrub_urls(&o.detail))));
            }
            h.push_str("</ul>");
        }
        let mut seen = std::collections::HashSet::new();
        for (i, o) in step.outcomes.iter().enumerate() {
            if let Some(n) = o.screenshot.as_deref() {
                if seen.insert(n) {
                    h.push_str(&picture(n, &format!("{label}, action {}", i + 1), exists));
                }
            }
        }
        if let Some(n) = step.screenshot.as_deref() {
            if seen.insert(n) {
                h.push_str(&picture(n, &label, exists));
            }
        }
        h.push_str("</div>");
    }
    h
}

/// The files a case's steps saved, in the order the steps ran.
pub fn case_downloads(case: &CaseRecord) -> Vec<&str> {
    case.steps.iter().flat_map(|s| s.downloads.iter().map(String::as_str)).collect()
}

/// `<p><strong>Downloads:</strong> a.xlsx (5.3 KB), b.csv (1.1 KB)</p>`, or
/// nothing for a case that saved no file. A name `size_of` has no size for
/// is a file no longer on this machine.
fn downloads_line(case: &CaseRecord, size_of: &dyn Fn(&str) -> Option<u64>) -> String {
    let names = case_downloads(case);
    if names.is_empty() {
        return String::new();
    }
    let each: Vec<String> = names
        .iter()
        .map(|n| {
            let size = size_of(n).map_or_else(|| "no longer on this machine".to_string(), crate::test_files::human_size);
            format!("{} ({size})", esc_attr(n))
        })
        .collect();
    format!("<p><strong>Downloads:</strong> {}</p>", each.join(", "))
}

/// A case's result as the report words it: its bucket, and for a pass that
/// leaves expectations unchecked, how many ("Passed, 2 expectations
/// unchecked"). Any other bucket is its word alone.
fn result_words(bucket: &str, unchecked: usize) -> String {
    if bucket == "Passed" && unchecked > 0 {
        format!("{bucket}, {}", count_words(unchecked))
    } else {
        bucket.to_string()
    }
}

/// One case's collapsible section. Open for Failed and Blocked, closed for
/// Passed and Not run - a reader lands on what needs them and can still
/// expand the rest. `<details>` needs no script.
fn case_section(
    case: &CaseRecord,
    script: Option<&CaseScript>,
    components: &ComponentFile,
    exists: &dyn Fn(&str) -> bool,
    size_of: &dyn Fn(&str) -> Option<u64>,
    divergences: &[&Divergence],
) -> String {
    let b = case_bucket(case);
    let unchecked = script.map(expectations).unwrap_or_default();
    let result = result_words(b, unchecked.len());
    let open = if matches!(b, "Failed" | "Blocked") { " open" } else { "" };
    let mut h = String::new();
    h.push_str(&format!("<details class=\"case\"{open}>"));
    // Run a second time after a transient failure (`transient`).
    let retried = if case.retried.is_some() { " <span class=\"retried\">Retried</span>" } else { "" };
    // Page errors a flagging script counted (`page_errors`).
    let flagged = match case.page_errors_seen {
        0 => String::new(),
        n => format!(" <span class=\"retried\">page errors seen: {n}</span>"),
    };
    h.push_str(&format!(
        "<summary><span class=\"id\">#{}</span> {} <span class=\"b-{}\">{}</span>{retried}{flagged}</summary>",
        case.case_id,
        esc_attr(&case.title),
        css_key(b),
        esc_attr(&result)
    ));
    if matches!(b, "Failed" | "Blocked") {
        h.push_str(&stopped_summary(case, script, components));
    } else if !case.reason.is_empty() {
        h.push_str(&format!("<p><strong>Why:</strong> {}</p>", esc_attr(&scrub_urls(&case.reason))));
    }
    if let Some(first) = &case.retried {
        h.push_str(&format!("<p><strong>Retried:</strong> the first try failed: {}</p>", esc_attr(&scrub_urls(first))));
    }
    // Something the run skipped for this case, and the case went on.
    if let Some(notice) = &case.notice {
        h.push_str(&format!("<p><strong>Notice:</strong> {}</p>", esc_attr(notice)));
    }
    for bug in &case.product_bugs {
        h.push_str(&format!("<p><strong>Product bug:</strong> {}</p>", esc_attr(bug)));
    }
    for d in divergences {
        h.push_str(&format!("<p><strong>Divergence:</strong> {}</p>", esc_attr(&divergence_words(d))));
    }
    // What the script's checks leave out, whatever the case's own verdict.
    for e in &unchecked {
        h.push_str(&format!("<p><strong>Unchecked:</strong> {}</p>", esc_attr(&unchecked_line(e))));
    }
    // A clean-up that failed, whatever the case's own verdict.
    if let Some(line) = case.cleanup_line() {
        let what = line.strip_prefix("clean-up: ").unwrap_or(&line);
        h.push_str(&format!("<p><strong>Clean-up:</strong> {}</p>", esc_attr(&scrub_urls(what))));
    }
    if !case.note.is_empty() {
        h.push_str(&format!("<p><strong>Note:</strong> {}</p>", esc_attr(&case.note)));
    }
    if let Some(phases) = &case.phases {
        h.push_str(&format!("<p class=\"took\">{}</p>", esc_attr(&phases.line())));
    }
    h.push_str(&downloads_line(case, size_of));
    h.push_str(&steps_block(case, exists));
    h.push_str("</details>");
    h
}

/// The lines a pause the run made is shown with: one per setting to set
/// (`Before Batch 1: make sure <name> is <value>`, later `Set <name> to
/// <value>`), then one per name to revert (`Reset: revert "<name>"`), each
/// ending ` - continued` (or `- stopped`). An outcome that is neither is
/// left off.
pub fn reset_lines(reset: &ResetRecord) -> Vec<String> {
    let ended = match reset.outcome.as_str() {
        o @ (super::RESET_CONTINUED | super::RESET_STOPPED) => format!(" - {o}"),
        _ => String::new(),
    };
    let set = reset.set.iter().map(|(name, value)| {
        if reset.batch == 1 {
            format!("Before Batch 1: make sure {name} is {value}{ended}")
        } else {
            format!("Set {name} to {value}{ended}")
        }
    });
    set.chain(reset.names.iter().map(|name| format!("Reset: revert \"{name}\"{ended}"))).collect()
}

/// Whether a run's cases are shown under batch headings: when it recorded
/// a pause with its batch (runs saved before batches never did).
pub fn in_batches(run: &LocalRun) -> bool {
    run.resets.iter().any(|r| r.batch > 0)
}

/// The batch heading due before case `case_id`, if one starts there:
/// `Batch N` for a recorded pause, and `Batch 1` before the first case of
/// a run shown in batches.
pub fn batch_heading(run: &LocalRun, case_id: i32) -> Option<String> {
    if !in_batches(run) {
        return None;
    }
    if let Some(r) = run.resets.iter().find(|r| r.before_case_id == case_id && r.batch > 0) {
        return Some(format!("Batch {}", r.batch));
    }
    (run.cases.first().map(|c| c.case_id) == Some(case_id)).then(|| "Batch 1".to_string())
}

/// The pause lines due before case `case_id`, escaped.
fn resets_before(run: &LocalRun, case_id: i32) -> Vec<String> {
    run.resets.iter().filter(|r| r.before_case_id == case_id).flat_map(reset_lines).map(|l| esc_attr(&l)).collect()
}

/// How many product bugs the top list names before it says "and N more".
pub const MAX_BUG_LINES: usize = 10;

/// How long after its ruling a divergence still shows in a run's report.
const RULED_SHOWN_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// What the report says of one divergence, unescaped.
fn divergence_words(d: &Divergence) -> String {
    let status = match (d.status, d.ruling) {
        (DivergenceStatus::Open, _) => "needs a ruling",
        (DivergenceStatus::AwaitingRepair, _) => "waits for a repair",
        (DivergenceStatus::Ruled, Some(Ruling::DataFixed)) => "ruled: the data was fixed",
        (DivergenceStatus::Ruled, Some(Ruling::Rewrite)) => "ruled: the case is being rewritten",
        (DivergenceStatus::Ruled, _) => "ruled: the case stands",
    };
    let by = match (d.ruled_by, d.said.as_deref()) {
        (Some(RuledBy::Assistant), said) => {
            format!(", ruled by the assistant: the person said \"{}\"", said.unwrap_or(""))
        }
        _ => String::new(),
    };
    format!(
        "step {}: the case says \"{}\", the app shows \"{}\" ({}, {status}{by})",
        d.step,
        d.case_says,
        d.app_shows,
        d.kind.word()
    )
}

/// The divergences of `case_id` the report shows: open or awaiting a
/// repair, or ruled in the week before the run started or after.
fn shown_divergences<'a>(run: &LocalRun, extras: &'a ReportExtras, case_id: i32) -> Vec<&'a Divergence> {
    let started: i64 = run.started_at.trim().parse().unwrap_or(0);
    extras
        .divergences
        .iter()
        .filter(|d| d.case_id == case_id)
        .filter(|d| match d.status {
            DivergenceStatus::Open | DivergenceStatus::AwaitingRepair => true,
            DivergenceStatus::Ruled => {
                d.ruled_at.as_deref().and_then(|s| s.parse::<i64>().ok()).is_some_and(|at| at >= started - RULED_SHOWN_MS)
            }
        })
        .collect()
}

/// The top section: each distinct product bug the run's cases work around
/// and the cases it affects, at most `MAX_BUG_LINES` lines. Empty when no
/// case carries one.
fn product_bugs_section(run: &LocalRun) -> String {
    let mut bugs: Vec<(&str, Vec<i32>)> = Vec::new();
    for case in &run.cases {
        for bug in &case.product_bugs {
            match bugs.iter_mut().find(|(t, _)| *t == bug.as_str()) {
                Some((_, ids)) => ids.push(case.case_id),
                None => bugs.push((bug.as_str(), vec![case.case_id])),
            }
        }
    }
    if bugs.is_empty() {
        return String::new();
    }
    let mut h = String::from("<h2>Product bugs worked around</h2><ul class=\"bugs\">");
    for (text, ids) in bugs.iter().take(MAX_BUG_LINES) {
        let cases: Vec<String> = ids.iter().map(|id| format!("#{id}")).collect();
        h.push_str(&format!("<li>{} - cases {}</li>", esc_attr(text), cases.join(", ")));
    }
    if bugs.len() > MAX_BUG_LINES {
        h.push_str(&format!("<li>and {} more</li>", bugs.len() - MAX_BUG_LINES));
    }
    h.push_str("</ul>");
    h
}

/// What the report shows beyond the run itself.
pub struct ReportExtras<'a> {
    /// The project's reported divergences (`divergences::load`).
    pub divergences: &'a [Divergence],
}

/// A bucket as a CSS class suffix.
fn css_key(bucket: &str) -> &'static str {
    match bucket {
        "Passed" => "passed",
        "Failed" => "failed",
        "Blocked" => "blocked",
        _ => "notrun",
    }
}

/// Only an http(s) address becomes a link; anything else is shown as text.
fn web_link(url: &str) -> String {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        format!("<a href=\"{}\">{}</a>", esc_attr(url.trim()), esc_attr(url.trim()))
    } else {
        esc_attr(url)
    }
}

/// The page's Content-Security-Policy: no script, nothing fetched, and
/// pictures only from a file on this machine (the shots folder, linked
/// relative to the page).
pub const CSP: &str = "default-src 'none'; img-src file:; style-src 'unsafe-inline'";

const STYLE: &str = "body{font-family:Segoe UI,Arial,sans-serif;color:#1f2328;background:#ffffff;margin:24px;font-size:14px;line-height:1.45}\
h1{font-size:22px;margin:0 0 12px}h2{font-size:17px;margin:24px 0 8px;border-bottom:1px solid #d0d7de;padding-bottom:4px}\
h3{font-size:15px;margin:0 0 6px}dl{display:grid;grid-template-columns:max-content 1fr;gap:2px 12px;margin:0 0 8px}\
dt{color:#59636e}dd{margin:0}table{border-collapse:collapse;width:100%}th,td{border:1px solid #d0d7de;padding:4px 8px;text-align:left;vertical-align:top}\
th{background:#f6f8fa}.id{font-family:Consolas,monospace;color:#59636e}.b-passed{color:#1a7f37;font-weight:600}\
.b-failed{color:#cf222e;font-weight:600}.b-blocked{color:#9a6700;font-weight:600}.b-notrun{color:#59636e;font-weight:600}\
.took{color:#59636e;font-size:12px;font-weight:400}.bugs{color:#9a6700;font-weight:600}.retried{color:#9a6700;font-size:12px;font-weight:600;border:1px solid #d4a72c;border-radius:10px;padding:0 6px;margin-left:6px}\
.case{border:1px solid #d0d7de;border-radius:6px;padding:8px 12px;margin:0 0 12px}.case>summary{cursor:pointer;font-weight:600;font-size:15px}\
.note{color:#59636e;font-style:italic}.reset{color:#9a6700;font-weight:600}.batch{text-align:left;font-weight:600}.step{margin:10px 0 0}h4{font-size:14px;margin:0 0 4px}\
.actions{list-style:none;margin:0 0 4px;padding:0}.actions li{margin:0 0 2px}.ok{color:#1a7f37}.bad{color:#cf222e}\
figure{margin:8px 0}figure img{max-width:100%;border:1px solid #d0d7de}figcaption{font-size:12px;color:#59636e}\
@media print{body{margin:0}.case,.step,tr,figure{break-inside:avoid}a{color:inherit}}";

/// The whole page. `ran_at` is the run's start as the person reads time
/// (the webview formats it in their own locale); blank falls back to UTC.
/// `scripts` supplies the words for a failed action and a case's area -
/// the scripts on this machine now, as `failures` reads them too. `exists`
/// says whether a screenshot is in the shots folder; it is only called for
/// a name the screenshot guard accepts.
///
/// Each download's size is read from the run's own folder under the
/// app's data root (`store::configured_root`), the way the report the app
/// writes reads it (`store::download_size`). Before that root is set, a
/// download reads as no longer on this machine.
pub fn build(
    run: &LocalRun,
    scripts: &[CaseScript],
    ran_at: &str,
    exists: &dyn Fn(&str) -> bool,
) -> String {
    let root = super::store::configured_root();
    let size_of = |name: &str| root.as_deref().and_then(|r| super::store::download_size(r, &run.id, name));
    build_with_downloads(run, scripts, ran_at, exists, &size_of)
}

/// [`build`], with `size_of` giving the size of each of the run's
/// downloads (`None`: no longer on this machine).
pub fn build_with_downloads(
    run: &LocalRun,
    scripts: &[CaseScript],
    ran_at: &str,
    exists: &dyn Fn(&str) -> bool,
    size_of: &dyn Fn(&str) -> Option<u64>,
) -> String {
    build_with_components(run, scripts, &ComponentFile::default(), ran_at, exists, size_of)
}

/// [`build_with_downloads`], with the components the run's scripts use:
/// an action a component ran is named as it expands from them, and by its
/// component only when it no longer does.
pub fn build_with_components(
    run: &LocalRun,
    scripts: &[CaseScript],
    components: &ComponentFile,
    ran_at: &str,
    exists: &dyn Fn(&str) -> bool,
    size_of: &dyn Fn(&str) -> Option<u64>,
) -> String {
    build_with_extras(run, scripts, components, ran_at, exists, size_of, &ReportExtras { divergences: &[] })
}

/// [`build_with_components`], with the product bugs listed on top (they
/// ride on the run's cases) and each case's divergences from `extras`.
pub fn build_with_extras(
    run: &LocalRun,
    scripts: &[CaseScript],
    components: &ComponentFile,
    ran_at: &str,
    exists: &dyn Fn(&str) -> bool,
    size_of: &dyn Fn(&str) -> Option<u64>,
    extras: &ReportExtras,
) -> String {
    let script_for = |id: i32| scripts.iter().find(|s| s.case_id == id);
    let when = if ran_at.trim().is_empty() { utc_time(&run.started_at) } else { ran_at.trim().to_string() };
    let mode = if run.mode == "unattended" { "Unattended" } else { "Supervised" };

    let mut h = String::new();
    h.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    // Defence in depth: everything below is escaped and nothing is fetched,
    // but should either ever slip, the page still may run no script and
    // load nothing beyond its linked pictures and inline style.
    h.push_str(&format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">"));
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    h.push_str(&format!("<title>Auto Run report - PBI #{}</title>", run.pbi_id));
    h.push_str(&format!("<style>{STYLE}</style></head><body>"));
    h.push_str("<h1>Auto Run report</h1><dl>");
    h.push_str(&format!("<dt>Ran</dt><dd>{}</dd>", esc_attr(&when)));
    h.push_str(&format!("<dt>Mode</dt><dd>{mode}</dd>"));
    if let Some(env) = run.environment.as_deref().filter(|e| !e.trim().is_empty()) {
        h.push_str(&format!("<dt>Environment</dt><dd>{}</dd>", esc_attr(env)));
    }
    h.push_str(&format!("<dt>PBI</dt><dd>#{}</dd>", run.pbi_id));
    match &run.published {
        Some(p) => h.push_str(&format!(
            "<dt>Azure DevOps</dt><dd>Sent as test run #{} - {}</dd>",
            p.run_id,
            web_link(&p.web_url)
        )),
        None => h.push_str("<dt>Azure DevOps</dt><dd>Not sent - these results are on this machine only</dd>"),
    }
    h.push_str("</dl>");
    h.push_str(&product_bugs_section(run));

    let c = counts(run);
    h.push_str("<h2>Summary</h2><table class=\"summary\"><thead><tr>");
    for b in BUCKETS {
        h.push_str(&format!("<th>{b}</th>"));
    }
    h.push_str("<th>Total</th></tr></thead><tbody><tr>");
    for (i, b) in BUCKETS.iter().enumerate() {
        h.push_str(&format!("<td class=\"b-{}\">{}</td>", css_key(b), c[i]));
    }
    h.push_str(&format!("<td>{}</td></tr></tbody></table>", run.cases.len()));
    // How much of what the passes claim their checks leave out.
    let (mut total, mut passed_with) = (0usize, 0usize);
    for case in run.cases.iter().filter(|c| case_bucket(c) == "Passed") {
        let n = script_for(case.case_id).map(|s| expectations(s).len()).unwrap_or(0);
        total += n;
        passed_with += usize::from(n > 0);
    }
    if total > 0 {
        let cases = if passed_with == 1 { "case" } else { "cases" };
        h.push_str(&format!("<p class=\"unchecked\">{} in {passed_with} passed {cases}</p>", count_words(total)));
    }

    h.push_str("<h2>Cases</h2><table class=\"cases\"><thead><tr><th>ID</th><th>Title</th><th>Result</th><th>Decided by</th><th>Account</th><th>Area</th></tr></thead><tbody>");
    for case in &run.cases {
        if let Some(heading) = batch_heading(run, case.case_id) {
            h.push_str(&format!("<tr class=\"batch\"><th colspan=\"6\">{}</th></tr>", esc_attr(&heading)));
        }
        for line in resets_before(run, case.case_id) {
            h.push_str(&format!("<tr class=\"reset\"><td colspan=\"6\">{line}</td></tr>"));
        }
        let b = case_bucket(case);
        let decided = if !case.verdict.is_empty() {
            "Confirmed"
        } else if !case.proposed.is_empty() {
            "Proposed"
        } else {
            "-"
        };
        let area = script_for(case.case_id).and_then(CaseScript::area_name).unwrap_or("");
        let result = result_words(b, script_for(case.case_id).map(|s| expectations(s).len()).unwrap_or(0));
        h.push_str(&format!(
            "<tr><td class=\"id\">#{}</td><td>{}</td><td class=\"b-{}\">{}</td><td>{decided}</td><td>{}</td><td>{}</td></tr>",
            case.case_id,
            esc_attr(&case.title),
            css_key(b),
            esc_attr(&result),
            esc_attr(case.account.as_deref().unwrap_or("")),
            esc_attr(area)
        ));
    }
    h.push_str("</tbody></table>");

    if !run.cases.is_empty() {
        h.push_str("<h2>Cases in detail</h2>");
        for case in &run.cases {
            if let Some(heading) = batch_heading(run, case.case_id) {
                h.push_str(&format!("<h3 class=\"batch\">{}</h3>", esc_attr(&heading)));
            }
            for line in resets_before(run, case.case_id) {
                h.push_str(&format!("<p class=\"reset\">{line}</p>"));
            }
            h.push_str(&case_section(
                case,
                script_for(case.case_id),
                components,
                exists,
                size_of,
                &shown_divergences(run, extras, case.case_id),
            ));
        }
    }

    h.push_str("</body></html>\n");
    h
}
