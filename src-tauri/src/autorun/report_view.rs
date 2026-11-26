//! The Auto Test report's page: the shell (top bar, sidebar), the lists and
//! the detail of a case, Insights, History and the gallery's shell.
//!
//! All of it is plain HTML written here - the charts are inline SVG, the
//! timeline is boxes with a width - so the page reads, and prints, with no
//! script at all. The script (`web/autorun-report.js`) only adds the views'
//! switching, the filter, the keys, the copy buttons, the local times and
//! the gallery. Every value is escaped; the one blob of data is JSON that
//! cannot close its own `<script>` (see [`data_blob`]).

use std::collections::HashMap;

use super::report::css_key;
use super::report_smart::{
    duration_of, CaseTrend, Cluster, Compare, Health, RunPoint, Timeline, Trend, HISTORY_RUNS, MIN_JUDGED_FOR_GRADE,
};
use super::{CaseRecord, LocalRun};
use crate::html_escape::esc_attr;

/// How many case links one compare panel lists before it says "and N more".
const MAX_LISTED: usize = 40;

// ----------------------------------------------------------------------
// Icons
// ----------------------------------------------------------------------

/// A 16px line icon (stroke 1.5), drawn here.
pub(super) fn icon(name: &str) -> String {
    let body = match name {
        "sidebar" => "<rect x=\"2\" y=\"3\" width=\"12\" height=\"10\" rx=\"2\"/><path d=\"M6 3v10\"/>",
        "failed" => "<circle cx=\"8\" cy=\"8\" r=\"6\"/><path d=\"M5.8 5.8l4.4 4.4M10.2 5.8l-4.4 4.4\"/>",
        "passed" => "<circle cx=\"8\" cy=\"8\" r=\"6\"/><path d=\"M5.5 8.2l1.7 1.7 3.3-3.6\"/>",
        "flaky" => "<circle cx=\"8\" cy=\"8\" r=\"6\"/><path d=\"M8 2a6 6 0 0 1 0 12z\" fill=\"currentColor\"/>",
        "blocked" => "<circle cx=\"8\" cy=\"8\" r=\"6\"/><path d=\"M3.8 12.2l8.4-8.4\"/>",
        "notrun" => "<circle cx=\"8\" cy=\"8\" r=\"6\" stroke-dasharray=\"2 2\"/>",
        "shuffle" => "<path d=\"M2 4.5h2.5L10 11.5h4M2 11.5h2.5L7 8.4M9.5 5.6L10 4.5h4M12.5 3l1.5 1.5-1.5 1.5M12.5 10l1.5 1.5-1.5 1.5\"/>",
        "list" => "<path d=\"M3 4h10M3 8h10M3 12h10\"/>",
        "gauge" => "<path d=\"M2.5 11.5a5.5 5.5 0 1 1 11 0\"/><path d=\"M8 11.5l2.5-3.5\"/>",
        "history" => "<path d=\"M2.5 8a5.5 5.5 0 1 0 1.8-4\"/><path d=\"M2.5 2.5v3h3M8 5v3l2 1.5\"/>",
        "image" => "<rect x=\"2\" y=\"3\" width=\"12\" height=\"10\" rx=\"2\"/><circle cx=\"6\" cy=\"7\" r=\"1\"/><path d=\"M3 12l3.5-3.5 2.5 2.5 2-2 3 3\"/>",
        "search" => "<circle cx=\"7\" cy=\"7\" r=\"4.5\"/><path d=\"M10.5 10.5L14 14\"/>",
        "moon" => "<path d=\"M13 9.5A5.5 5.5 0 0 1 6.5 3 5.5 5.5 0 1 0 13 9.5z\"/>",
        "sun" => "<circle cx=\"8\" cy=\"8\" r=\"2.6\"/><path d=\"M8 1.5v1.5M8 13v1.5M1.5 8H3M13 8h1.5M3.4 3.4l1 1M11.6 11.6l1 1M12.6 3.4l-1 1M4.4 11.6l-1 1\"/>",
        "chevron-right" => "<path d=\"M6 4l4 4-4 4\"/>",
        "chevron-down" => "<path d=\"M4 6l4 4 4-4\"/>",
        "chevron-up" => "<path d=\"M4 10l4-4 4 4\"/>",
        "filter" => "<path d=\"M2.5 3.5h11L9.5 8.5V13l-3-1.5V8.5z\"/>",
        "copy" => "<rect x=\"5.5\" y=\"5.5\" width=\"8\" height=\"8\" rx=\"1.5\"/><path d=\"M10.5 5.5V4A1.5 1.5 0 0 0 9 2.5H4A1.5 1.5 0 0 0 2.5 4v5A1.5 1.5 0 0 0 4 10.5h1.5\"/>",
        "keyboard" => "<rect x=\"1.5\" y=\"4\" width=\"13\" height=\"8\" rx=\"1.5\"/><path d=\"M4 7h1M7 7h1M10 7h2M5 10h6\"/>",
        "video" => "<rect x=\"2\" y=\"4\" width=\"8\" height=\"8\" rx=\"1.5\"/><path d=\"M10 7l4-2v6l-4-2z\"/>",
        _ => "",
    };
    format!(
        "<svg class=\"icon\" width=\"16\" height=\"16\" viewBox=\"0 0 16 16\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linecap=\"round\" stroke-linejoin=\"round\" aria-hidden=\"true\">{body}</svg>"
    )
}

/// The status glyph of a case, in its tone.
pub(super) fn glyph(key: &str) -> String {
    format!("<span class=\"glyph tone-{}\">{}</span>", tone(key), icon(key))
}

