//! Seeing the page: a text rendering of Chrome's own accessibility tree,
//! and a probe that says what a locator matches right now.
//!
//! An assistant repairing a script cannot open a browser window and look;
//! this is what it gets instead. Chrome has already computed role, name
//! and the folding of purely structural nodes (`generic`, `StaticText`...)
//! into their parents - this module reads that tree and turns it into
//! text with a locator on every line, rather than re-deriving any of it.

use super::cdp::{CdpError, Driver};
use super::locator::{resolve, Target, VISIBLE_JS};
use super::page;
use serde_json::{json, Value};
use std::collections::HashMap;

/// The line count a snapshot stops at unless a caller asks for more.
pub const DEFAULT_LIMIT: usize = 300;

/// Roles Chrome uses purely for structure or for text already folded into
/// a parent's name. Printing them would double up on what the parent line
/// already says, so they are skipped - their children print at the
/// skipped node's own depth, as if it were never there.
const FOLDED_ROLES: [&str; 5] = ["generic", "none", "presentation", "InlineTextBox", "StaticText"];

/// Roles whose current value is worth showing on the line.
const VALUE_ROLES: [&str; 3] = ["textbox", "combobox", "searchbox"];

/// One node of the accessibility tree, flattened out of Chrome's own
/// `nodeId`/`childIds` shape. `value` is already `None` for anything this
/// module has decided is password-like - see `parse_nodes`.
#[derive(Debug, Clone, PartialEq)]
pub struct AxNode {
    pub id: String,
    pub role: String,
    pub name: String,
    pub value: Option<String>,
    pub ignored: bool,
    pub children: Vec<String>,
    pub focusable: bool,
    pub disabled: bool,
}

/// Is a boolean AX property, by name, true on this raw node?
fn property_bool(raw: &Value, name: &str) -> bool {
    raw["properties"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["name"].as_str() == Some(name))
        .and_then(|p| p["value"]["value"].as_bool())
        .unwrap_or(false)
}

/// Turn `Accessibility.getFullAXTree`'s answer (the whole result object,
/// with its `nodes` array) into the flat list `render` walks. A field
/// Chrome omitted reads as empty/false, never as a parse failure - a
/// missing `name` or `value` is routine, not something to error out over.
pub fn parse_nodes(v: &Value) -> Vec<AxNode> {
    v["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|raw| {
            let role = raw["role"]["value"].as_str().unwrap_or("").to_string();
            let name = raw["name"]["value"].as_str().unwrap_or("").to_string();
            let children = raw["childIds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|c| c.as_str().map(str::to_string))
                .collect();
            // Two independent signals, because either one alone misses
            // real cases: a site that never sets the `protected` property
            // but names its field "Password", and a `protected` field
            // this app has no other way to recognise.
            let protected = (role.eq_ignore_ascii_case("textbox") && property_bool(raw, "protected"))
                || (role.eq_ignore_ascii_case("textbox") && name.to_lowercase().contains("password"));
            let value = if protected {
                None
            } else {
                raw["value"]["value"].as_str().map(str::to_string)
            };
            AxNode {
                id: raw["nodeId"].as_str().unwrap_or("").to_string(),
                role,
                name,
                value,
                ignored: raw["ignored"].as_bool().unwrap_or(false),
                children,
                focusable: property_bool(raw, "focusable"),
                disabled: property_bool(raw, "disabled"),
            }
        })
        .collect()
}

/// 80 characters, with `...` (three ASCII dots, never the single `…`
/// character) standing in for whatever was cut.
fn truncate_name(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= 80 {
        name.to_string()
    } else {
        let mut s: String = chars[..80].iter().collect();
        s.push_str("...");
        s
    }
}

/// The locator that reaches this line, in the same shape a script's
/// `target` takes: role alone when there is no name to narrow with.
fn locator_suffix(role: &str, name: &str) -> String {
    if name.is_empty() {
        format!(" -> {{ \"role\": \"{role}\" }}")
    } else {
        format!(" -> {{ \"role\": \"{role}\", \"name\": \"{name}\" }}")
    }
}

fn format_line(node: &AxNode, depth: usize) -> String {
    let indent = " ".repeat(depth.min(12));
    let name = truncate_name(&node.name);
    let mut line = format!("{indent}{} \"{name}\"", node.role);
    // A password never leaves this module: the name check runs here too,
    // not only in `parse_nodes`, so a hand-built node reaches the same
    // outcome as one this module actually read off a real page.
    if VALUE_ROLES.contains(&node.role.as_str()) {
        if let Some(value) = &node.value {
            if !node.name.to_lowercase().contains("password") {
                line.push_str(&format!(" = \"{value}\""));
            }
        }
    }
    if node.disabled {
        line.push_str(" (disabled)");
    }
    line.push_str(&locator_suffix(&node.role, &name));
    line
}

