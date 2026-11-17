//! The parts of the Auto Run report that are new on top of the per-case
//! sections: the tabs, the overview, failures in clusters with a timeline
//! per case, the history, the gallery's shell and the data the page's own
//! script reads.
//!
//! All of it is plain HTML written here - the charts are inline SVG, the
//! timelines are boxes with a width - so the page reads, and prints, with
//! no script at all. The script (`web/autorun-report.js`) only adds the
//! tabs, the filter, the keyboard, the copy buttons, the gallery and its
//! lightbox. Every value is escaped; the one blob of data is JSON that
//! cannot close its own `<script>` (see [`data_blob`]).

use super::flaky::FLAKY_WINDOW;
use super::report::{css_key, utc_time};
use super::report_smart::{Cluster, Compare, Failure, RunPoint, Timeline, Trend, CaseTrend, HISTORY_RUNS};
use super::CaseRecord;
use crate::html_escape::esc_attr;

/// How many case numbers one compare box lists before it says "and N more".
const MAX_LISTED: usize = 40;

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
    pub failure: Option<&'a Failure>,
}

impl CaseView<'_> {
    /// Flaky in any sense the report knows: marked, or alternating lately.
    pub fn flaky(&self) -> bool {
        self.flaky_failure || self.trend.is_some_and(CaseTrend::flaky)
    }

    pub fn grade(&self) -> Option<char> {
        self.trend.and_then(|t| t.grade)
    }

    /// The attributes the filter reads, with the leading space.
    pub fn attrs(&self) -> String {
        let cause = match (self.cluster, self.failure) {
            (Some(c), _) => format!("{} {} {}", c.label(), c.action, c.cause),
            _ => String::new(),
        };
        let mut text = format!("{} {} {} {}", self.case.case_id, self.case.title, self.area, self.module);
        if let Some(a) = self.case.account.as_deref() {
            text.push(' ');
            text.push_str(a);
        }
        if let Some(f) = self.failure {
            text.push_str(&format!(" {} {} {}", f.step, f.action, f.message));
        }
        format!(
            " data-id=\"{}\" data-status=\"{}\" data-flaky=\"{}\" data-area=\"{}\" data-module=\"{}\" data-grade=\"{}\" data-cause=\"{}\" data-text=\"{}\"",
            self.case.case_id,
            css_key(self.bucket),
            u8::from(self.flaky()),
            esc_attr(&self.area),
            esc_attr(&self.module),
            self.grade().map_or("-".to_string(), |g| g.to_string()),
            esc_attr(&cause),
            esc_attr(&text),
        )
    }
}

/// The page's tab bar. The counts are of Failed and Blocked cases and of
/// pictures and videos.
pub(super) fn tabs(failed_cases: usize, pictures: usize) -> String {
    let failures = if failed_cases > 0 { format!("Failures ({failed_cases})") } else { "Failures".to_string() };
    let gallery = if pictures > 0 { format!("Gallery ({pictures})") } else { "Gallery".to_string() };
    format!(
        "<nav class=\"tabs\" aria-label=\"Report views\">\
<button type=\"button\" data-view=\"overview\" aria-selected=\"true\">Overview</button>\
<button type=\"button\" data-view=\"failures\" aria-selected=\"false\">{failures}</button>\
<button type=\"button\" data-view=\"history\" aria-selected=\"false\">History</button>\
<button type=\"button\" data-view=\"gallery\" aria-selected=\"false\">{gallery}</button>\
<button type=\"button\" data-view=\"all\" aria-selected=\"false\">All cases</button></nav>\
<div class=\"toolbar\" hidden>\
<input id=\"filter\" type=\"search\" autocomplete=\"off\" aria-label=\"Filter cases\" \
placeholder=\"Filter: words, status:failed, area:, module:, grade:, cause:\">\
<span id=\"filter-count\" class=\"hint\" aria-live=\"polite\"></span>\
<span class=\"hint\">Press ? for the keys</span></div>"
    )
}

