//! One run as a single HTML page a person can keep, print or attach to an
//! email: when it ran, where, what each case came to, and for each case
//! that failed or was blocked, the step that stopped it with its pictures.
//!
//! The page is opened in the browser from `<root>/reports/`: inline CSS and
//! one inline script, nothing fetched. It opens on an overview (the counts,
//! the biggest groups of failures that share a cause, the recent runs) and
//! has a view of every failure in those groups with a timeline of its
//! steps, a history of the environment's recent runs with a comparison to
//! the run before, a gallery of the pictures and videos, and "All cases":
//! the table and a collapsible section per case with every step the run
//! recorded, as it has always been. The pictures and videos are linked, not
//! embedded - `../shots/<name>`, beside the reports folder - so a long
//! unattended run stays a small file. Every value from the run is escaped:
//! a case title is whatever someone typed into Azure DevOps. The script
//! only adds the tabs, the filter, the keys, the copy buttons and the
//! gallery; with it off, or on paper, everything is on the page in order.
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
use super::report_smart::{self, Cluster, Failure};
use super::report_view::{self, CaseView, Counts};
use super::unchecked::{count_words, expectations, line as unchecked_line};
use super::{CaseRecord, CaseScript, LocalRun, ResetRecord, StepRecord};
use crate::browser::actions::{Action, ActionOutcome, DialogAnswer};
use crate::html_escape::esc_attr;
use crate::webtheme::PagePalette;

/// The result buckets, in the order the report (and the Auto Test screen's
/// filter row) lists them.
pub const BUCKETS: [&str; 4] = ["Passed", "Failed", "Blocked", "Not run"];

/// Which bucket a case's result falls in: the person's confirmed `verdict`
/// when set, else the machine's `proposed`; exactly "Passed", "Failed" or
/// "Blocked" is its own bucket, anything else (nothing proposed, or a word
/// that is none of the three) is "Not run".
///
/// The Auto Test screen filters with its own copy of this rule
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

/// Whether a case's failure is a flaky one: it is a Failed result and the
/// person marked the case's script flaky. It is still a failure - sent to
/// Azure DevOps as failed, its details shown in full - but the summary
/// counts it as flaky rather than failed, as Past runs and the review do
/// (`isFlakyFailure` in src/screens/AutoRun/verdicts.ts).
pub fn is_flaky_failure(case: &CaseRecord, script: Option<&CaseScript>) -> bool {
    case_bucket(case) == "Failed" && script.is_some_and(|s| s.flaky.is_some())
}