/// A status key (`failed`, `passed`, `flaky`, `blocked`, `notrun`) as the
/// tone class suffix.
fn tone(key: &str) -> &'static str {
    match key {
        "failed" => "fail",
        "passed" => "pass",
        "flaky" => "flaky",
        "blocked" => "blocked",
        _ => "skip",
    }
}

// ----------------------------------------------------------------------
// One case, as the views read it
// ----------------------------------------------------------------------

/// Everything the views say about one case.
pub(super) struct CaseView<'a> {
    pub case: &'a CaseRecord,
    pub bucket: &'static str,
    /// A Failed result of a case marked flaky (the summary counts it as flaky).
    pub flaky_failure: bool,
    pub area: String,
    pub module: String,
    pub trend: Option<&'a CaseTrend>,
    pub cluster: Option<&'a Cluster>,
    /// Failing now and not failing in the previous run of this PBI.
    pub new_failure: bool,
    pub timeline: Option<Timeline>,
}

impl CaseView<'_> {
    /// The status the counts and the filter use: a flaky failure is `flaky`.
    pub fn status(&self) -> &'static str {
        if self.flaky_failure {
            "flaky"
        } else {
            css_key(self.bucket)
        }
    }

    /// Flaky in any sense the report knows: marked, or alternating lately.
    pub fn flaky(&self) -> bool {
        self.flaky_failure || self.trend.is_some_and(CaseTrend::flaky)
    }

    pub fn grade(&self) -> Option<char> {
        self.trend.and_then(|t| t.grade)
    }

    /// Failed or Blocked (a flaky failure included): what a printed report
    /// shows in full.
    pub fn is_failure(&self) -> bool {
        matches!(self.bucket, "Failed" | "Blocked")
    }

    pub fn slow(&self) -> bool {
        self.timeline.as_ref().is_some_and(|t| t.steps.iter().any(|s| s.slow))
    }

    /// The attributes the filter reads, with the leading space.
    pub fn attrs(&self, failure_text: &str) -> String {
        let cause = match self.cluster {
            Some(c) => format!("{} {} {}", c.label(), c.title, c.cause),
            None => String::new(),
        };
        let mut text = format!("{} {} {} {}", self.case.case_id, self.case.title, self.area, self.module);
        if let Some(a) = self.case.account.as_deref() {
            text.push(' ');
            text.push_str(a);
        }
        if !failure_text.is_empty() {
            text.push(' ');
            text.push_str(failure_text);
        }
        format!(
            " data-id=\"{}\" data-status=\"{}\" data-flaky=\"{}\" data-area=\"{}\" data-module=\"{}\" data-grade=\"{}\" data-cause=\"{}\" data-text=\"{}\"",
            self.case.case_id,
            self.status(),
            u8::from(self.flaky()),
            esc_attr(&self.area),
            esc_attr(&self.module),
            self.grade().map_or("-".to_string(), |g| g.to_string()),
            esc_attr(&cause),
            esc_attr(&text),
        )
    }
}

// ----------------------------------------------------------------------
// Small helpers
// ----------------------------------------------------------------------

/// Milliseconds the way the page says them: `733 ms`, `6.4 s`, `1m 39s`.
pub(super) fn took(ms: u64) -> String {
    if ms >= 60_000 {
        format!("{}m {:02}s", ms / 60_000, (ms % 60_000) / 1000)
    } else if ms >= 1000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{ms} ms")
    }
}

/// A run's start as a `<time>` the script rewrites in the reader's own
/// locale, in one short format everywhere; the text is UTC for anyone
/// reading it without the script.
pub(super) fn time_tag(started_at: &str) -> String {
    time_tag_as(started_at, &super::report::utc_time(started_at))
}

/// [`time_tag`] with the words to show without the script (the time the
/// webview formatted, when it sent one).
pub(super) fn time_tag_as(started_at: &str, text: &str) -> String {
    format!("<time data-ts=\"{}\">{}</time>", esc_attr(started_at.trim()), esc_attr(text))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ----------------------------------------------------------------------
// Shell
// ----------------------------------------------------------------------

/// The five numbers the top bar and the bar chart show: failed (without the
/// flaky ones), passed, flaky, blocked, not run - adding up to the total.
pub(super) struct Counts {
    pub failed: usize,
    pub passed: usize,
    pub flaky: usize,
    pub blocked: usize,
    pub not_run: usize,
}

impl Counts {
    pub fn total(&self) -> usize {
        self.failed + self.passed + self.flaky + self.blocked + self.not_run
    }
}

/// What a run was over, in words: `PBI #5`, or `suite <title>` for a run of a
/// test suite. Not escaped; each place escapes it as it prints.
pub(super) fn scope_label(run: &LocalRun) -> String {
    match &run.suite {
        Some(r) => format!("suite {}", r.title),
        None => format!("PBI #{}", run.pbi_id),
    }
}

/// `PBI` or `suite`: the word for what a run was over, for "this ...".
pub(super) fn scope_noun(run: &LocalRun) -> &'static str {
    if run.suite.is_some() { "suite" } else { "PBI" }
}