/// One outcome bar: a segment per bucket, with the flaky failures split out
/// of the failed ones, as the summary table does.
pub(super) fn outcome_bar(c: [usize; 4], flaky: usize) -> String {
    let total: usize = c.iter().sum::<usize>() + flaky;
    if total == 0 {
        return String::new();
    }
    let parts = [
        ("o-passed", "Passed", c[0]),
        ("o-failed", "Failed", c[1]),
        ("o-flaky", "Flaky", flaky),
        ("o-blocked", "Blocked", c[2]),
        ("o-notrun", "Not run", c[3]),
    ];
    let mut bar = String::from("<div class=\"outcome\" role=\"img\" aria-label=\"Outcome of the run\">");
    let mut legend = Vec::new();
    for (class, name, n) in parts {
        if n == 0 {
            continue;
        }
        let pct = n as f64 / total as f64 * 100.0;
        bar.push_str(&format!("<i class=\"{class}\" style=\"width:{pct:.2}%\" title=\"{name}: {n}\"></i>"));
        legend.push(format!("{name} {n}"));
    }
    bar.push_str("</div>");
    format!("{bar}<p class=\"legend\">{}</p>", legend.join(" - "))
}

/// Milliseconds the way the timeline says them.
fn took(ms: u64) -> String {
    if ms >= 60_000 {
        format!("{}m {:02}s", ms / 60_000, (ms % 60_000) / 1000)
    } else {
        format!("{:.1} s", ms as f64 / 1000.0)
    }
}

/// The overview's short list of the biggest clusters.
pub(super) fn top_clusters(clusters: &[Cluster]) -> String {
    if clusters.is_empty() {
        return String::new();
    }
    let mut h = String::from("<h2>Top causes</h2><ul class=\"topc\">");
    for c in clusters.iter().take(5) {
        h.push_str(&format!(
            "<li><span class=\"count\">{}</span><a href=\"#cluster-{}\" data-goto=\"cluster-{}\">{}</a> <span class=\"cause\">{}</span></li>",
            c.cases.len(),
            c.number,
            c.number,
            c.label().to_uppercase(),
            esc_attr(&cluster_words(c)),
        ));
    }
    if clusters.len() > 5 {
        h.push_str(&format!("<li class=\"note\">and {} more in Failures</li>", clusters.len() - 5));
    }
    h.push_str("</ul>");
    h
}

/// A cluster in one line: where it stopped, then why.
fn cluster_words(c: &Cluster) -> String {
    if c.action.is_empty() {
        c.cause.clone()
    } else {
        format!("{} - {}", c.action, c.cause)
    }
}

// ----------------------------------------------------------------------
// Charts
// ----------------------------------------------------------------------

/// A small bar chart as inline SVG, one bar per value, the last one (this
/// run) drawn in the text colour. Each bar carries a `<title>` so hovering
/// says which run and how much. `max` fixes the top of the scale.
fn bars(values: &[(f64, String, bool)], max: f64, label: &str) -> String {
    let n = values.len().max(1);
    let (w, h, gap) = (14.0_f64, 60.0_f64, 4.0_f64);
    let width = n as f64 * w;
    let mut svg = format!(
        "<svg class=\"spark\" viewBox=\"0 0 {width} {h}\" preserveAspectRatio=\"none\" role=\"img\" aria-label=\"{}\">",
        esc_attr(label)
    );
    svg.push_str(&format!("<line class=\"axis\" x1=\"0\" y1=\"{h}\" x2=\"{width}\" y2=\"{h}\"/>"));
    let max = if max <= 0.0 { 1.0 } else { max };
    for (i, (value, title, current)) in values.iter().enumerate() {
        let bh = (value / max * (h - 2.0)).clamp(0.0, h - 2.0).max(if *value > 0.0 { 1.0 } else { 0.0 });
        let class = if *current { " class=\"cur\"" } else { "" };
        svg.push_str(&format!(
            "<rect{class} x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{bh:.1}\"><title>{}</title></rect>",
            i as f64 * w + gap / 2.0,
            h - bh,
            w - gap,
            esc_attr(title)
        ));
    }
    svg.push_str("</svg>");
    svg
}

fn when_of(p: &RunPoint) -> String {
    utc_time(&p.started_at)
}

/// The pass rate of each run, for the overview.
pub(super) fn sparkline(points: &[RunPoint]) -> String {
    if points.len() < 2 {
        return String::new();
    }
    let values: Vec<(f64, String, bool)> = points
        .iter()
        .map(|p| {
            let r = p.pass_rate.map_or("no result".to_string(), |r| format!("{r}% passed"));
            (f64::from(p.pass_rate.unwrap_or(0)), format!("{}: {r}", when_of(p)), p.current)
        })
        .collect();
    format!(
        "<h2>Recent runs</h2><div class=\"chart\" style=\"max-width:360px\"><h4>Pass rate, last {} runs</h4>{}</div>",
        points.len(),
        bars(&values, 100.0, "Pass rate of the last runs")
    )
}

