//! One run as a single HTML page a person can keep, print or attach to an
//! email: when it ran, where, what each case came to, and for each case
//! that failed or was blocked, the step that stopped it with its pictures.
//!
//! The page is self-contained - inline CSS, pictures embedded as data URLs,
//! no script and nothing fetched - so it opens the same anywhere, offline,
//! and never phones home. Every value from the run is escaped: a case title
//! is whatever someone typed into Azure DevOps.
//!
//! What never goes in: passwords, the values a script typed (`fill`'s
//! value), cookies, or any file's bytes other than the run's own failure
//! pictures - read only through `store`'s screenshot guard, so a name that
//! is not a screenshot in the shots folder is refused, never read. The
//! accounts are their keys, as the run recorded them, never logins.

use super::replay::{MODULE_STEP, SIGN_IN_STEP};
use super::{CaseRecord, CaseScript, LocalRun, StepRecord};
use crate::browser::actions::{Action, ActionOutcome};

/// The result buckets, in the order the report (and the Auto Run screen's
/// filter row) lists them.
pub const BUCKETS: [&str; 4] = ["Passed", "Failed", "Blocked", "Not run"];

/// The largest picture embedded. A failure screenshot is a JPEG of one
/// browser window, normally well under 300 KB; anything this big is not
/// one, and would make the page slow to open and to mail.
pub const MAX_SHOT_BYTES: usize = 3 * 1024 * 1024;

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
        Action::CheckUrl { contains } => format!("check the address contains \"{contains}\""),
        Action::ExpectVisible { selector, .. } => format!("expect {} to be visible", selector.describe()),
        Action::ExpectHidden { selector, .. } => format!("expect {} to be hidden", selector.describe()),
        Action::ExpectText { selector, equals, .. } => {
            format!("expect {} to read \"{equals}\"", selector.describe())
        }
        Action::ExpectContainsText { selector, value, .. } => {
            format!("expect {} to contain \"{value}\"", selector.describe())
        }
        Action::ExpectCount { selector, equals, .. } => {
            format!("expect {equals} of {}", selector.describe())
        }
        Action::ExpectAttribute { selector, name, equals, .. } => {
            format!("expect {}'s {name} to be \"{equals}\"", selector.describe())
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

/// Text into HTML, element content or a quoted attribute alike.
pub fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
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
fn stopped_action(case: &CaseRecord, step: &StepRecord, index: usize, script: Option<&CaseScript>) -> String {
    match step.step_number {
        SIGN_IN_STEP => match &case.account {
            Some(a) => format!("sign in as {a}"),
            None => "sign in".to_string(),
        },
        MODULE_STEP => "go to the case's module".to_string(),
        n => match script {
            None => "(the script for this case is no longer on this machine)".to_string(),
            Some(s) => s
                .steps
                .iter()
                .find(|st| st.step_number == n)
                .and_then(|st| st.actions.get(index))
                .map(action_words)
                .unwrap_or_else(|| {
                    format!("action {} (the script on this machine has changed since the run)", index + 1)
                }),
        },
    }
}

/// One picture as an `<img>`, or a line saying why it is not there.
/// `load` is only ever reached for a name the screenshot guard accepts.
fn picture(name: &str, load: &dyn Fn(&str) -> Result<Vec<u8>, String>) -> String {
    if !super::store::safe_shot_name(name) {
        return format!(
            "<p class=\"note\">A picture was skipped: {} is not a screenshot name.</p>",
            esc(name)
        );
    }
    let bytes = match load(name) {
        Ok(b) => b,
        Err(_) => {
            return format!(
                "<p class=\"note\">The picture {} is no longer on this machine.</p>",
                esc(name)
            )
        }
    };
    if bytes.len() > MAX_SHOT_BYTES {
        return format!(
            "<p class=\"note\">The picture {} was too large to include ({} KB).</p>",
            esc(name),
            bytes.len() / 1024
        );
    }
    let mime = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        "image/jpeg"
    } else {
        return format!(
            "<p class=\"note\">The picture {} could not be read as an image.</p>",
            esc(name)
        );
    };
    use base64::Engine;
    format!(
        "<figure><img src=\"data:{mime};base64,{}\" alt=\"Screenshot {}\"><figcaption>{}</figcaption></figure>",
        base64::engine::general_purpose::STANDARD.encode(&bytes),
        esc(name),
        esc(name)
    )
}

/// A Failed or Blocked case's block: the step that stopped it, the action
/// in words, the page's message, and that step's pictures.
fn failure_block(
    case: &CaseRecord,
    script: Option<&CaseScript>,
    load: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> String {
    let mut h = String::new();
    h.push_str("<section class=\"failure\">");
    h.push_str(&format!(
        "<h3><span class=\"id\">#{}</span> {} <span class=\"b-{}\">{}</span></h3>",
        case.case_id,
        esc(&case.title),
        css_key(case_bucket(case)),
        case_bucket(case)
    ));
    if !case.reason.is_empty() {
        h.push_str(&format!("<p><strong>Why:</strong> {}</p>", esc(&case.reason)));
    }
    match stopping_point(case) {
        Some((step, index, outcome)) => {
            h.push_str("<dl>");
            h.push_str(&format!("<dt>Stopped at</dt><dd>{}</dd>", esc(&step_label(step.step_number))));
            h.push_str(&format!(
                "<dt>Action</dt><dd>{}</dd>",
                esc(&stopped_action(case, step, index, script))
            ));
            h.push_str(&format!("<dt>Message</dt><dd>{}</dd>", esc(&outcome.detail)));
            h.push_str("</dl>");
            // The failed action's own picture first, then the step's.
            let mut names: Vec<&str> = Vec::new();
            for o in &step.outcomes {
                if !o.ok {
                    if let Some(n) = o.screenshot.as_deref() {
                        names.push(n);
                    }
                }
            }
            if let Some(n) = step.screenshot.as_deref() {
                names.push(n);
            }
            let mut seen = std::collections::HashSet::new();
            for n in names {
                if seen.insert(n) {
                    h.push_str(&picture(n, load));
                }
            }
        }
        None => h.push_str(
            "<p class=\"note\">No step failed on its own - this result was decided by the person reviewing the run.</p>",
        ),
    }
    if !case.note.is_empty() {
        h.push_str(&format!("<p><strong>Note:</strong> {}</p>", esc(&case.note)));
    }
    h.push_str("</section>");
    h
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
        format!("<a href=\"{}\">{}</a>", esc(url.trim()), esc(url.trim()))
    } else {
        esc(url)
    }
}

