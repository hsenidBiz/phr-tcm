//! Report diffing — "what changed" between two report byte-sets, as `JSON-path: a -> b`
//! lines over the decoded models. The same comparison `rpt-tools diff` prints, exposed for
//! a front end's changes panel / version compare.
//!
//! The diff is over STORED facts only (the decoded semantic model, including the subreport
//! tree), not derived analytics — two files that decode identically diff clean, which is
//! the property a version-compare surface needs.

use rpt_reader::Rpt;

/// Diff two `.rpt` byte-sets; the lines are `$.path.to.field: <old> -> <new>` (and
/// `: added` / `: removed` for map keys, `: length a -> b` for arrays), or empty when the
/// decoded models are identical.
///
/// # Errors
///
/// Either side failing to decode as a report.
pub fn diff_bytes(a: &[u8], b: &[u8]) -> Result<Vec<String>, String> {
    diff_reports(&crate::open_bytes(a)?, &crate::open_bytes(b)?)
}

/// [`diff_bytes`] over already-decoded reports.
pub fn diff_reports(a: &Rpt, b: &Rpt) -> Result<Vec<String>, String> {
    let va = serde_json::to_value(a.report()).map_err(|e| e.to_string())?;
    let vb = serde_json::to_value(b.report()).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    diff_values(&va, &vb, "$", &mut out);
    Ok(out)
}

fn diff_values(a: &serde_json::Value, b: &serde_json::Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (serde_json::Value::Object(ma), serde_json::Value::Object(mb)) => {
            for (k, va) in ma {
                let p = format!("{path}.{k}");
                match mb.get(k) {
                    Some(vb) => diff_values(va, vb, &p, out),
                    None => out.push(format!("{p}: removed")),
                }
            }
            for k in mb.keys() {
                if !ma.contains_key(k) {
                    out.push(format!("{path}.{k}: added"));
                }
            }
        }
        (serde_json::Value::Array(va), serde_json::Value::Array(vb)) => {
            if va.len() != vb.len() {
                out.push(format!("{path}: length {} -> {}", va.len(), vb.len()));
            }
            for (i, (xa, xb)) in va.iter().zip(vb.iter()).enumerate() {
                diff_values(xa, xb, &format!("{path}[{i}]"), out);
            }
        }
        _ => {
            if a != b {
                out.push(format!("{path}: {} -> {}", brief(a), brief(b)));
            }
        }
    }
}

fn brief(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() > 90 {
        format!("{}…", &s[..88])
    } else {
        s
    }
}