fn ids(list: &[i32]) -> String {
    if list.is_empty() {
        return "none".to_string();
    }
    let mut links: Vec<String> = list
        .iter()
        .take(MAX_LISTED)
        .map(|id| format!("<a href=\"#case-{id}\" data-goto=\"case-{id}\">#{id}</a>"))
        .collect();
    if list.len() > MAX_LISTED {
        links.push(format!("and {} more", list.len() - MAX_LISTED));
    }
    links.join(", ")
}

fn compare_box(title: &str, list: &[i32]) -> String {
    format!("<div><h4>{title} ({})</h4><p>{}</p></div>", list.len(), ids(list))
}

/// The History view: charts per run, a table, and the comparison with the
/// run before.
pub(super) fn history_view(trend: &Trend, environment: Option<&str>) -> String {
    let mut h = String::from("<section class=\"view\" id=\"view-history\"><h2>History</h2>");
    let env = environment.map(str::trim).filter(|e| !e.is_empty()).map_or("this environment".to_string(), |e| format!("the {e} environment"));
    if trend.points.len() < 2 {
        h.push_str(&format!(
            "<p class=\"note\">This is the only run on this machine in {} so far, so there is no history to chart or compare yet.</p></section>",
            esc_attr(&env)
        ));
        return h;
    }
    h.push_str(&format!(
        "<p class=\"note\">The last {} runs in {}, oldest first (at most {HISTORY_RUNS}). A case counts as flaky when its script is marked flaky and it failed.</p>",
        trend.points.len(),
        esc_attr(&env)
    ));
    let rate: Vec<(f64, String, bool)> = trend
        .points
        .iter()
        .map(|p| (f64::from(p.pass_rate.unwrap_or(0)), format!("{}: {}", when_of(p), p.pass_rate.map_or("no result".to_string(), |r| format!("{r}%"))), p.current))
        .collect();
    let long: Vec<(f64, String, bool)> =
        trend.points.iter().map(|p| (p.duration_ms as f64, format!("{}: {}", when_of(p), took(p.duration_ms)), p.current)).collect();
    let max_ms = trend.points.iter().map(|p| p.duration_ms).max().unwrap_or(0) as f64;
    let flaky: Vec<(f64, String, bool)> =
        trend.points.iter().map(|p| (p.flaky as f64, format!("{}: {} flaky", when_of(p), p.flaky), p.current)).collect();
    let max_flaky = trend.points.iter().map(|p| p.flaky).max().unwrap_or(0).max(1) as f64;
    h.push_str("<div class=\"charts\">");
    h.push_str(&format!("<div class=\"chart\"><h4>Pass rate</h4>{}</div>", bars(&rate, 100.0, "Pass rate per run")));
    h.push_str(&format!("<div class=\"chart\"><h4>Duration</h4>{}</div>", bars(&long, max_ms, "Duration per run")));
    h.push_str(&format!("<div class=\"chart\"><h4>Flaky failures</h4>{}</div>", bars(&flaky, max_flaky, "Flaky failures per run")));
    h.push_str("</div>");

    h.push_str("<table class=\"runs\"><thead><tr><th>Ran</th><th>Total</th><th>Passed</th><th>Failed</th><th>Blocked</th><th>Not run</th><th>Flaky</th><th>Pass rate</th><th>Duration</th></tr></thead><tbody>");
    for p in trend.points.iter().rev() {
        let now = if p.current { " <span class=\"hint\">(this run)</span>" } else { "" };
        h.push_str(&format!(
            "<tr><td>{}{now}</td><td>{}</td><td class=\"b-passed\">{}</td><td class=\"b-failed\">{}</td><td class=\"b-blocked\">{}</td><td class=\"b-notrun\">{}</td><td class=\"b-flaky\">{}</td><td>{}</td><td>{}</td></tr>",
            esc_attr(&when_of(p)),
            p.total,
            p.passed,
            p.failed,
            p.blocked,
            p.not_run,
            p.flaky,
            p.pass_rate.map_or("-".to_string(), |r| format!("{r}%")),
            if p.duration_ms == 0 { "-".to_string() } else { took(p.duration_ms) },
        ));
    }
    h.push_str("</tbody></table>");

    if let Some(c) = &trend.compare {
        h.push_str(&compare_html(c));
    }
    h.push_str("</section>");
    h
}