/// The top bar: sidebar toggle, the run's name and meta, the counts, the
/// duration, Search and the light/dark toggle.
pub(super) fn topbar(scope: &str, environment: Option<&str>, when_html: &str, attended: bool, c: &Counts, duration_ms: u64) -> String {
    let mut h = String::from(
        "<header class=\"topbar\"><button type=\"button\" class=\"icon-btn\" id=\"rail-toggle\" aria-label=\"Show or hide the sidebar\" title=\"Sidebar (Ctrl+B)\">",
    );
    h.push_str(&icon("sidebar"));
    h.push_str("</button><span class=\"topbar__brand\">Auto Test report</span><span class=\"topbar__run\">");
    h.push_str(&format!("<span class=\"topbar__meta mono\">{}</span>", esc_attr(scope)));
    if let Some(env) = environment.map(str::trim).filter(|e| !e.is_empty()) {
        h.push_str(&format!("<span class=\"topbar__meta\">{}</span>", esc_attr(env)));
    }
    h.push_str(&format!("<span class=\"topbar__meta\">{when_html}</span>"));
    if attended {
        h.push_str("<span class=\"topbar__meta\">Attended</span>");
    }
    h.push_str("</span><span class=\"topbar__spacer\"></span><nav class=\"topbar__verdict\" aria-label=\"Counts\">");
    let rows = [
        ("failed", c.failed, "failed", "failures", "", true),
        ("passed", c.passed, "passed", "all", "status:passed", true),
        ("flaky", c.flaky, "flaky", "flaky", "", false),
        ("blocked", c.blocked, "blocked", "all", "status:blocked", false),
        ("notrun", c.not_run, "not run", "all", "status:notrun", false),
    ];
    for (key, n, label, open, filter, always) in rows {
        if n == 0 && !always {
            continue;
        }
        h.push_str(&format!(
            "<button type=\"button\" class=\"count count--button\" data-open=\"{open}\" data-filter=\"{filter}\"><span class=\"tone-{t}\">{ic}</span><span class=\"count__value tone-{t}\">{n}</span><span class=\"count__label\">{label}</span></button>",
            t = tone(key),
            ic = icon(key),
        ));
    }
    h.push_str("</nav>");
    if duration_ms > 0 {
        h.push_str(&format!("<span class=\"topbar__duration mono\" title=\"Run duration\">{}</span>", took(duration_ms)));
    }
    h.push_str(&format!(
        "<button type=\"button\" class=\"btn\" id=\"search-btn\">{}<span class=\"hide-narrow\">Search</span><kbd class=\"kbd\">/</kbd></button>",
        icon("search")
    ));
    h.push_str(&format!(
        "<button type=\"button\" class=\"icon-btn\" id=\"theme-btn\" aria-label=\"Switch between light and dark\" title=\"Light or dark\"><span class=\"to-dark\">{}</span><span class=\"to-light\">{}</span></button>",
        icon("moon"),
        icon("sun")
    ));
    h.push_str("</header>");
    h
}

/// The sidebar. `failures` is the Failed and Blocked cases, `flaky` the
/// cases flaky in any sense, `all` every case.
pub(super) fn sidebar(failures: usize, flaky: usize, all: usize, version: &str, flaky_note: &str) -> String {
    let item = |nav: &str, ic: &str, label: &str, n: Option<usize>| {
        let title = if nav == "flaky" { format!(" title=\"{}\"", esc_attr(flaky_note)) } else { String::new() };
        format!(
            "<a class=\"rail__item\" href=\"#view-{nav}\" data-nav=\"{nav}\"{title}>{}<span class=\"rail__label\">{label}</span>{}</a>",
            icon(ic),
            n.map(|n| format!("<span class=\"rail__count\">{n}</span>")).unwrap_or_default()
        )
    };
    let mut h = String::from("<aside class=\"rail\" id=\"rail\" aria-label=\"Views\"><div class=\"rail__group\">");
    h.push_str(&item("failures", "failed", "Failures", Some(failures)));
    h.push_str(&item("flaky", "shuffle", "Flaky", Some(flaky)));
    h.push_str(&item("all", "list", "All cases", Some(all)));
    h.push_str("</div><div class=\"rail__group\">");
    h.push_str(&item("insights", "gauge", "Insights", None));
    h.push_str(&item("history", "history", "History", None));
    h.push_str(&item("gallery", "image", "Gallery", None));
    h.push_str(&format!(
        "</div><div class=\"rail__foot\"><button type=\"button\" class=\"rail__keys\" id=\"keys-btn\">{}<span>Keyboard shortcuts</span><kbd class=\"kbd\">?</kbd></button><p class=\"rail__note\">Test Case Manager {}</p></div></aside>",
        icon("keyboard"),
        esc_attr(version)
    ));
    h
}

// ----------------------------------------------------------------------
// Lists
// ----------------------------------------------------------------------

/// One case's row in a list.
pub(super) fn row(v: &CaseView, list: &str, failure_text: &str) -> String {
    let mut badges = String::new();
    if v.new_failure {
        badges.push_str("<span class=\"signal\">New failure</span>");
    }
    if v.flaky() {
        badges.push_str("<span class=\"signal\">Flaky</span>");
    }
    if v.slow() {
        badges.push_str("<span class=\"signal\">Slow</span>");
    }
    let ms = duration_of(v.case);
    format!(
        "<div class=\"row f-item\" role=\"option\" tabindex=\"-1\" aria-selected=\"false\" data-list=\"{list}\"{attrs}>{glyph}<span class=\"row__title\" title=\"{title}\">{title}</span>{badges}<span class=\"row__duration\">{dur}</span></div>",
        attrs = v.attrs(failure_text),
        glyph = glyph(v.status()),
        title = esc_attr(&v.case.title),
        dur = if ms > 0 { took(ms) } else { String::new() },
    )
}

/// A collapsible group of rows: a sticky header (its label, its count) and
/// the rows. `extra` is anything shown between the header and the rows.
pub(super) fn group(label: &str, mono: bool, count: usize, rows: &str, extra: &str) -> String {
    format!(
        "<section class=\"grp\"><button type=\"button\" class=\"group\" aria-expanded=\"true\"><span class=\"chev\">{}</span><span class=\"group__label{}\">{}</span><span class=\"group__count\">{count}</span></button>{extra}<div class=\"grp__rows\">{rows}</div></section>",
        icon("chevron-right"),
        if mono { " mono" } else { "" },
        esc_attr(label),
    )
}

