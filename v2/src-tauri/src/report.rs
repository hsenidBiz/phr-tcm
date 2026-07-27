//! Execution report HTML (spec: Execution Depth & Trust, C). Pure builder
//! over already-fetched points + failure details so it is fully testable;
//! the command in lib.rs does the fetching and the temp-file/open part.

use crate::ado_testplan::TestPoint;
use std::collections::HashMap;

/// Comment + linked bug ids for a failed point's last result.
#[derive(Debug, Clone, Default)]
pub struct FailureInfo {
    pub comment: String,
    pub bug_ids: Vec<i32>,
}

/// UTC timestamp from epoch seconds without a chrono dependency
/// (Howard Hinnant's civil-from-days algorithm; unit-tested).
pub fn format_epoch_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    let (hh, mm) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} UTC")
}

fn esc(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn outcome_rank(outcome: &str) -> u8 {
    match outcome.to_lowercase().as_str() {
        "failed" => 0,
        "blocked" => 1,
        "notapplicable" => 2,
        "passed" => 3,
        _ => 4, // never run last
    }
}

fn outcome_label(outcome: &str) -> String {
    match outcome.to_lowercase().as_str() {
        "notapplicable" => "Not Applicable".into(),
        "" => "Never run".into(),
        o => {
            let mut c = o.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        }
    }
}

/// The app's palette, handed over when a report is opened so the page in
/// the browser matches the app the user just came from.
///
/// The values are read live from the running UI's CSS variables rather
/// than duplicated here, so a new theme (or an accent preset composed on
/// top of one) needs no change in Rust. Every field falls back to the
/// original light styling if it arrives empty, which is what happens for
/// any caller that doesn't supply a palette.
#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct ReportPalette {
    pub bg: String,
    pub surface: String,
    pub surface_2: String,
    pub text: String,
    pub muted: String,
    pub faint: String,
    pub border: String,
    pub accent: String,
    pub success: String,
    pub danger: String,
    pub warning: String,
    /// Drives `color-scheme`, so form controls and scrollbars follow too.
    pub dark: bool,
}

impl Default for ReportPalette {
    /// The report's original light styling.
    fn default() -> Self {
        Self {
            bg: "#f3f5f8".into(),
            surface: "#ffffff".into(),
            surface_2: "#eef1f5".into(),
            text: "#1f2530".into(),
            muted: "#5b6472".into(),
            faint: "#8a93a1".into(),
            border: "#e8ecf1".into(),
            accent: "#15803d".into(),
            success: "#16a34a".into(),
            danger: "#dc2626".into(),
            warning: "#d97706".into(),
            dark: false,
        }
    }
}

impl ReportPalette {
    /// Replace any empty field with the light default: a half-populated
    /// palette must never render unreadable text on an unstyled page.
    fn filled(&self) -> Self {
        let d = Self::default();
        let or = |v: &str, fallback: &str| {
            let v = v.trim();
            if v.is_empty() { fallback.to_string() } else { v.to_string() }
        };
        Self {
            bg: or(&self.bg, &d.bg),
            surface: or(&self.surface, &d.surface),
            surface_2: or(&self.surface_2, &d.surface_2),
            text: or(&self.text, &d.text),
            muted: or(&self.muted, &d.muted),
            faint: or(&self.faint, &d.faint),
            border: or(&self.border, &d.border),
            accent: or(&self.accent, &d.accent),
            success: or(&self.success, &d.success),
            danger: or(&self.danger, &d.danger),
            warning: or(&self.warning, &d.warning),
            dark: self.dark,
        }
    }

    fn vars(&self) -> String {
        let p = self.filled();
        format!(
            ":root {{ color-scheme: {scheme};\n\
             --bg: {bg}; --surface: {surface}; --surface-2: {surface_2};\n\
             --text: {text}; --muted: {muted}; --faint: {faint}; --border: {border};\n\
             --accent: {accent}; --success: {success}; --danger: {danger}; --warning: {warning}; }}",
            scheme = if p.dark { "dark" } else { "light" },
            bg = p.bg,
            surface = p.surface,
            surface_2 = p.surface_2,
            text = p.text,
            muted = p.muted,
            faint = p.faint,
            border = p.border,
            accent = p.accent,
            success = p.success,
            danger = p.danger,
            warning = p.warning,
        )
    }
}

/// Everything here is expressed against the variables above, so the same
/// stylesheet serves every theme. Cards carry a real border as well as a
/// shadow - a drop shadow is invisible on a black background, and the
/// table would otherwise dissolve into the page on the OLED theme.
const CSS: &str = r#"
* { box-sizing: border-box; }
body { font-family: 'Segoe UI', system-ui, sans-serif; margin: 0; padding: 32px 16px;
       background: var(--bg); color: var(--text); }
.page { max-width: 900px; margin: 0 auto; }
h1 { font-size: 22px; margin: 0 0 4px; }
.sub { color: var(--muted); font-size: 13px; margin-bottom: 20px; }
.headline { display: flex; gap: 24px; align-items: baseline; margin-bottom: 12px; }
.rate { font-size: 40px; font-weight: 700; }
.bar { display: flex; height: 10px; border-radius: 6px; overflow: hidden; margin: 8px 0 4px;
       background: var(--surface-2); }
.bar span { display: block; height: 100%; }
.legend { font-size: 12px; color: var(--muted); margin-bottom: 24px; }
.passed { background: var(--success); } .failed { background: var(--danger); }
.blocked { background: var(--warning); } .notapplicable { background: var(--faint); }
.neverrun { background: var(--border); }
table { width: 100%; border-collapse: collapse; background: var(--surface); border-radius: 8px;
        overflow: hidden; border: 1px solid var(--border); font-size: 14px; }