fn compare_html(c: &Compare) -> String {
    let mut h = format!(
        "<h2>Compared with the run before it</h2><p class=\"note\">The previous run in this environment started {}.</p><div class=\"cmp\">",
        esc_attr(&utc_time(&c.previous_started_at))
    );
    h.push_str(&compare_box("New failures", &c.new_failures));
    h.push_str(&compare_box("Fixed", &c.fixed));
    h.push_str(&compare_box("Still failing", &c.still_failing));
    h.push_str(&compare_box("New cases", &c.new_cases));
    h.push_str(&format!(
        "<div><h4>Not in this run ({})</h4><p>{}</p></div>",
        c.missing.len(),
        if c.missing.is_empty() { "none".to_string() } else { c.missing.iter().take(MAX_LISTED).map(|i| format!("#{i}")).collect::<Vec<_>>().join(", ") }
    ));
    h.push_str("</div>");
    h
}

// ----------------------------------------------------------------------
// Per-case bits
// ----------------------------------------------------------------------

/// The last runs of a case as small boxes, oldest first.
pub(super) fn strip_html(t: &CaseTrend) -> String {
    if t.strip.is_empty() {
        return String::new();
    }
    let word = |c: &char| match c {
        'P' => "passed",
        'F' => "failed",
        'B' => "blocked",
        _ => "not run",
    };
    let words: Vec<&str> = t.strip.iter().map(word).collect();
    let boxes: String = t
        .strip
        .iter()
        .map(|c| format!("<i class=\"s-{}\"></i>", c.to_ascii_lowercase()))
        .collect();
    format!(
        "<span class=\"strip\" role=\"img\" aria-label=\"{0}\" title=\"{0}\">{boxes}</span>",
        esc_attr(&format!("Last {} runs, oldest first: {}", words.len(), words.join(", ")))
    )
}

/// The grade cell: the letter and, when it applies, the flaky marker.
pub(super) fn grade_cell(v: &CaseView) -> String {
    let letter = match v.grade() {
        Some(g) => format!("<span class=\"grade g-{}\" title=\"Stability over the last runs in this environment\">{g}</span>", g.to_ascii_lowercase()),
        None => "<span class=\"hint\" title=\"Too few runs to grade\">-</span>".to_string(),
    };
    let flk = match v.trend {
        Some(t) if t.marked => format!(" <span class=\"flk\" title=\"Marked as flaky\">flaky</span>"),
        Some(t) if t.alternates => format!(" <span class=\"flk\" title=\"Passed and failed in its last {FLAKY_WINDOW} runs\">flaky</span>"),
        _ => String::new(),
    };
    format!("{letter}{flk}")
}

/// A failed case's steps as bars, the failing step and the first anomaly
/// marked.
pub(super) fn timeline_html(t: &Timeline) -> String {
    if t.steps.is_empty() {
        return "<p class=\"note\">No steps were recorded for this case.</p>".to_string();
    }
    let longest = t.steps.iter().filter_map(|s| s.ms).max().unwrap_or(0).max(1);
    let mut h = String::from("<div class=\"tl\" role=\"list\">");
    for (i, s) in t.steps.iter().enumerate() {
        let mut class = String::from("tl-row");
        if s.failed {
            class.push_str(" failed");
        }
        if s.slow {
            class.push_str(" slow");
        }
        if t.anomaly == Some(i) {
            class.push_str(" anomaly");
        }
        let pct = s.ms.map_or(0.0, |ms| ms as f64 / longest as f64 * 100.0);
        let mut notes: Vec<String> = Vec::new();
        if s.failed {
            notes.push("failed".to_string());
        }
        if s.slow {
            notes.push(match s.median_ms {
                Some(m) => format!("slow, usually {}", took(m)),
                None => "slow".to_string(),
            });
        }
        if t.anomaly == Some(i) {
            notes.push("first anomaly".to_string());
        }
        h.push_str(&format!(
            "<div class=\"{class}\" role=\"listitem\"><span class=\"tl-l\">{}</span><span class=\"tl-bar\"><i style=\"width:{pct:.1}%\"></i></span><span class=\"tl-t\">{}</span><span class=\"tl-m\">{}</span></div>",
            esc_attr(&s.label),
            s.ms.map_or("-".to_string(), took),
            esc_attr(&notes.join(", ")),
        ));
    }
    h.push_str("</div>");
    h
}

// ----------------------------------------------------------------------
// Failures
// ----------------------------------------------------------------------