/// The middle pane's header and filter, the lists inside it.
pub(super) fn list_pane(lists: &str) -> String {
    format!(
        "<div class=\"pane__list\"><div class=\"pane__head\"><h1 class=\"pane__title\" id=\"list-title\">Failures</h1><span class=\"pane__count\" id=\"list-count\"></span><span class=\"pane__sub\" id=\"list-sub\"></span></div>\
<div class=\"filterbar\"><span class=\"filterbar__icon\">{}</span>\
<input id=\"filter\" class=\"filterbar__input\" type=\"search\" autocomplete=\"off\" aria-label=\"Filter cases\" placeholder=\"Filter: text, status:, area:, module:, grade:, cause:\">\
<kbd class=\"kbd\">/</kbd></div><div class=\"pane__scroll\" role=\"listbox\" aria-label=\"Cases\">{lists}</div></div>",
        icon("filter")
    )
}

// ----------------------------------------------------------------------
// Detail
// ----------------------------------------------------------------------

/// A collapsible section of the detail.
pub(super) fn section(title: &str, meta: &str, open: bool, body: &str) -> String {
    format!(
        "<details class=\"section\"{}><summary class=\"section__header\"><span class=\"chev\">{}</span><span class=\"section__title\">{title}</span><span class=\"section__meta\">{}</span></summary><div class=\"section__body\">{body}</div></details>",
        if open { " open" } else { "" },
        icon("chevron-right"),
        esc_attr(meta),
    )
}

/// The Timeline section's body: the callout, one track of the steps laid
/// end to end, and its axis.
pub(super) fn timeline_body(t: &Timeline) -> String {
    if t.steps.is_empty() {
        return "<p class=\"muted\">No steps were recorded for this case.</p>".to_string();
    }
    if t.total_ms == 0 {
        return "<p class=\"muted\">The run did not time this case's steps.</p>".to_string();
    }
    let total = t.total_ms as f64;
    let mut h = String::from("<div class=\"timeline\">");
    if let Some(i) = t.anomaly {
        let s = &t.steps[i];
        let why = if s.failed {
            "failed".to_string()
        } else {
            match (s.ms, s.median_ms) {
                (Some(ms), Some(m)) if m > 0 => format!("{:.1}\u{d7} slower than usual ({})", ms as f64 / m as f64, took(m)),
                _ => "slower than usual".to_string(),
            }
        };
        h.push_str(&format!(
            "<div class=\"timeline__anomaly\"><span class=\"timeline__anomaly-label\">Possible first anomaly</span><span>{}</span><span class=\"mono muted\">at {}</span><span class=\"muted\">{}</span></div>",
            esc_attr(&s.label),
            took(t.anomaly_at_ms),
            esc_attr(&why),
        ));
    }
    // The anomaly is named on the track itself, not only by a colour and the
    // callout above: a label over the marker, kept inside the lane's ends.
    let marker_label = if t.anomaly.is_some() {
        let pct = t.anomaly_at_ms as f64 / total * 100.0;
        let edge = if pct < 12.0 {
            " data-edge=\"start\""
        } else if pct > 88.0 {
            " data-edge=\"end\""
        } else {
            ""
        };
        format!("<div class=\"timeline__flags\"><span class=\"timeline__flag\"{edge} style=\"left:{pct:.2}%\">First anomaly</span></div>")
    } else {
        String::new()
    };
    let spacer = if t.anomaly.is_some() { "<span></span>" } else { "" };
    h.push_str(&format!(
        "<div class=\"timeline__grid\"><div class=\"timeline__labels\">{spacer}<span>Steps</span></div><div><div class=\"timeline__track\">{marker_label}<div class=\"timeline__lane\" role=\"img\" aria-label=\"Steps on a time axis\">"
    ));
    let mut at = 0u64;
    for s in &t.steps {
        let ms = s.ms.unwrap_or(0);
        let flag = if s.failed {
            " data-failed"
        } else if s.slow {
            " data-slow"
        } else {
            ""
        };
        h.push_str(&format!(
            "<span class=\"timeline__bar\"{flag} title=\"{t}\" style=\"left:{:.2}%;width:{:.2}%\"><span class=\"sr-only\">{t}</span></span>",
            at as f64 / total * 100.0,
            ms as f64 / total * 100.0,
            t = esc_attr(&format!("{}: {}", s.label, took(ms))),
        ));
        at += ms;
    }
    if t.anomaly.is_some() {
        h.push_str(&format!("<span class=\"timeline__marker\" style=\"left:{:.2}%\"></span>", t.anomaly_at_ms as f64 / total * 100.0));
    }
    h.push_str("</div></div><div class=\"timeline__axis\">");
    for k in 0..5u64 {
        let ms = t.total_ms * k / 4;
        let edge = match k {
            0 => " data-edge=\"start\"",
            4 => " data-edge=\"end\"",
            _ => "",
        };
        h.push_str(&format!("<span class=\"timeline__tick\"{edge} style=\"left:{}%\">{}</span>", k * 25, took(ms)));
    }
    h.push_str("</div></div></div></div>");
    h
}