/// How many of the run's cases are flaky failures, given the scripts on
/// this machine.
pub fn flaky_count(run: &LocalRun, scripts: &[CaseScript]) -> usize {
    run.cases
        .iter()
        .filter(|c| is_flaky_failure(c, scripts.iter().find(|s| s.case_id == c.case_id)))
        .count()
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
        Action::Hover { selector, .. } => format!("hover over {}", selector.describe()),
        Action::Fill { selector, .. } => format!("fill in {}", selector.describe()),
        Action::SelectOption { selector, .. } => format!("choose an option in {}", selector.describe()),
        Action::WaitFor { selector, .. } => format!("wait for {}", selector.describe()),
        Action::CheckText { value, .. } => format!("check the page says \"{value}\""),
        Action::CheckUrl { equals: Some(want), .. } => {
            format!("expect the address to be \"{}\"", crate::browser::actions::path_only(want))
        }
        Action::CheckUrl { same_as: Some(name), .. } => {
            format!("expect the address to be the one noted as \"{name}\"")
        }
        Action::CheckUrl { contains, .. } => format!("check the address contains \"{contains}\""),
        Action::RememberUrl { name } => format!("note the address as \"{name}\""),
        Action::RememberText { selector, name } => {
            format!("note the text of {} as \"{name}\"", selector.describe())
        }
        Action::ExpectTextMoved { selector, same_as, by, .. } => {
            format!("expect {} to read the number noted as \"{same_as}\" moved by {by}", selector.describe())
        }
        Action::ExpectText { selector, same_as: Some(name), .. } => {
            format!("expect {} to read the text noted as \"{name}\"", selector.describe())
        }
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
        Action::ExpectNoChange { selector, region, .. } => {
            let to = region.as_ref().map(|r| format!(" to {}", r.describe())).unwrap_or_default();
            format!("click {} anyway and expect nothing to happen{to}", selector.describe())
        }
        Action::SignIn { account } => format!("sign in as {account}"),
        Action::Upload { selector, file } => format!("upload {file} into {}", selector.describe()),
        Action::ExpectResponse { method, url_contains, status, not_before_ms, started_within_ms, same_document, .. } => {
            let which = method.as_deref().map(|m| format!("{} ", m.trim().to_ascii_uppercase())).unwrap_or_default();
            let mut words = format!("expect a {which}request to {} to answer {status}", without_query(url_contains));
            if let Some(nb) = not_before_ms {
                words.push_str(&format!(", starting at least {nb} ms after the action before it"));
            }
            if let Some(sw) = started_within_ms {
                words.push_str(&format!(", starting within {sw} ms of the action before it"));
            }
            if *same_document {
                words.push_str(", with no page reload");
            }
            words
        }
        Action::ApiRequest { path, expect, .. } => {
            format!("ask {} and expect {}", without_query(path), expect.status)
        }
        Action::WhenVisible { selector, then, .. } => {
            let what: Vec<String> = then.iter().map(action_words).collect();
            format!("if {} shows up, {}", selector.describe(), what.join(", then "))
        }
        Action::WhenHidden { selector, then, .. } => {
            let what: Vec<String> = then.iter().map(action_words).collect();
            format!("if {} is not on the page, {}", selector.describe(), what.join(", then "))
        }
        Action::RepeatWhileVisible { selector, max, except, then, .. } => {
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

/// The longest start every title shares, cut to end right after a " - "
/// (whole segments only). None for under two titles, no shared separator, or
/// when dropping it would leave a title blank.
pub fn shared_title_start(titles: &[&str]) -> Option<String> {
    let first = titles.first()?;
    if titles.len() < 2 {
        return None;
    }
    let mut common = first.len();
    for t in &titles[1..] {
        common = common.min(first.bytes().zip(t.bytes()).take_while(|(a, b)| a == b).count());
    }
    let head = first.get(..common).unwrap_or_else(|| {
        let mut n = common;
        while !first.is_char_boundary(n) {
            n -= 1;
        }
        &first[..n]
    });
    let end = head.rfind(" - ")? + 3;
    if titles.iter().any(|t| t[end..].trim().is_empty()) {
        return None;
    }
    Some(first[..end].to_string())
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
pub(super) fn utc_time(started_at: &str) -> String {
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
pub(super) fn stopping_point(case: &CaseRecord) -> Option<(&StepRecord, usize, &ActionOutcome)> {
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
pub(super) fn stopped_action(
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
        esc_attr(alt)
    )
}

/// Milliseconds as `2.3 s`.
fn seconds(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// A link to the video kept of a failed case, beside its pictures: relative
/// like they are (`../shots/<name>`) and never embedded, so the page loads
/// nothing and stays as small as it was. A name the video guard does not
/// accept is never linked; a video no longer on this machine is said so.
fn video_line(case: &CaseRecord, exists: &dyn Fn(&str) -> bool) -> String {
    let Some(name) = case.video.as_deref() else { return String::new() };
    if !super::store::safe_video_name(name) {
        return format!("<p class=\"note\">A video was skipped: {} is not a video name.</p>", esc_attr(name));
    }
    if !exists(name) {
        return format!("<p class=\"note\">The video {} was not found - it is no longer on this machine.</p>", esc_attr(name));
    }
    // A tile like the pictures', captioned Video: the file's name is not shown. The script opens it
    // in the player; without the script the link opens the file.
    format!(
        "<figure class=\"vtile\"><a class=\"vtile__media\" href=\"../shots/{0}\" data-video=\"{0}\" aria-label=\"Play the video\">{1}</a><figcaption>Video</figcaption></figure>",
        esc_attr(name),
        report_view::icon("video")
    )
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

/// What the run recorded about a case besides its steps and pictures: why,
/// a retry, a notice, product bugs, divergences, what its checks leave out,
/// a clean-up that failed, the person's note, where the time went, and the
/// files it downloaded. Empty when there is none of it.
fn case_notes(
    case: &CaseRecord,
    script: Option<&CaseScript>,
    size_of: &dyn Fn(&str) -> Option<u64>,
    divergences: &[&Divergence],
) -> String {
    let mut h = String::new();
    if !case.reason.is_empty() {
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
    for e in &script.map(expectations).unwrap_or_default() {
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
    if case.page_errors_seen > 0 {
        h.push_str(&format!("<p><strong>Page errors seen:</strong> {}</p>", case.page_errors_seen));
    }
    if let Some(phases) = &case.phases {
        h.push_str(&format!("<p class=\"muted\">{}</p>", esc_attr(&phases.line())));
    }
    h.push_str(&downloads_line(case, size_of));
    h
}

/// Every step of a case as a line - its number, its label, a small bar for
/// how long it took and the time - with the actions it ran under it, each
/// marked with a tick or a cross and the sentence the run recorded (never
/// anything from the script). The case's clean-up comes last.
fn steps_list(case: &CaseRecord) -> String {
    if case.steps.is_empty() {
        return "<p class=\"muted\">No steps were recorded for this case.</p>".to_string();
    }
    let labelled = case.steps.iter().map(|s| (step_label(s.step_number), s));
    let tidy = case.cleanup.iter().map(|s| (super::CLEANUP_LABEL.to_string(), s));
    let all: Vec<(String, &StepRecord)> = labelled.chain(tidy).collect();
    let longest = all.iter().filter_map(|(_, s)| s.duration_ms).max().unwrap_or(0).max(1);
    let mut h = String::from("<ol class=\"steps\">");
    for (label, step) in &all {
        let failed = step.outcomes.iter().any(|o| !o.ok && !o.detail.starts_with("not run:"));
        let (bar, dur) = match step.duration_ms {
            Some(ms) => (format!("<span style=\"width:{:.1}%\"></span>", ms as f64 / longest as f64 * 100.0), seconds(ms)),
            None => ("<span style=\"width:0%\"></span>".to_string(), String::new()),
        };
        h.push_str(&format!(
            "<li class=\"stepitem\"><div class=\"step\"{}><span class=\"step__dot\"></span><span class=\"step__title\">{}</span><span class=\"step__bar\">{bar}</span><span class=\"step__dur mono\">{dur}</span></div>",
            if failed { " data-failed" } else { "" },
            esc_attr(label),
        ));
        if step.outcomes.is_empty() {
            h.push_str("<p class=\"muted step__none\">No actions were recorded for this step.</p>");
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
        h.push_str("</li>");
    }
    h.push_str("</ol>");
    h
}

/// The pictures a case's steps took (each step's, once), the video, and how
/// many pictures that is. A picture whose name is not a screenshot name, or
/// that is no longer on this machine, is a note.
fn pictures_block(case: &CaseRecord, exists: &dyn Fn(&str) -> bool) -> (String, usize) {
    let mut h = String::new();
    let mut n = 0;
    let labelled = case.steps.iter().map(|s| (step_label(s.step_number), s));
    let tidy = case.cleanup.iter().map(|s| (super::CLEANUP_LABEL.to_string(), s));
    for (label, step) in labelled.chain(tidy) {
        let mut seen = std::collections::HashSet::new();
        for (i, o) in step.outcomes.iter().enumerate() {
            if let Some(name) = o.screenshot.as_deref() {
                if seen.insert(name) {
                    h.push_str(&picture(name, &format!("{label}, action {}", i + 1), exists));
                    n += 1;
                }
            }
        }
        if let Some(name) = step.screenshot.as_deref() {
            if seen.insert(name) {
                h.push_str(&picture(name, &label, exists));
                n += 1;
            }
        }
    }
    h.push_str(&video_line(case, exists));
    (h, n)
}

/// One case's detail: the title and its meta, the copy buttons, and the
/// sections - the error, the timeline, the steps, the pictures, its history
/// and what else the run recorded. All of them are on the page whether or
/// not the script shows only the selected one, so nothing is lost.
#[allow(clippy::too_many_arguments)]
fn detail(
    v: &CaseView,
    failure: Option<&Failure>,
    script: Option<&CaseScript>,
    exists: &dyn Fn(&str) -> bool,
    size_of: &dyn Fn(&str) -> Option<u64>,
    divergences: &[&Divergence],
    before: &[String],
) -> String {
    let case = v.case;
    let id = case.case_id;
    let unchecked = script.map(expectations).unwrap_or_default();
    let result = result_words(v.bucket, unchecked.len());
    let ms = report_smart::duration_of(case);
    let mut meta = format!("<span class=\"mono\">#{id}</span><span class=\"tone-{}\">{}</span>", match v.status() {
        "failed" => "fail",
        "passed" => "pass",
        "flaky" => "flaky",
        "blocked" => "blocked",
        _ => "skip",
    }, esc_attr(&result));
    let decided = if !case.verdict.is_empty() {
        "Confirmed"
    } else if !case.proposed.is_empty() {
        "Proposed"
    } else {
        ""
    };
    if !decided.is_empty() {
        meta.push_str(&format!("<span class=\"muted\">Decided by: {decided}</span>"));
    }
    let place: Vec<&str> = [v.area.as_str(), v.module.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
    if !place.is_empty() {
        meta.push_str(&format!("<span>{}</span>", esc_attr(&place.join(" \u{203a} "))));
    }
    if ms > 0 {
        meta.push_str(&format!("<span class=\"mono\">{}</span>", report_view::took(ms)));
    }
    if let Some(g) = v.grade() {
        meta.push_str(&format!("<span>Grade {g}</span>"));
    }
    if v.new_failure {
        meta.push_str("<span class=\"signal\">New failure</span>");
    }
    if v.flaky() {
        meta.push_str("<span class=\"signal\">Flaky</span>");
    }
    if v.slow() {
        meta.push_str("<span class=\"signal\">Slow</span>");
    }
    if case.retried.is_some() {
        meta.push_str("<span class=\"signal\">Retried</span>");
    }
    if case.page_errors_seen > 0 {
        meta.push_str(&format!("<span class=\"signal\">page errors seen: {}</span>", case.page_errors_seen));
    }
    if let Some(a) = case.account.as_deref().filter(|a| !a.is_empty()) {
        meta.push_str(&format!("<span class=\"muted\">as {}</span>", esc_attr(a)));
    }
    let mut h = format!(
        "<article class=\"dt detail\" id=\"case-{id}\" data-id=\"{id}\" data-status=\"{}\" data-print=\"{}\"><div class=\"detail__head\"><div class=\"detail__title-row\">{}<h2 class=\"detail__title\">{}</h2><span class=\"detail__nav\"><button type=\"button\" class=\"icon-btn\" data-step=\"-1\" aria-label=\"Previous case\">{}</button><button type=\"button\" class=\"icon-btn\" data-step=\"1\" aria-label=\"Next case\">{}</button></span></div><div class=\"detail__meta\">{meta}</div>",
        v.status(),
        u8::from(v.is_failure()),
        report_view::glyph(v.status()),
        esc_attr(&case.title),
        report_view::icon("chevron-up"),
        report_view::icon("chevron-down"),
    );
    if failure.is_some() {
        h.push_str(&format!(
            "<div class=\"detail__actions btns\"><button type=\"button\" class=\"btn\" data-copy=\"summary\" data-id=\"{id}\">{} Copy failure summary <kbd class=\"kbd\">c</kbd></button><button type=\"button\" class=\"btn\" data-copy=\"prompt\" data-id=\"{id}\">Copy AI prompt</button></div>",
            report_view::icon("copy")
        ));
    }
    h.push_str("</div>");

    if let Some(f) = failure {
        let mut lines = f.message.lines();
        let first = lines.next().unwrap_or("");
        let rest: Vec<&str> = lines.collect();
        let mut body = format!("<pre class=\"error-text\"><span class=\"error-text__head\">{}</span>", esc_attr(first));
        if !rest.is_empty() {
            body.push('\n');
            body.push_str(&esc_attr(&rest.join("\n")));
        }
        body.push_str("</pre>");
        if f.step_number.is_some() {
            body.push_str(&format!("<p class=\"error__step\"><span class=\"muted\">Stopped at</span> {} \u{b7} {}</p>", esc_attr(&f.step), esc_attr(&f.action)));
        } else {
            body.push_str("<p class=\"note muted\">No step failed on its own - this result was decided by the person reviewing the run.</p>");
        }
        h.push_str(&report_view::section("Error", "", true, &body));
        if let Some(t) = &v.timeline {
            let meta = if t.anomaly.is_some() { "possible first anomaly found" } else { "" };
            h.push_str(&report_view::section("Timeline", meta, true, &report_view::timeline_body(t)));
        }
    }
    // What the run did just before this case (a batch's start, a pause for settings), in order.
    let pauses: String = before.iter().map(|l| format!("<p class=\"reset\">{l}</p>")).collect();
    h.push_str(&report_view::section("Steps", &format!("{} steps", case.steps.len()), true, &format!("{pauses}{}", steps_list(case))));
    let (pics, n) = pictures_block(case, exists);
    if !pics.is_empty() {
        let meta = if n > 0 { format!("{n} picture{}", if n == 1 { "" } else { "s" }) } else { String::new() };
        h.push_str(&report_view::section("Screenshots and video", &meta, true, &pics));
    }
    h.push_str(&report_view::section("History", "", true, &report_view::history_body(v.trend)));
    let notes = case_notes(case, script, size_of, divergences);
    if !notes.is_empty() {
        h.push_str(&report_view::section("Run notes", "", true, &notes));
    }
    h.push_str("</article>");
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
        (DivergenceStatus::Ruled, Some(Ruling::DataFixed)) => "ruled: you fixed the data",
        (DivergenceStatus::Ruled, Some(Ruling::Rewrite)) => "ruled: the app is right, the case is being updated",
        (DivergenceStatus::Ruled, _) => "ruled: the app is wrong",
    };
    let by = match (d.ruled_by, d.said.as_deref()) {
        (Some(RuledBy::Assistant), said) => {
            format!(", ruled by the assistant: the person said \"{}\"", said.unwrap_or(""))
        }
        _ => String::new(),
    };
    format!(
        "step {}: the case says \"{}\", the app shows \"{}\" ({}, {status}{by}){}",
        d.step,
        d.case_says,
        d.app_shows,
        d.kind.word(),
        crate::autorun::divergences::proposal_words(d)
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
pub(super) fn css_key(bucket: &str) -> &'static str {
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

/// The page's design tokens: the neutral and status colours of each mode,
/// the type and space scales, and `--accent` (`web/autorun-report-tokens.css`).
const TOKENS: &str = include_str!("../../web/autorun-report-tokens.css");

/// The page's stylesheet: components only, in the tokens' variables, with no
/// colour of its own (`tests/suite/autorun_report_smart.rs` scans it).
const STYLE: &str = include_str!("../../web/autorun-report.css");

/// The page's own script: views, filter, keys, copy, gallery.
const SCRIPT: &str = include_str!("../../web/autorun-report.js");

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
/// ride on the run's cases) and each case's divergences from `extras`. The
/// history is only this run, and the palette the page's default one: see
/// [`build_smart`].
pub fn build_with_extras(
    run: &LocalRun,
    scripts: &[CaseScript],
    components: &ComponentFile,
    ran_at: &str,
    exists: &dyn Fn(&str) -> bool,
    size_of: &dyn Fn(&str) -> Option<u64>,
    extras: &ReportExtras,
) -> String {
    let marked: Vec<i32> = scripts.iter().filter(|s| s.flaky.is_some()).map(|s| s.case_id).collect();
    build_smart(
        run,
        scripts,
        components,
        ran_at,
        exists,
        size_of,
        extras,
        &TrendInput { runs: &[], flaky_marked: &marked },
        &PagePalette::default(),
    )
}

/// The runs a report's trends are read from.
pub struct TrendInput<'a> {
    /// Every run on this machine (`store::list_runs`); the report's own may
    /// be among them. Only the ones in the same environment and not newer
    /// count (`report_smart::trend`).
    pub runs: &'a [LocalRun],
    /// The ids of the cases whose script is marked flaky
    /// (`store::flaky_marked`): a failure of one of them counts as flaky.
    pub flaky_marked: &'a [i32],
}

/// The names of the pictures a step took, in the order the report shows
/// them (an action's, then the step's), unique, and only the ones that are
/// safe names present in the shots folder.
fn step_pictures<'a>(step: &'a StepRecord, exists: &dyn Fn(&str) -> bool) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    let named = step.outcomes.iter().filter_map(|o| o.screenshot.as_deref()).chain(step.screenshot.as_deref());
    for n in named {
        if super::store::safe_shot_name(n) && exists(n) && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

/// `sha256-<base64>` of a script's exact text, the form a
/// Content-Security-Policy `script-src` takes.
fn script_hash(text: &str) -> String {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    format!("'sha256-{}'", base64::engine::general_purpose::STANDARD.encode(digest))
}

/// The page's Content-Security-Policy for the scripts it writes (`scripts`,
/// each exactly as it sits between its tags): nothing fetched, pictures and
/// videos only from a file on this machine (the shots folder, linked
/// relative to the page), style only inline, and script only those
/// scripts, by hash - no `'unsafe-inline'` - so a value that slipped past
/// the escaping could not run. No `<base>` and no form can send anything
/// anywhere.
pub fn csp(scripts: &[&str]) -> String {
    let hashes: Vec<String> = scripts.iter().map(|s| script_hash(s)).collect();
    format!(
        "default-src 'none'; img-src file:; media-src file:; style-src 'unsafe-inline'; script-src {}; base-uri 'none'; form-action 'none'",
        hashes.join(" ")
    )
}

/// The first script in the page's head: the choice of light or dark the
/// reader made last time (else the app's, which is on `<html>`), and the
/// flag that says the page's script is running, so its views may hide
/// themselves. Guarded: a `file://` page has no usable localStorage in some
/// browsers, and it throws rather than returning null.
pub const HEAD_JS: &str = "try{var m=localStorage.getItem('tcm-report-mode');if(m==='dark'||m==='light')document.documentElement.setAttribute('data-mode',m)}catch(e){}\ndocument.documentElement.classList.add('js');";

/// [`build_with_extras`], with the runs the history and trends are read
/// from and the palette the page wears (only its accent: the page keeps
/// its own neutrals and status colours).
#[allow(clippy::too_many_arguments)]
pub fn build_smart(
    run: &LocalRun,
    scripts: &[CaseScript],
    components: &ComponentFile,
    ran_at: &str,
    exists: &dyn Fn(&str) -> bool,
    size_of: &dyn Fn(&str) -> Option<u64>,
    extras: &ReportExtras,
    history: &TrendInput,
    palette: &PagePalette,
) -> String {
    let script_for = |id: i32| scripts.iter().find(|s| s.case_id == id);
    let when = if ran_at.trim().is_empty() { utc_time(&run.started_at) } else { ran_at.trim().to_string() };
    let mode = if run.mode == "unattended" { "Unattended" } else { "Supervised" };

    // What the views say, worked out once.
    let (clusters, failures) = report_smart::clusters(run, scripts, components);
    let trend = report_smart::trend(run, history.runs, history.flaky_marked);
    let cluster_of: std::collections::HashMap<i32, &Cluster> =
        clusters.iter().flat_map(|c| c.cases.iter().map(move |id| (*id, c))).collect();
    let new_failures: std::collections::HashSet<i32> =
        trend.compare.as_ref().map(|c| c.new_failures.iter().copied().collect()).unwrap_or_default();
    let views: Vec<CaseView> = run
        .cases
        .iter()
        .map(|case| {
            let script = script_for(case.case_id);
            let bucket = case_bucket(case);
            let timeline = matches!(bucket, "Failed" | "Blocked").then(|| {
                let passes = trend.passes.get(&case.case_id).map(Vec::as_slice).unwrap_or(&[]);
                report_smart::timeline(case, passes)
            });
            CaseView {
                case,
                bucket,
                flaky_failure: is_flaky_failure(case, script),
                area: script.and_then(CaseScript::area_name).unwrap_or("").to_string(),
                module: report_smart::module_of(case).unwrap_or_default(),
                trend: trend.per_case.get(&case.case_id),
                cluster: cluster_of.get(&case.case_id).copied(),
                new_failure: new_failures.contains(&case.case_id),
                timeline,
            }
        })
        .collect();
    let by_id: std::collections::HashMap<i32, &CaseView> = views.iter().map(|v| (v.case.case_id, v)).collect();
    let failure_text = |v: &CaseView| -> String {
        failures.get(&v.case.case_id).map(|f| format!("{} {} {}", f.step, f.action, f.message)).unwrap_or_default()
    };

    // The counts, with the app's rule: a flaky-marked failure is flaky, once.
    let flaky = flaky_count(run, scripts);
    let mut c4 = counts(run);
    c4[1] -= flaky;
    let counts_now = Counts { failed: c4[1], passed: c4[0], flaky, blocked: c4[2], not_run: c4[3] };
    let flaky_cases = views.iter().filter(|v| v.flaky()).count();
    let failure_cases = views.iter().filter(|v| v.is_failure() && !v.flaky_failure).count();
    let run_ms: u64 = run.cases.iter().map(report_smart::duration_of).sum();

    // The gallery, and each failed case's own pictures for its summary.
    let mut gallery: Vec<serde_json::Value> = Vec::new();
    let (mut shots, mut videos) = (0usize, 0usize);
    let mut seen_pictures = std::collections::HashSet::new();
    for case in &run.cases {
        let labelled = case.steps.iter().map(|s| (step_label(s.step_number), s));
        let tidy = case.cleanup.iter().map(|s| (super::CLEANUP_LABEL.to_string(), s));
        for (label, step) in labelled.chain(tidy) {
            for name in step_pictures(step, exists) {
                if seen_pictures.insert(name.to_string()) {
                    gallery.push(serde_json::json!({ "n": name, "c": case.case_id, "s": label }));
                    shots += 1;
                }
            }
        }
        if let Some(v) = case.video.as_deref().filter(|v| super::store::safe_video_name(v) && exists(v)) {
            if seen_pictures.insert(v.to_string()) {
                gallery.push(serde_json::json!({ "n": v, "c": case.case_id, "s": "Video", "k": 1 }));
                videos += 1;
            }
        }
    }
    let mut summaries = serde_json::Map::new();
    for v in views.iter().filter(|v| v.is_failure()) {
        let Some(failure) = failures.get(&v.case.case_id) else { continue };
        let pictures: Vec<String> = v
            .case
            .steps
            .iter()
            .filter(|s| Some(s.step_number) == failure.step_number)
            .flat_map(|s| step_pictures(s, exists))
            .map(str::to_string)
            .collect();
        let video = v.case.video.as_deref().filter(|n| super::store::safe_video_name(n) && exists(n));
        summaries.insert(
            v.case.case_id.to_string(),
            serde_json::Value::String(report_smart::summary_markdown(v.case, failure, run.environment.as_deref(), &when, &pictures, video)),
        );
    }

    // ---- The lists.
    let mut fail_list = String::new();
    let mut insight_clusters: Vec<String> = Vec::new();
    let mut print_clusters = String::new();
    let mut n_clusters = 0;
    for c in &clusters {
        let members: Vec<&CaseView> = c.cases.iter().filter_map(|id| by_id.get(id).copied()).filter(|v| !v.flaky_failure).collect();
        if members.is_empty() {
            continue;
        }
        n_clusters += 1;
        let rows: String = members.iter().map(|v| report_view::row(v, "failures", &failure_text(v))).collect();
        let note = if c.example.is_empty() { String::new() } else { format!("<p class=\"grp__note\">{}</p>", esc_attr(&c.example)) };
        fail_list.push_str(&report_view::group(&c.title, true, members.len(), &rows, &note));
        insight_clusters.push(format!(
            "<span class=\"lines__label mono\" title=\"{t}\">{t}</span><span class=\"tone-fail count mono\">{}{}</span>",
            report_view::icon("failed"),
            members.len(),
            t = esc_attr(&c.title)
        ));
        let ids: Vec<String> = members.iter().map(|v| format!("#{}", v.case.case_id)).collect();
        print_clusters.push_str(&format!(
            "<li><strong>{}</strong> ({}): {}<br><span class=\"muted\">{}</span></li>",
            esc_attr(&c.title),
            members.len(),
            ids.join(", "),
            esc_attr(&c.example)
        ));
    }
    if fail_list.is_empty() {
        fail_list = "<div class=\"empty\"><p class=\"empty__title\">No case failed or was blocked in this run.</p></div>".to_string();
    }
    let flaky_rows: String = views
        .iter()
        .filter(|v| v.flaky())
        .map(|v| report_view::row(v, "flaky", &failure_text(v)))
        .collect();
    let flaky_list = if flaky_rows.is_empty() {
        "<div class=\"empty\"><p class=\"empty__title\">No flaky cases.</p><p class=\"empty__body\">A case is flaky when its script is marked flaky, or it passed and failed among its recent runs in this environment.</p></div>".to_string()
    } else {
        flaky_rows
    };
    let mut all_list = String::new();
    if views.is_empty() {
        all_list.push_str("<div class=\"empty\"><p class=\"empty__title\">This run has no cases.</p></div>");
    } else if in_batches(run) {
        // Under the batches the run made, with the pauses between them.
        let mut rows = String::new();
        let mut label = String::new();
        let mut extra = String::new();
        let mut count = 0;
        for v in &views {
            if let Some(heading) = batch_heading(run, v.case.case_id) {
                if count > 0 {
                    all_list.push_str(&report_view::group(&label, false, count, &rows, &extra));
                }
                label = heading;
                rows.clear();
                extra = resets_before(run, v.case.case_id).iter().map(|l| format!("<p class=\"reset grp__note\">{l}</p>")).collect();
                count = 0;
            }
            rows.push_str(&report_view::row(v, "all", &failure_text(v)));
            count += 1;
        }
        all_list.push_str(&report_view::group(&label, false, count, &rows, &extra));
    } else if views.iter().any(|v| !v.area.is_empty()) {
        let mut areas: Vec<(String, String, usize)> = Vec::new();
        for v in &views {
            let name = if v.area.is_empty() { "No area".to_string() } else { v.area.clone() };
            let pauses: String = resets_before(run, v.case.case_id).iter().map(|l| format!("<p class=\"reset grp__note\">{l}</p>")).collect();
            let row = format!("{pauses}{}", report_view::row(v, "all", &failure_text(v)));
            match areas.iter_mut().find(|(n, _, _)| *n == name) {
                Some((_, rows, n)) => {
                    rows.push_str(&row);
                    *n += 1;
                }
                None => areas.push((name, row, 1)),
            }
        }
        for (name, rows, n) in &areas {
            all_list.push_str(&report_view::group(name, false, *n, rows, ""));
        }
    } else {
        for v in &views {
            for line in resets_before(run, v.case.case_id) {
                all_list.push_str(&format!("<p class=\"reset grp__note\">{line}</p>"));
            }
            all_list.push_str(&report_view::row(v, "all", &failure_text(v)));
        }
    }
    // The sidebar's Flaky counts every case flaky in any sense; the top bar counts this run's flaky
    // failures. Both say so.
    let flaky_note = format!("Cases whose result flipped in recent runs. {} counted as flaky in this run.", counts_now.flaky);
    let lists = format!(
        "<div class=\"list\" id=\"list-failures\" data-title=\"Failures\" data-sub=\"Failed and blocked cases, grouped by what stopped them.\" data-count=\"{failure_cases}\">{fail_list}</div>\
<div class=\"list\" id=\"list-flaky\" data-title=\"Flaky\" data-sub=\"{}\" data-count=\"{flaky_cases}\" hidden>{flaky_list}</div>\
<div class=\"list\" id=\"list-all\" data-title=\"All cases\" data-sub=\"Every case of the run.\" data-count=\"{}\" hidden>{all_list}</div>",
        esc_attr(&flaky_note),
        views.len()
    );

    // ---- The details, one per case, in the run's order.
    let mut details = String::new();
    for v in &views {
        details.push_str(&detail(
            v,
            failures.get(&v.case.case_id),
            script_for(v.case.case_id),
            exists,
            size_of,
            &shown_divergences(run, extras, v.case.case_id),
            &{
                let mut lines: Vec<String> = batch_heading(run, v.case.case_id).map(|b| esc_attr(&b)).into_iter().collect();
                lines.extend(resets_before(run, v.case.case_id));
                lines
            },
        ));
    }
    let cases_view = format!(
        "<section class=\"view split\" id=\"view-cases\">{}<div class=\"pane__detail\" id=\"details\"><div class=\"empty detail__empty\" hidden><p class=\"empty__title\">Select a case</p></div>{details}</div></section>",
        report_view::list_pane(&lists)
    );

    // ---- Insights.
    let scope = report_view::scope_label(run);
    let health = report_smart::health(&trend.points, flaky_cases, views.len());
    let new_cases = trend.compare.as_ref().map(|c| c.new_cases.len());
    let prior = trend.points.len().saturating_sub(1);
    // One instant for every time on the page: the run's own start, shown by the script in one format.
    let when_tag = report_view::time_tag_as(&run.started_at, &when);
    let sub = if prior == 0 {
        format!("Run of {when_tag} \u{b7} the first run of {} in this environment", esc_attr(&scope))
    } else {
        format!("Run of {when_tag} \u{b7} compared with the previous {}", if prior == 1 { "run".to_string() } else { format!("{prior} runs") })
    };
    let shared = shared_title_start(&views.iter().map(|v| v.case.title.as_str()).collect::<Vec<_>>()).unwrap_or_default();
    let line_for = |ids: &[i32], status: &str| -> Vec<String> {
        ids.iter()
            .filter_map(|id| by_id.get(id).copied())
            .map(|v| report_view::case_line(if status.is_empty() { v.status() } else { status }, v.case.case_id, &v.case.title, "", &shared))
            .collect()
    };
    let cmp = trend.compare.clone().unwrap_or_default();
    let slow_lines: Vec<String> = report_smart::slowest(run, 5)
        .into_iter()
        .filter_map(|(id, ms)| by_id.get(&id).map(|v| report_view::case_line(v.status(), id, &v.case.title, &report_view::took(ms), &shared)))
        .collect();
    let order: Vec<i32> = run.cases.iter().map(|c| c.case_id).collect();
    let flaky_lines: Vec<String> = report_smart::most_flaky(&trend.per_case, &order, 5)
        .into_iter()
        .filter_map(|(id, pct)| by_id.get(&id).map(|v| report_view::case_line(v.status(), id, &v.case.title, &format!("{pct}%"), &shared)))
        .collect();
    let mut insights = format!(
        "<section class=\"view page\" id=\"view-insights\"><div class=\"page__head\"><h1 class=\"page__title\">Insights</h1><span class=\"muted\">{sub}</span></div>"
    );
    insights.push_str(&report_view::stats(&trend.points, new_cases, views.len(), health.as_ref(), report_view::scope_noun(run)));
    insights.push_str(&report_view::minibar(&counts_now));
    insights.push_str("<div class=\"panels\">");
    insights.push_str(&report_view::panel("Failure clusters", &format!("{n_clusters} cluster{}", if n_clusters == 1 { "" } else { "s" }), &insight_clusters, "None this run."));
    insights.push_str(&report_view::panel("Slowest", "this run", &slow_lines, "No timings were recorded."));
    insights.push_str(&report_view::panel("Most flaky", "flips between runs", &flaky_lines, "None this run."));
    insights.push_str(&report_view::panel("Fixed", &cmp.fixed.len().to_string(), &line_for(&cmp.fixed, "passed"), "None this run."));
    insights.push_str(&report_view::panel("New failures", &cmp.new_failures.len().to_string(), &line_for(&cmp.new_failures, ""), "None this run."));
    insights.push_str(&report_view::panel("New cases", &cmp.new_cases.len().to_string(), &line_for(&cmp.new_cases, ""), "None this run."));
    insights.push_str("</div>");
    if !print_clusters.is_empty() {
        insights.push_str(&format!("<div class=\"print-only\"><h2 class=\"panel__title\">Failure clusters</h2><ul>{print_clusters}</ul></div>"));
    }

    // The run's details, as the report always gave them.
    insights.push_str("<h2 class=\"panel__title run__title\">Run</h2><dl class=\"run__dl\">");
    insights.push_str(&format!("<dt>Ran</dt><dd>{when_tag}</dd>"));
    insights.push_str(&format!("<dt>Mode</dt><dd>{mode}</dd>"));
    if let Some(env) = run.environment.as_deref().filter(|e| !e.trim().is_empty()) {
        insights.push_str(&format!("<dt>Environment</dt><dd>{}</dd>", esc_attr(env)));
    }
    match &run.suite {
        Some(r) => insights.push_str(&format!("<dt>Suite</dt><dd>{}</dd>", esc_attr(&r.title))),
        None => insights.push_str(&format!("<dt>PBI</dt><dd>#{}</dd>", run.pbi_id)),
    }
    match &run.published {
        Some(p) => insights.push_str(&format!(
            "<dt>Azure DevOps</dt><dd>Sent as test run #{} - {}</dd>",
            p.run_id,
            web_link(&p.web_url)
        )),
        None => insights.push_str("<dt>Azure DevOps</dt><dd>Not sent - these results are on this machine only</dd>"),
    }
    insights.push_str("</dl>");
    insights.push_str(&product_bugs_section(run));
    insights.push_str("<h2>Summary</h2><table class=\"summary\"><thead><tr>");
    for b in BUCKETS {
        insights.push_str(&format!("<th>{b}</th>"));
    }
    if flaky > 0 {
        insights.push_str("<th>Flaky</th>");
    }
    insights.push_str("<th>Total</th></tr></thead><tbody><tr>");
    for (i, b) in BUCKETS.iter().enumerate() {
        insights.push_str(&format!("<td class=\"b-{}\">{}</td>", css_key(b), c4[i]));
    }
    if flaky > 0 {
        insights.push_str(&format!("<td class=\"b-flaky\">{flaky}</td>"));
    }
    insights.push_str(&format!("<td>{}</td></tr></tbody></table>", run.cases.len()));
    if flaky > 0 {
        insights.push_str(&format!(
            "<p class=\"note\">{flaky} failed case{} marked as flaky, so {} not counted as failed above. Still sent as failed.</p>",
            if flaky == 1 { " is" } else { "s are" },
            if flaky == 1 { "it is" } else { "they are" }
        ));
    }
    // How much of what the passes claim their checks leave out.
    let (mut total, mut passed_with) = (0usize, 0usize);
    for case in run.cases.iter().filter(|c| case_bucket(c) == "Passed") {
        let n = script_for(case.case_id).map(|s| expectations(s).len()).unwrap_or(0);
        total += n;
        passed_with += usize::from(n > 0);
    }
    if total > 0 {
        let cases = if passed_with == 1 { "case" } else { "cases" };
        insights.push_str(&format!("<p class=\"unchecked\">{} in {passed_with} passed {cases}</p>", count_words(total)));
    }
    if run.video_unavailable {
        insights.push_str("<p class=\"note\">Videos of failed cases were asked for, but this browser cannot record them.</p>");
    }
    insights.push_str("</section>");

    // ---- History: the titles the compare lists need.
    let titles: std::collections::HashMap<i32, (String, &'static str)> = views.iter().map(|v| (v.case.case_id, (v.case.title.clone(), v.status()))).collect();
    let history_html = report_view::history_view(&trend, &scope, report_view::scope_noun(run), run.environment.as_deref(), &titles, &shared);

    // ---- The page.
    let blob = report_view::data_blob(&serde_json::json!({ "sum": summaries, "gallery": gallery }));
    let head_js = HEAD_JS;
    // A checkout may have turned the file's line ends into CRLF; a browser reads a script's CRLF
    // as LF before it hashes it, so the hash is of the LF text, and that is the text written.
    let app_js_lf = SCRIPT.replace("\r\n", "\n");
    let app_js = app_js_lf.as_str();
    let (accent_l, accent_d) = palette.accents();
    let mut accent = String::new();
    if let Some(a) = accent_l {
        accent.push_str(&format!("--accent-l:{a};"));
    }
    if let Some(a) = accent_d {
        accent.push_str(&format!("--accent-d:{a};"));
    }
    let accent = if accent.is_empty() { String::new() } else { format!(":root{{{accent}}}\n") };
    let start = if failure_cases > 0 { "failures" } else { "insights" };

    let mut h = String::new();
    h.push_str(&format!("<!doctype html><html lang=\"en\" data-mode=\"{}\"><head><meta charset=\"utf-8\">", palette.initial_scheme()));
    // Defence in depth: everything below is escaped and nothing is fetched,
    // but should either ever slip, the page still may load nothing beyond
    // its linked pictures and videos and its own inline style, and run only
    // the two scripts it carries (by hash).
    h.push_str(&format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{}\">", csp(&[head_js, app_js])));
    h.push_str(&format!("<script>{head_js}</script>"));
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    h.push_str(&format!("<title>Auto Test report - {}</title>", esc_attr(run.suite.as_ref().map_or(&scope, |r| &r.title))));
    h.push_str(&format!("<style>{TOKENS}\n{accent}{STYLE}</style></head><body data-start=\"{start}\">"));
    h.push_str("<a class=\"skip-link\" href=\"#main\">Skip to the report</a>");
    h.push_str("<div class=\"app\" data-rail=\"open\">");
    h.push_str(&report_view::topbar(&scope, run.environment.as_deref(), &when_tag, run.mode != "unattended", &counts_now, run_ms));
    h.push_str("<div class=\"app__body\">");
    h.push_str(&report_view::sidebar(failure_cases, flaky_cases, views.len(), env!("CARGO_PKG_VERSION"), &flaky_note));
    h.push_str("<main class=\"app__main\" id=\"main\" tabindex=\"-1\">");
    // On paper only: the run's name and its counts, ahead of everything.
    h.push_str(&format!(
        "<div class=\"print-only print-head\"><h1 class=\"page__title\">Auto Test report</h1><p>{}{} \u{b7} {}</p><p>{} passed, {} failed, {} flaky, {} blocked, {} not run</p></div>",
        esc_attr(&scope),
        run.environment.as_deref().filter(|e| !e.trim().is_empty()).map(|e| format!(" \u{b7} {}", esc_attr(e))).unwrap_or_default(),
        esc_attr(&when),
        counts_now.passed,
        counts_now.failed,
        counts_now.flaky,
        counts_now.blocked,
        counts_now.not_run
    ));
    h.push_str(&cases_view);
    h.push_str(&insights);
    h.push_str(&history_html);
    h.push_str(&report_view::gallery_view(shots, videos));
    h.push_str("</main></div></div>");
    h.push_str(&report_view::dialogs());
    h.push_str(&format!("<script type=\"application/json\" id=\"report-data\">{blob}</script>"));
    h.push_str(&format!("<script>{app_js}</script>"));
    h.push_str("</body></html>\n");
    h
}