th, td { text-align: left; padding: 8px 12px; border-bottom: 1px solid var(--border); }
tr:last-child td { border-bottom: none; }
th { background: var(--surface-2); font-size: 12px; text-transform: uppercase;
     letter-spacing: .03em; color: var(--muted); }
.o-failed { color: var(--danger); font-weight: 600; } .o-passed { color: var(--success); }
.o-blocked { color: var(--warning); } .o-notapplicable, .o-neverrun { color: var(--faint); }
h2 { font-size: 16px; margin: 28px 0 8px; }
.fail { background: var(--surface); border: 1px solid var(--border);
        border-left: 4px solid var(--danger); border-radius: 6px;
        padding: 10px 14px; margin-bottom: 8px; }
.fail .name { font-weight: 600; }
.fail .comment { color: var(--muted); font-size: 13px; margin-top: 4px; white-space: pre-wrap; }
.fail .bugs a { color: var(--danger); font-size: 13px; margin-right: 8px; }
.mono { font-family: Consolas, monospace; color: var(--faint); font-size: 12px; }
.footer { color: var(--faint); font-size: 12px; margin-top: 24px; }
a { color: var(--accent); }
"#;

/// Build the self-contained report. `failures` is keyed by point_id.
/// `generated_at` is injected so the builder stays deterministic in tests.
pub fn build_report_html(
    title: &str,
    org: &str,
    project: &str,
    points: &[TestPoint],
    failures: &HashMap<i32, FailureInfo>,
    generated_at: &str,
    palette: &ReportPalette,
) -> String {
    let total = points.len();
    let mut counts: HashMap<String, usize> = HashMap::new();
    for p in points {
        let key = if p.last_outcome.is_empty() {
            "neverrun".to_string()
        } else {
            p.last_outcome.to_lowercase()
        };
        *counts.entry(key).or_default() += 1;
    }
    let get = |k: &str| counts.get(k).copied().unwrap_or(0);
    let executed = total - get("neverrun");
    let pass_rate = if executed > 0 {
        (get("passed") as f64 / executed as f64 * 100.0).round() as i64
    } else {
        0
    };

    let mut sorted: Vec<&TestPoint> = points.iter().collect();
    sorted.sort_by(|a, b| {
        outcome_rank(&a.last_outcome)
            .cmp(&outcome_rank(&b.last_outcome))
            .then_with(|| a.test_case_name.to_lowercase().cmp(&b.test_case_name.to_lowercase()))
    });

    let mut bar = String::new();
    let mut legend: Vec<String> = vec![];
    for (key, label) in [
        ("passed", "Passed"),
        ("failed", "Failed"),
        ("blocked", "Blocked"),
        ("notapplicable", "Not Applicable"),
        ("neverrun", "Never run"),
    ] {
        let n = get(key);
        if n == 0 || total == 0 {
            continue;
        }
        let pct = n as f64 / total as f64 * 100.0;
        bar.push_str(&format!(r#"<span class="{key}" style="width:{pct:.1}%"></span>"#));
        legend.push(format!("{label}: {n}"));
    }

    let mut rows = String::new();
    for p in &sorted {
        let key = if p.last_outcome.is_empty() {
            "neverrun".to_string()
        } else {
            p.last_outcome.to_lowercase()
        };
        rows.push_str(&format!(
            r#"<tr><td class="mono">#{}</td><td>{}</td><td class="o-{}">{}</td></tr>"#,
            p.test_case_id.map(|i| i.to_string()).unwrap_or_default(),
            esc(&p.test_case_name),
            key,
            outcome_label(&p.last_outcome)
        ));
    }

    let mut fail_html = String::new();
    for p in &sorted {
        if !p.last_outcome.eq_ignore_ascii_case("failed") {
            continue;
        }
        let info = failures.get(&p.point_id).cloned().unwrap_or_default();
        let bugs = if info.bug_ids.is_empty() {
            String::new()
        } else {
            let links: Vec<String> = info
                .bug_ids
                .iter()
                .map(|id| {
                    format!(
                        r#"<a href="https://dev.azure.com/{org}/{}/_workitems/edit/{id}">Bug #{id}</a>"#,
                        urlencoding::encode(project)
                    )
                })
                .collect();
            format!(r#"<div class="bugs">{}</div>"#, links.join(""))
        };
        let comment = if info.comment.is_empty() {
            String::new()
        } else {
            format!(r#"<div class="comment">{}</div>"#, esc(&info.comment))
        };
        fail_html.push_str(&format!(
            r#"<div class="fail"><span class="mono">#{}</span> <span class="name">{}</span>{comment}{bugs}</div>"#,
            p.test_case_id.map(|i| i.to_string()).unwrap_or_default(),
            esc(&p.test_case_name)
        ));
    }
    let failures_section = if fail_html.is_empty() {
        String::new()
    } else {
        format!("<h2>Failures</h2>{fail_html}")
    };

    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{t}</title><style>{vars}{CSS}</style></head>
<body><div class="page">
<h1>Execution report — {t}</h1>
<div class="sub">{org} / {proj}</div>
<div class="headline"><span class="rate">{pass_rate}%</span><span>pass rate over {executed} executed of {total} cases</span></div>
<div class="bar">{bar}</div>
<div class="legend">{legend}</div>
<table><thead><tr><th>Id</th><th>Test case</th><th>Last outcome</th></tr></thead><tbody>{rows}</tbody></table>
{failures_section}
<div class="footer">Generated {generated_at} by Test Case Manager</div>
</div></body></html>"#,
        vars = palette.vars(),
        t = esc(title),
        org = esc(org),
        proj = esc(project),
        legend = legend.join(" · "),
    )
}