/// The History section's body: the strip, the grade and its explanation.
pub(super) fn history_body(t: Option<&CaseTrend>) -> String {
    let Some(t) = t.filter(|t| !t.strip.is_empty()) else {
        return "<p class=\"muted\">No earlier runs of this case on this machine.</p>".to_string();
    };
    let word = |c: &char| match c {
        'P' => "passed",
        'F' => "failed",
        'B' => "blocked",
        _ => "not run",
    };
    let n = t.strip.len();
    let cells: String = t
        .strip
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let tone = match c {
                'P' => "pass",
                'F' => "fail",
                'B' => "blocked",
                _ => "skip",
            };
            let when = match n - 1 - i {
                0 => "this run".to_string(),
                1 => "1 run ago".to_string(),
                k => format!("{k} runs ago"),
            };
            format!("<span class=\"strip__cell seg-{tone}\" title=\"{when}: {}\"></span>", word(c))
        })
        .collect();
    let grade = match t.grade {
        Some(g) => format!("<span class=\"mono\">Grade {g}</span> <span class=\"muted\">- {} in {} judged runs</span>", plural(t.flips, "flip", "flips"), t.judged),
        None => format!("<span class=\"muted\">Not graded: fewer than {MIN_JUDGED_FOR_GRADE} judged (passed or failed) runs yet.</span>"),
    };
    let flaky = if t.marked {
        " <span class=\"signal\">Marked flaky</span>"
    } else if t.alternates {
        " <span class=\"signal\">Flaky in its recent runs</span>"
    } else {
        ""
    };
    let last = t.strip.last().map_or("not run", word);
    format!(
        "<div class=\"history\"><div class=\"strip\" role=\"img\" aria-label=\"Outcome of its last {n} runs, oldest first; the last one {last}\">{cells}</div><p class=\"strip__caption muted\">oldest \u{2192} newest; last: {last}</p><p>{grade}{flaky}</p><p class=\"muted\">Oldest to newest, up to {HISTORY_RUNS} runs in this environment. A grade comes from the pass rate and how often the result flipped (A steady, F unreliable).</p></div>"
    )
}

// ----------------------------------------------------------------------
// Insights
// ----------------------------------------------------------------------

/// A sparkline (96 by 24) of `values`; empty for fewer than two. Under it,
/// its smallest and largest value as text, and the values themselves in the
/// accessible name, so no fact lives only in the shape. `fmt` writes one
/// value the way the tile writes its own.
pub(super) fn sparkline(values: &[f64], label: &str, fmt: &dyn Fn(f64) -> String) -> String {
    if values.len() < 2 {
        return String::new();
    }
    let (w, h, pad) = (96.0_f64, 24.0_f64, 2.0_f64);
    let lo = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = if hi - lo == 0.0 { 1.0 } else { hi - lo };
    let pts: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| (pad + i as f64 / (values.len() - 1) as f64 * (w - pad * 2.0), pad + (1.0 - (v - lo) / span) * (h - pad * 2.0)))
        .collect();
    let line: Vec<String> = pts.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
    let (lx, ly) = pts[pts.len() - 1];
    let range = if lo == hi { fmt(lo) } else { format!("{}\u{2013}{}", fmt(lo), fmt(hi)) };
    let all: Vec<String> = values.iter().map(|v| fmt(*v)).collect();
    format!(
        "<svg class=\"spark\" width=\"96\" height=\"24\" viewBox=\"0 0 96 24\" role=\"img\" aria-label=\"{}: {}, oldest first\"><polyline points=\"{}\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/><circle class=\"spark__last\" cx=\"{lx:.1}\" cy=\"{ly:.1}\" r=\"2\"/></svg><span class=\"spark__range\">{}</span>",
        esc_attr(label),
        esc_attr(&all.join(", ")),
        line.join(" "),
        esc_attr(&range),
    )
}

fn stat(label: &str, value: &str, sub: &str) -> String {
    format!(
        "<div class=\"stat\"><span class=\"stat__label\">{label}</span><span class=\"stat__value\">{}</span><span class=\"stat__sub\">{sub}</span></div>",
        esc_attr(value)
    )
}

/// The row of stat tiles.
pub(super) fn stats(points: &[RunPoint], new_cases: Option<usize>, total: usize, health: Option<&Health>, noun: &str) -> String {
    let last = points.last();
    let rate: Vec<f64> = points.iter().map(|p| f64::from(p.pass_rate.unwrap_or(0))).collect();
    let dur: Vec<f64> = points.iter().map(|p| p.duration_ms as f64).collect();
    let flaky: Vec<f64> = points.iter().map(|p| p.flaky as f64).collect();
    let mut h = String::from("<div class=\"stats\">");
    h.push_str(&stat(
        "Pass rate",
        &last.and_then(|p| p.pass_rate).map_or("-".to_string(), |r| format!("{r}%")),
        &sparkline(&rate, "Pass rate of the recent runs", &|v| format!("{v:.0}%")),
    ));
    h.push_str(&stat(
        "Duration",
        &last.filter(|p| p.duration_ms > 0).map_or("-".to_string(), |p| took(p.duration_ms)),
        &sparkline(&dur, "Duration of the recent runs", &|v| took(v as u64)),
    ));
    let sub = match new_cases {
        Some(0) => "no new cases".to_string(),
        Some(n) => plural(n, "new case", "new cases"),
        None => format!("first run of this {noun}"),
    };
    h.push_str(&stat("Cases", &total.to_string(), &sub));
    h.push_str(&stat("Flaky", &last.map_or(0, |p| p.flaky).to_string(), &sparkline(&flaky, "Flaky failures of the recent runs", &|v| format!("{v:.0}"))));
    match health {
        Some(hl) => h.push_str(&stat(
            "Health",
            &hl.letter.to_string(),
            &format!("{} \u{b7} pass {} \u{b7} stability {} \u{b7} speed {}", hl.score, hl.pass, hl.stability, hl.speed),
        )),
        None => h.push_str(&stat("Health", "-", "no result yet")),
    }
    h.push_str("</div>");
    h
}

