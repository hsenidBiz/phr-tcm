//! A semantic difference between two decoded reports.
//!
//! The bundle has no library function for this: its command-line tool
//! serializes each report's decoded model and walks the two JSON trees.
//! `diff_values` and `brief` below are a port of that walk
//! (crystal-reports/rpt-cli/src/main.rs, `diff_values` and `brief`), kept
//! line for line so a `path: a -> b` line here reads the same as the
//! tool's.

use serde_json::Value;

/// Every difference between `a` and `b`, one `path: a -> b` line each
/// (`path: removed`, `path: added` and `path: length n -> m` for keys and
/// lists). Empty when the two match.
pub fn diff(a: &Value, b: &Value) -> Vec<String> {
    let mut out = Vec::new();
    diff_values(a, b, "$", &mut out);
    out
}

fn diff_values(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
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
        (Value::Array(va), Value::Array(vb)) => {
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

/// A value as JSON, cut to about 90 characters.
fn brief(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 90 {
        // The tool cuts at byte 88, which panics inside a multi-byte
        // character; this cuts at the last character boundary before it.
        let mut end = 88;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    } else {
        s
    }
}