/// The page's Content-Security-Policy.
pub const CSP: &str = "default-src 'none'; img-src data:; style-src 'unsafe-inline'";

const STYLE: &str = "body{font-family:Segoe UI,Arial,sans-serif;color:#1f2328;background:#ffffff;margin:24px;font-size:14px;line-height:1.45}\
h1{font-size:22px;margin:0 0 12px}h2{font-size:17px;margin:24px 0 8px;border-bottom:1px solid #d0d7de;padding-bottom:4px}\
h3{font-size:15px;margin:0 0 6px}dl{display:grid;grid-template-columns:max-content 1fr;gap:2px 12px;margin:0 0 8px}\
dt{color:#59636e}dd{margin:0}table{border-collapse:collapse;width:100%}th,td{border:1px solid #d0d7de;padding:4px 8px;text-align:left;vertical-align:top}\
th{background:#f6f8fa}.id{font-family:Consolas,monospace;color:#59636e}.b-passed{color:#1a7f37;font-weight:600}\
.b-failed{color:#cf222e;font-weight:600}.b-blocked{color:#9a6700;font-weight:600}.b-notrun{color:#59636e;font-weight:600}\
.failure{border:1px solid #d0d7de;border-radius:6px;padding:10px 12px;margin:0 0 12px}.note{color:#59636e;font-style:italic}\
figure{margin:8px 0}figure img{max-width:100%;border:1px solid #d0d7de}figcaption{font-size:12px;color:#59636e}\
@media print{body{margin:0}.failure,tr,figure{break-inside:avoid}a{color:inherit}}";

/// The whole page. `ran_at` is the run's start as the person reads time
/// (the webview formats it in their own locale); blank falls back to UTC.
/// `scripts` supplies the words for a failed action and a case's area -
/// the scripts on this machine now, as `failures` reads them too. `load`
/// reads one screenshot by name; it is only called for a name the
/// screenshot guard accepts.
pub fn build(
    run: &LocalRun,
    scripts: &[CaseScript],
    ran_at: &str,
    load: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> String {
    let script_for = |id: i32| scripts.iter().find(|s| s.case_id == id);
    let when = if ran_at.trim().is_empty() { utc_time(&run.started_at) } else { ran_at.trim().to_string() };
    let mode = if run.mode == "unattended" { "Unattended" } else { "Supervised" };

    let mut h = String::new();
    h.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">");
    // Defence in depth: everything below is escaped and nothing is fetched,
    // but should either ever slip, the page still may run no script and
    // load nothing beyond its own embedded pictures and inline style.
    h.push_str(&format!("<meta http-equiv=\"Content-Security-Policy\" content=\"{CSP}\">"));
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    h.push_str(&format!("<title>Auto Run report - PBI #{}</title>", run.pbi_id));
    h.push_str(&format!("<style>{STYLE}</style></head><body>"));
    h.push_str("<h1>Auto Run report</h1><dl>");
    h.push_str(&format!("<dt>Ran</dt><dd>{}</dd>", esc(&when)));
    h.push_str(&format!("<dt>Mode</dt><dd>{mode}</dd>"));
    if let Some(env) = run.environment.as_deref().filter(|e| !e.trim().is_empty()) {
        h.push_str(&format!("<dt>Environment</dt><dd>{}</dd>", esc(env)));
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

    h.push_str("<h2>Cases</h2><table class=\"cases\"><thead><tr><th>ID</th><th>Title</th><th>Result</th><th>Decided by</th><th>Account</th><th>Area</th></tr></thead><tbody>");
    for case in &run.cases {
        let b = case_bucket(case);
        let decided = if !case.verdict.is_empty() {
            "Confirmed"
        } else if !case.proposed.is_empty() {
            "Proposed"
        } else {
            "-"
        };
        let area = script_for(case.case_id).and_then(CaseScript::area_name).unwrap_or("");
        h.push_str(&format!(
            "<tr><td class=\"id\">#{}</td><td>{}</td><td class=\"b-{}\">{b}</td><td>{decided}</td><td>{}</td><td>{}</td></tr>",
            case.case_id,
            esc(&case.title),
            css_key(b),
            esc(case.account.as_deref().unwrap_or("")),
            esc(area)
        ));
    }
    h.push_str("</tbody></table>");

    let failing: Vec<&CaseRecord> =
        run.cases.iter().filter(|c| matches!(case_bucket(c), "Failed" | "Blocked")).collect();
    if !failing.is_empty() {
        h.push_str("<h2>Failed and blocked cases</h2>");
        for case in failing {
            h.push_str(&failure_block(case, script_for(case.case_id), load));
        }
    }

    h.push_str("</body></html>\n");
    h
}