/// The full-width segmented bar and its legend.
pub(super) fn minibar(c: &Counts) -> String {
    if c.total() == 0 {
        return String::new();
    }
    let parts = [
        ("pass", "passed", c.passed),
        ("fail", "failed", c.failed),
        ("flaky", "flaky", c.flaky),
        ("blocked", "blocked", c.blocked),
        ("skip", "not run", c.not_run),
    ];
    let mut track = String::new();
    let mut labels = String::new();
    for (t, name, n) in parts {
        if n == 0 {
            continue;
        }
        track.push_str(&format!("<span class=\"minibar__seg seg-{t}\" style=\"flex-grow:{n}\" title=\"{n} {name}\"></span>"));
        labels.push_str(&format!("<span><span class=\"minibar__value tone-{t}\">{n}</span> {name}</span>"));
    }
    format!(
        "<div class=\"stats-bar\"><div class=\"minibar\"><div class=\"minibar__track\" role=\"img\" aria-label=\"Outcome of the run\">{track}</div><div class=\"minibar__labels\">{labels}</div></div></div>"
    )
}

/// A panel of the Insights grid: a title, a right-hand note, its lines.
pub(super) fn panel(title: &str, meta: &str, lines: &[String], empty: &str) -> String {
    let body = if lines.is_empty() {
        format!("<p class=\"panel__empty\">{empty}</p>")
    } else {
        format!("<ul class=\"lines\">{}</ul>", lines.iter().map(|l| format!("<li class=\"lines__item\">{l}</li>")).collect::<String>())
    };
    format!(
        "<div class=\"panel\"><div class=\"panel__head\"><h2 class=\"panel__title\">{title}</h2><span class=\"panel__meta\">{}</span></div>{body}</div>",
        esc_attr(meta)
    )
}

/// The title without the start every case shares (when it has it).
fn label_of<'a>(title: &'a str, shared: &str) -> &'a str {
    if shared.is_empty() { title } else { title.strip_prefix(shared).unwrap_or(title) }
}

/// A case as a line in a panel: its status glyph, its title as a link, a value.
/// `shared` is the title start every case of the run has: the label drops it,
/// the hover title keeps the whole title.
pub(super) fn case_line(status: &str, id: i32, title: &str, value: &str, shared: &str) -> String {
    format!(
        "{}<a class=\"lines__label\" href=\"#case-{id}\" data-case=\"{id}\" title=\"{t}\">{l}</a><span class=\"lines__value mono\">{}</span>",
        glyph(status),
        esc_attr(value),
        t = esc_attr(title),
        l = esc_attr(label_of(title, shared)),
    )
}

// ----------------------------------------------------------------------
// History
// ----------------------------------------------------------------------

/// One trend chart: its title and this run's value, bars in a muted
/// neutral, this run's bar in the text colour, which stands out from them
/// by 3:1 or more in both modes (amber when `flaky`), the
/// three-run average over the bars, the dates under it.
fn chart(title: &str, values: &[(f64, &RunPoint, String)], flaky: bool) -> String {
    let n = values.len();
    let current = values.last().map(|v| v.2.clone()).unwrap_or_default();
    let (w, h, gap) = (300.0_f64, 64.0_f64, 4.0_f64);
    let bw = ((w - gap * (n as f64 - 1.0)) / n as f64).max(1.0);
    let max = values.iter().map(|v| v.0).fold(0.0_f64, f64::max);
    let max = if max <= 0.0 { 1.0 } else { max };
    let mut svg = format!(
        "<svg class=\"trend__svg\" viewBox=\"0 0 {w} {h}\" role=\"img\" aria-label=\"{} per run, scale from zero\">",
        esc_attr(title)
    );
    svg.push_str(&format!("<line class=\"trend__axis\" x1=\"0\" y1=\"{h}\" x2=\"{w}\" y2=\"{h}\"/>"));
    let mut avg: Vec<String> = Vec::new();
    for (i, (v, p, label)) in values.iter().enumerate() {
        let bh = (v / max * (h - 4.0)).clamp(0.0, h - 4.0).max(if *v > 0.0 { 1.0 } else { 0.0 });
        let x = i as f64 * (bw + gap);
        let last = i == n - 1;
        let class = match (last, flaky) {
            (true, true) => "trend__bar trend__bar--last tone-flaky",
            (true, false) => "trend__bar trend__bar--last",
            _ => "trend__bar",
        };
        svg.push_str(&format!(
            "<rect class=\"{class}\" x=\"{x:.1}\" y=\"{:.1}\" width=\"{bw:.1}\" height=\"{bh:.1}\"><title data-ts=\"{}\" data-val=\"{}\">{}: {}</title></rect>",
            h - bh,
            esc_attr(p.started_at.trim()),
            esc_attr(label),
            esc_attr(&super::report::utc_time(&p.started_at)),
            esc_attr(label),
        ));
        let from = i.saturating_sub(2);
        let mean = values[from..=i].iter().map(|v| v.0).sum::<f64>() / (i - from + 1) as f64;
        avg.push(format!("{:.1},{:.1}", x + bw / 2.0, h - (mean / max * (h - 4.0)).clamp(0.0, h - 4.0)));
    }
    if n >= 2 {
        svg.push_str(&format!("<polyline class=\"trend__avg\" fill=\"none\" points=\"{}\"/>", avg.join(" ")));
    }
    svg.push_str("</svg>");
    let dates = format!(
        "<div class=\"trend__dates\">{}{}</div>",
        values.first().map(|v| time_tag(&v.1.started_at)).unwrap_or_default(),
        if n > 1 { values.last().map(|v| time_tag(&v.1.started_at)).unwrap_or_default() } else { String::new() },
    );
    format!(
        "<figure class=\"trend\"><div class=\"trend__head\"><span class=\"trend__title\">{title}</span><span class=\"trend__value\">{}</span></div>{svg}{dates}</figure>",
        esc_attr(&current)
    )
}