/// Depth-first, skipping folded nodes but still visiting their children -
/// at the folded node's own depth, since it never printed a line to
/// indent under.
fn walk(id: &str, depth: usize, by_id: &HashMap<&str, &AxNode>, out: &mut Vec<String>) {
    let Some(node) = by_id.get(id) else { return };
    let folded = node.ignored || FOLDED_ROLES.contains(&node.role.as_str());
    if folded {
        for child in &node.children {
            walk(child, depth, by_id, out);
        }
        return;
    }
    out.push(format_line(node, depth));
    for child in &node.children {
        walk(child, depth + 1, by_id, out);
    }
}

/// Pure rendering: every rule (folding, password redaction, the locator
/// suffix, the line cap, the depth cap, the empty-tree sentence) lives
/// here so it can be tested against hand-built nodes with no browser at
/// all. The root is `nodes[0]`, matching what `getFullAXTree` returns.
pub fn render(nodes: &[AxNode], limit: usize) -> String {
    let Some(root) = nodes.first() else {
        return "the page has nothing a locator could name".to_string();
    };
    let by_id: HashMap<&str, &AxNode> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut lines = Vec::new();
    walk(&root.id, 0, &by_id, &mut lines);
    if lines.is_empty() {
        return "the page has nothing a locator could name".to_string();
    }
    let total = lines.len();
    let mut out = lines.iter().take(limit).cloned().collect::<Vec<_>>().join("\n");
    if total > limit {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("... and {} more (raise the limit, or scope the probe)", total - limit));
    }
    out
}

/// `Accessibility.getFullAXTree`, retried once after `Accessibility.enable`
/// if the domain was never switched on - real Edge answers the first call
/// that way rather than enabling it implicitly.
pub async fn snapshot<D: Driver>(d: &mut D, limit: usize) -> Result<String, CdpError> {
    let result = match d.call("Accessibility.getFullAXTree", json!({})).await {
        Ok(v) => v,
        Err(CdpError::Protocol { message, .. }) if message.to_lowercase().contains("enabled") => {
            d.call("Accessibility.enable", json!({})).await?;
            d.call("Accessibility.getFullAXTree", json!({})).await?
        }
        Err(e) => return Err(e),
    };
    Ok(render(&parse_nodes(&result), limit))
}

/// `this` is the element. One call for everything `probe` prints besides
/// visibility, which reuses `VISIBLE_JS` - the same check every other
/// action already trusts, rather than a second copy of it here.
pub const PROBE_SUMMARY_JS: &str = r#"function() {
  const r = this.getBoundingClientRect();
  const text = (this.innerText || this.value || '').trim();
  return { tag: this.tagName.toLowerCase(), text, rect: [r.x, r.y, r.width, r.height] };
}"#;

fn trim_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// What a locator matches right now, for a person or an assistant deciding
/// whether it is safe to use. Empty is reported, not treated as an error:
/// a target that matches nothing is exactly what a caller needs to know.
pub async fn probe<D: Driver>(d: &mut D, target: &Target) -> Result<String, CdpError> {
    let handles = resolve(d, target).await?;
    if handles.is_empty() {
        return Ok(format!("matches: 0 - nothing on the page answers to {}", target.describe()));
    }
    let mut lines = vec![format!("matches: {}", handles.len())];
    for handle in handles.iter().take(10) {
        let visible = page::call_value(d, handle, VISIBLE_JS, &[]).await?.as_bool().unwrap_or(false);
        let summary = page::call_value(d, handle, PROBE_SUMMARY_JS, &[]).await?;
        let tag = summary["tag"].as_str().unwrap_or("");
        let text = trim_chars(summary["text"].as_str().unwrap_or(""), 60);
        let rect = |i: usize| summary["rect"][i].as_f64().unwrap_or(0.0).round() as i64;
        lines.push(format!(
            "{tag} \"{text}\" {} at {},{} {}x{}",
            if visible { "visible" } else { "hidden" },
            rect(0),
            rect(1),
            rect(2),
            rect(3),
        ));
    }
    if handles.len() > 1 {
        lines.push("narrow the locator (add \"name\", \"exact\": true, a scope, or \"nth\")".to_string());
    }
    Ok(lines.join("\n"))
}