/// One failed case inside its cluster. `pictures` and `video` are only the
/// names the run kept (already checked); the buttons copy what the page's
/// data holds for `case_id`.
pub(super) fn failure_case(v: &CaseView, failure: &Failure, timeline: &Timeline) -> String {
    let id = v.case.case_id;
    let flaky = if v.flaky_failure { " <span class=\"flaky\">Flaky</span>" } else { "" };
    let place = if failure.step_number.is_some() {
        format!("<strong>{}</strong>: {}", esc_attr(&failure.step), esc_attr(&failure.action))
    } else {
        "No step failed on its own".to_string()
    };
    format!(
        "<div class=\"f-item fcase\"{attrs}><div class=\"fhead\"><span class=\"grow\"><a href=\"#case-{id}\" data-goto=\"case-{id}\"><span class=\"id\">#{id}</span> {title}</a> <span class=\"b-{key}\">{bucket}</span>{flaky}</span>\
<span class=\"btns\"><button type=\"button\" class=\"btn\" data-copy=\"summary\" data-id=\"{id}\">Copy failure summary</button>\
<button type=\"button\" class=\"btn\" data-copy=\"prompt\" data-id=\"{id}\">Copy AI prompt</button></span></div>\
<p class=\"fmsg\">{place}<br><span class=\"cause\">{message}</span></p>{timeline}</div>",
        attrs = v.attrs(),
        title = esc_attr(&v.case.title),
        key = css_key(v.bucket),
        bucket = v.bucket,
        message = esc_attr(&failure.message),
        timeline = timeline_html(timeline),
    )
}

/// One cluster's box around its cases.
pub(super) fn cluster_box(c: &Cluster, cases: &str) -> String {
    let n = c.cases.len();
    format!(
        "<section class=\"cluster\" id=\"cluster-{}\"><h3><span class=\"count\">{n}</span>{} <span class=\"hint\">{} case{}</span></h3><p class=\"cause\">{}</p>{cases}</section>",
        c.number,
        c.label().to_uppercase(),
        n,
        if n == 1 { "" } else { "s" },
        esc_attr(&cluster_words(c)),
    )
}

// ----------------------------------------------------------------------
// Gallery, dialogs, data
// ----------------------------------------------------------------------

/// The gallery's shell: its grid and lightbox are built by the page's
/// script from the data blob.
pub(super) fn gallery_view(count: usize) -> String {
    let body = if count == 0 {
        "<p class=\"note\">No pictures or videos were kept for this run.</p>".to_string()
    } else {
        format!(
            "<p class=\"note\">{count} picture{} and video{} from this run. Click one to open it; the arrow keys move between them.</p>\
<div class=\"gal\" id=\"gal-grid\"></div><p><button type=\"button\" class=\"btn\" id=\"gal-more\" hidden>Show more</button></p>",
            if count == 1 { "" } else { "s" },
            if count == 1 { "" } else { "s" },
        )
    };
    format!("<section class=\"view\" id=\"view-gallery\"><h2>Gallery</h2>{body}</section>")
}

/// The keyboard list and the lightbox, both closed.
pub(super) fn dialogs() -> &'static str {
    "<div class=\"dlg\" id=\"help\" role=\"dialog\" aria-modal=\"true\" aria-label=\"Keyboard shortcuts\" hidden><div class=\"box\">\
<h3>Keyboard</h3><dl>\
<dt><kbd>j</kbd> <kbd>k</kbd></dt><dd>Next and previous case</dd>\
<dt><kbd>Enter</kbd></dt><dd>Open or close the case</dd>\
<dt><kbd>/</kbd></dt><dd>Go to the filter</dd>\
<dt><kbd>?</kbd></dt><dd>Show or hide this list</dd>\
<dt><kbd>Esc</kbd></dt><dd>Close a window, clear the filter</dd>\
<dt><kbd>\u{2190}</kbd> <kbd>\u{2192}</kbd></dt><dd>Previous and next picture in the gallery</dd></dl>\
<p class=\"hint\">The filter takes words and <code>status:</code> (passed, failed, blocked, notrun, flaky), <code>area:</code>, <code>module:</code>, <code>grade:</code> and <code>cause:</code> (a cluster such as c1, or words from its cause). Put quotes around a value with spaces.</p></div></div>\
<div class=\"dlg lb\" id=\"lb\" role=\"dialog\" aria-modal=\"true\" aria-label=\"Picture\" hidden>\
<div class=\"stage\"></div><div class=\"bar\"><button type=\"button\" class=\"btn lb-prev\">Previous</button><span class=\"lb-cap\"></span>\
<button type=\"button\" class=\"btn lb-next\">Next</button><button type=\"button\" class=\"btn lb-x\">Close</button></div></div>"
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