/// The History view: Trends and Compare, then the per-run table. `titles`
/// is every case id the compare may name, with its title and its status key
/// in this run.
pub(super) fn history_view(trend: &Trend, scope: &str, noun: &str, environment: Option<&str>, titles: &HashMap<i32, (String, &'static str)>, shared: &str) -> String {
    let env = environment.map(str::trim).filter(|e| !e.is_empty()).map_or("this environment".to_string(), |e| e.to_string());
    let n = trend.points.len();
    let mut h = format!(
        "<section class=\"view page\" id=\"view-history\"><div class=\"page__head\"><h1 class=\"page__title\">History</h1><span class=\"muted\">{} of {} in {}</span></div>",
        plural(n, "recorded run", "recorded runs"),
        esc_attr(scope),
        esc_attr(&env)
    );
    h.push_str("<div class=\"tabs\" role=\"tablist\"><button type=\"button\" class=\"tab\" role=\"tab\" aria-selected=\"true\" data-tab=\"trends\">Trends</button><button type=\"button\" class=\"tab\" role=\"tab\" aria-selected=\"false\" data-tab=\"compare\">Compare</button></div>");

    // Trends.
    h.push_str("<div class=\"hist\" id=\"hist-trends\" role=\"tabpanel\">");
    if n < 2 {
        h.push_str(&format!("<p class=\"trends__note muted\">Only one run of this {noun} is on this machine in this environment, so there is no trend to draw yet.</p>"));
    } else {
        h.push_str("<p class=\"trends__note muted\">Line: three-run average. Bars start at zero; the highlighted bar is this run.</p><div class=\"trends\">");
        let pick = |f: &dyn Fn(&RunPoint) -> (f64, String)| -> Vec<(f64, &RunPoint, String)> {
            trend
                .points
                .iter()
                .map(|p| {
                    let (v, s) = f(p);
                    (v, p, s)
                })
                .collect()
        };
        let rate = pick(&|p| (f64::from(p.pass_rate.unwrap_or(0)), p.pass_rate.map_or("no result".to_string(), |r| format!("{r}%"))));
        let dur = pick(&|p| (p.duration_ms as f64, if p.duration_ms == 0 { "not timed".to_string() } else { took(p.duration_ms) }));
        let flaky = pick(&|p| (p.flaky as f64, plural(p.flaky, "flaky case", "flaky cases")));
        let failed = pick(&|p| (p.failed as f64, plural(p.failed, "failed case", "failed cases")));
        h.push_str(&chart("Pass rate", &rate, false));
        h.push_str(&chart("Duration", &dur, false));
        h.push_str(&chart("Flaky cases", &flaky, true));
        h.push_str(&chart("Failed cases", &failed, false));
        h.push_str("</div>");
    }
    h.push_str("</div>");

    // Compare.
    h.push_str("<div class=\"hist\" id=\"hist-compare\" role=\"tabpanel\" hidden>");
    match &trend.compare {
        None => h.push_str(&format!("<p class=\"compare__base muted\">This is the first run of {} in {}.</p>", esc_attr(scope), esc_attr(&env))),
        Some(c) => h.push_str(&compare_html(c, noun, titles, shared)),
    }
    h.push_str("</div>");

    // The runs.
    h.push_str("<h2 class=\"panel__title runs__title\">Runs</h2><table class=\"runs\"><thead><tr><th>Ran</th><th>Total</th><th>Passed</th><th>Failed</th><th>Flaky</th><th>Blocked</th><th>Not run</th><th>Pass rate</th><th>Duration</th></tr></thead><tbody>");
    for p in trend.points.iter().rev() {
        let now = if p.current { " <span class=\"signal\">this run</span>" } else { "" };
        h.push_str(&format!(
            "<tr><td>{}{now}</td><td>{}</td><td class=\"tone-pass\">{}</td><td class=\"tone-fail\">{}</td><td class=\"tone-flaky\">{}</td><td class=\"tone-blocked\">{}</td><td class=\"tone-skip\">{}</td><td>{}</td><td>{}</td></tr>",
            time_tag(&p.started_at),
            p.total,
            p.passed,
            p.failed,
            p.flaky,
            p.blocked,
            p.not_run,
            p.pass_rate.map_or("-".to_string(), |r| format!("{r}%")),
            if p.duration_ms == 0 { "-".to_string() } else { took(p.duration_ms) },
        ));
    }
    h.push_str("</tbody></table></section>");
    h
}

fn compare_list(title: &str, list: &[i32], titles: &HashMap<i32, (String, &'static str)>, status: &str, shared: &str) -> String {
    let lines: Vec<String> = list
        .iter()
        .take(MAX_LISTED)
        .map(|id| {
            let (t, now) = titles.get(id).cloned().unwrap_or((format!("Case {id}"), "notrun"));
            let st = if status.is_empty() { now } else { status };
            format!("{}<a class=\"lines__label\" href=\"#case-{id}\" data-case=\"{id}\" title=\"{t}\">#{id} {l}</a>", glyph(st), t = esc_attr(&t), l = esc_attr(label_of(&t, shared)))
        })
        .chain((list.len() > MAX_LISTED).then(|| format!("<span class=\"muted\">and {} more</span>", list.len() - MAX_LISTED)))
        .collect();
    panel(title, &list.len().to_string(), &lines, "None this run.")
}

fn compare_html(c: &Compare, noun: &str, titles: &HashMap<i32, (String, &'static str)>, shared: &str) -> String {
    let mut h = format!(
        "<p class=\"compare__base muted\">Against the previous run of this {noun}, which started {}.</p><div class=\"panels\" id=\"compare-panels\">",
        time_tag(&c.previous_started_at)
    );
    // A blocked case shows the blocked icon: the status is the case's own.
    h.push_str(&compare_list("New failures", &c.new_failures, titles, "", shared));
    h.push_str(&compare_list("Fixed", &c.fixed, titles, "passed", shared));
    h.push_str(&compare_list("Still failing", &c.still_failing, titles, "", shared));
    h.push_str(&compare_list("New cases", &c.new_cases, titles, "", shared));
    // A case gone from this run is not in it, so it links nowhere.
    let gone: Vec<String> = c
        .missing
        .iter()
        .take(MAX_LISTED)
        .map(|id| format!("{}<span class=\"lines__label\">#{id} {}</span>", glyph("notrun"), esc_attr(label_of(c.titles.get(id).map(String::as_str).unwrap_or_default(), shared))))
        .collect();
    h.push_str(&panel("Not in this run", &c.missing.len().to_string(), &gone, "None this run."));
    h.push_str("</div>");
    h
}

// ----------------------------------------------------------------------
// Gallery, dialogs, data
// ----------------------------------------------------------------------

/// The gallery's shell: its tabs, and a grid built by the page's script
/// from the data blob.
pub(super) fn gallery_view(pictures: usize, videos: usize) -> String {
    let body = if pictures + videos == 0 {
        "<p class=\"muted\">No pictures or videos were kept for this run.</p>".to_string()
    } else {
        format!(
            "<div class=\"tabs\" role=\"tablist\"><button type=\"button\" class=\"tab\" role=\"tab\" aria-selected=\"true\" data-gtab=\"all\">All <span class=\"tab__count\">{}</span></button>\
<button type=\"button\" class=\"tab\" role=\"tab\" aria-selected=\"false\" data-gtab=\"shot\">Screenshots <span class=\"tab__count\">{pictures}</span></button>\
<button type=\"button\" class=\"tab\" role=\"tab\" aria-selected=\"false\" data-gtab=\"video\">Videos <span class=\"tab__count\">{videos}</span></button></div>\
<ul class=\"gallery\" id=\"gal-grid\"></ul><p><button type=\"button\" class=\"btn\" id=\"gal-more\" hidden>Show more</button></p>",
            pictures + videos
        )
    };
    format!(
        "<section class=\"view page\" id=\"view-gallery\"><div class=\"page__head\"><h1 class=\"page__title\">Gallery</h1><span class=\"muted\">{}</span></div>{body}</section>",
        plural(pictures + videos, "picture or video", "pictures and videos")
    )
}

/// The keyboard list and the lightbox, both closed.
pub(super) fn dialogs() -> String {
    let key = |keys: &str, what: &str| format!("<dt>{keys}</dt><dd>{what}</dd>");
    let k = |s: &str| format!("<kbd class=\"kbd\">{s}</kbd>");
    format!(
        "<div class=\"overlay\" id=\"help\" role=\"dialog\" aria-modal=\"true\" aria-label=\"Keyboard shortcuts\" hidden><div class=\"dialog\"><div class=\"dialog__header\"><span class=\"dialog__title\">Keyboard shortcuts</span><button type=\"button\" class=\"icon-btn\" data-close aria-label=\"Close\">\u{d7}</button></div>\
<div class=\"keymap\"><div><h3 class=\"keymap__title\">Move</h3><dl class=\"keymap__list\">{}{}{}{}{}</dl></div>\
<div><h3 class=\"keymap__title\">Views</h3><dl class=\"keymap__list\">{}{}{}</dl></div>\
<div><h3 class=\"keymap__title\">Case</h3><dl class=\"keymap__list\">{}{}{}</dl></div></div>\
<p class=\"keymap__note muted\">The filter takes words and <code>status:</code> (passed, failed, blocked, notrun, flaky), <code>is:flaky</code> (marked, or passed and failed lately), <code>area:</code>, <code>module:</code>, <code>grade:</code> and <code>cause:</code> (words from a cause). Quote a value with spaces.</p></div></div>\
<div class=\"overlay lb\" id=\"lb\" role=\"dialog\" aria-modal=\"true\" aria-label=\"Picture\" hidden><div class=\"lightbox\"><div class=\"stage\"></div><div class=\"lightbox__nav\"><button type=\"button\" class=\"btn lb-prev\">Previous</button><span class=\"lb-cap muted\"></span><button type=\"button\" class=\"btn lb-next\">Next</button><button type=\"button\" class=\"btn lb-x\">Close</button></div></div></div>",
        key(&format!("{} {}", k("j"), k("k")), "Next and previous case"),
        key(&k("Enter"), "Show the case"),
        key(&format!("{} {}{}", k("/"), k("Ctrl"), k("K")), "Go to the filter"),
        key(&format!("{}{}", k("Ctrl"), k("B")), "Show or hide the sidebar"),
        key(&k("Esc"), "Close a window, clear the filter"),
        key(&format!("{}\u{2013}{}", k("1"), k("5")), "Failures, All cases, Insights, History, Gallery"),
        key(&format!("{} {}", k("g"), k("f")), "Go to Failures"),
        key(&format!("{} {} {}", k("g"), k("l"), k("i")), "Flaky, Insights (and h: History)"),
        key(&k("c"), "Copy the failure summary"),
        key(&k("?"), "Show or hide this list"),
        key(&format!("{} {}", k("\u{2190}"), k("\u{2192}")), "Previous and next picture"),
    )
}

/// JSON for an inline `<script type="application/json">`: no `<`, `>` or `&`
/// is ever written raw (they become `<`...), so no value - a case title
/// with `</script>` in it, say - can close the tag or open a comment. The
/// line and paragraph separators are escaped too, for older engines.
pub fn data_blob(json: &serde_json::Value) -> String {
    json.to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
