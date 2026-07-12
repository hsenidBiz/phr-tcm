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

const CSS: &str = r#"
:root { color-scheme: light; }
* { box-sizing: border-box; }
body { font-family: 'Segoe UI', system-ui, sans-serif; margin: 0; padding: 32px 16px;
       background: #f3f5f8; color: #1f2530; }
.page { max-width: 900px; margin: 0 auto; }
h1 { font-size: 22px; margin: 0 0 4px; }
.sub { color: #5b6472; font-size: 13px; margin-bottom: 20px; }
.headline { display: flex; gap: 24px; align-items: baseline; margin-bottom: 12px; }
.rate { font-size: 40px; font-weight: 700; }
.bar { display: flex; height: 10px; border-radius: 6px; overflow: hidden; margin: 8px 0 4px; }
.bar span { display: block; height: 100%; }
.legend { font-size: 12px; color: #5b6472; margin-bottom: 24px; }
.passed { background: #16a34a; } .failed { background: #dc2626; }
.blocked { background: #d97706; } .notapplicable { background: #94a3b8; }
.neverrun { background: #e2e8f0; }
table { width: 100%; border-collapse: collapse; background: #fff; border-radius: 8px;
        overflow: hidden; box-shadow: 0 1px 3px rgb(15 23 42 / .08); font-size: 14px; }
th, td { text-align: left; padding: 8px 12px; border-bottom: 1px solid #e8ecf1; }
th { background: #eef1f5; font-size: 12px; text-transform: uppercase; letter-spacing: .03em; }
.o-failed { color: #dc2626; font-weight: 600; } .o-passed { color: #16a34a; }
.o-blocked { color: #d97706; } .o-notapplicable, .o-neverrun { color: #94a3b8; }
h2 { font-size: 16px; margin: 28px 0 8px; }
.fail { background: #fff; border-left: 4px solid #dc2626; border-radius: 6px;
        padding: 10px 14px; margin-bottom: 8px; box-shadow: 0 1px 3px rgb(15 23 42 / .08); }
.fail .name { font-weight: 600; }
.fail .comment { color: #414b5a; font-size: 13px; margin-top: 4px; white-space: pre-wrap; }
.fail .bugs a { color: #b91c1c; font-size: 13px; margin-right: 8px; }
.mono { font-family: Consolas, monospace; color: #7c8698; font-size: 12px; }
.footer { color: #8a93a1; font-size: 12px; margin-top: 24px; }
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
        r#"<!doctype html><html><head><meta charset="utf-8"><title>{t}</title><style>{CSS}</style></head>
<body><div class="page">
<h1>Execution report — {t}</h1>
<div class="sub">{org} / {proj}</div>
<div class="headline"><span class="rate">{pass_rate}%</span><span>pass rate over {executed} executed of {total} cases</span></div>
<div class="bar">{bar}</div>
<div class="legend">{legend}</div>
<table><thead><tr><th>Id</th><th>Test case</th><th>Last outcome</th></tr></thead><tbody>{rows}</tbody></table>
{failures_section}
<div class="footer">Generated {generated_at} by Test Case Manager V2</div>
</div></body></html>"#,
        t = esc(title),
        org = esc(org),
        proj = esc(project),
        legend = legend.join(" · "),
    )
}
