//! Seeing the page: a text rendering of Chrome's own accessibility tree,
//! and a probe that says what a locator matches right now.
//!
//! An assistant repairing a script cannot open a browser window and look;
//! this is what it gets instead. Chrome has already computed role, name
//! and the folding of purely structural nodes (`generic`, `StaticText`...)
//! into their parents - this module reads that tree and turns it into
//! text with a locator on every line, rather than re-deriving any of it.

use super::cdp::{CdpError, Driver};
use super::locator::{resolve_explained, Target, FRAME_JS, VISIBLE_JS};
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
    /// The DOM node behind it (`backendDOMNodeId`), when Chrome gave one -
    /// how an `Iframe` line finds the frame whose tree prints under it.
    pub backend: Option<i64>,
    /// The AX `required` property: a field the page will not accept empty.
    pub required: bool,
}

/// One printed line as data: what a caller needs to pick a control without
/// parsing the text. `locator` is the value printed after `->` on the line,
/// frame steps included, so the text and the data never disagree.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapLine {
    pub role: String,
    pub name: String,
    pub locator: Target,
    pub required: bool,
}

/// One iframe's own accessibility tree, printed under the iframe's line.
/// `iframe_id` is that `Iframe` node's id in the tree it sits in; `step` is
/// the locator step that reaches the iframe, put in front of every locator
/// printed inside it. Frames inside this frame are in `frames`, keyed by ids
/// in THIS tree - each frame numbers its nodes on its own, so ids from
/// different frames must never share one lookup.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameTree {
    pub iframe_id: String,
    pub step: Value,
    pub nodes: Vec<AxNode>,
    /// Why the frame's tree could not be read; one line says so instead.
    pub unreadable: Option<String>,
    /// The page itself said it cannot reach the frame's document (another
    /// site, or sandboxed): nothing in it can ever be recorded or scripted,
    /// so a read that could not show it still shows the whole page. Any
    /// other unreadable frame leaves the read partial.
    pub cross_origin: bool,
    pub frames: Vec<FrameTree>,
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
                backend: raw["backendDOMNodeId"].as_i64(),
                required: property_bool(raw, "required"),
            }
        })
        .collect()
}

/// Every control character (`\n`, `\r`, `\t`, and anything else in that
/// category) becomes a space. Run before truncation, on every piece of
/// text this module prints - a stray newline or tab in an accessible name
/// or a probe's read-back text would otherwise break the one-line format
/// this whole module promises.
fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
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
/// `target` takes: role alone when there is no name to narrow with. Built
/// through `serde_json` rather than hand-formatted, so a name carrying a
/// `"` or a `\` still comes out as valid JSON a `Target` can be read back
/// from.
/// The line's own locator, behind the steps of any frames it sits in.
fn locator_value(role: &str, name: &str, frames: &[Value]) -> Value {
    let obj = if name.is_empty() { json!({ "role": role }) } else { json!({ "role": role, "name": name }) };
    if frames.is_empty() {
        return obj;
    }
    let mut chain = frames.to_vec();
    chain.push(obj);
    Value::Array(chain)
}

fn locator_suffix(role: &str, name: &str, frames: &[Value]) -> String {
    format!(" -> {}", locator_value(role, name, frames))
}

/// The data behind a printed line: the same locator value the suffix prints.
fn snap_line(node: &AxNode, frames: &[Value]) -> SnapLine {
    let name = sanitize(&node.name);
    let value = locator_value(&node.role, &name, frames);
    let locator = serde_json::from_value::<Target>(value.clone()).unwrap_or_else(|_| Target::Legacy(value.to_string()));
    SnapLine { role: node.role.clone(), name, locator, required: node.required }
}

/// A printed line and, when it carries a locator, its data.
type Printed = (String, Option<SnapLine>);

fn format_line(node: &AxNode, depth: usize, frames: &[Value]) -> String {
    let indent = " ".repeat(depth.min(12));
    // The printed name is capped for readability, but the locator on the
    // end of the line has to reach the element by its REAL name - a
    // locator built from the truncated prefix would only ever match a
    // control whose accessible name happens to start with those same 80
    // characters, which is not what this line is actually called.
    let full_name = sanitize(&node.name);
    let name = truncate_name(&full_name);
    let mut line = format!("{indent}{} \"{name}\"", node.role);
    // A password never leaves this module: the name check runs here too,
    // not only in `parse_nodes`, so a hand-built node reaches the same
    // outcome as one this module actually read off a real page. A blank
    // name is refused outright - a value with nothing to label it is not
    // something a locator could ever ask for by name.
    if VALUE_ROLES.contains(&node.role.as_str()) && !name.is_empty() {
        if let Some(value) = &node.value {
            if !name.to_lowercase().contains("password") {
                line.push_str(&format!(" = \"{}\"", sanitize(value)));
            }
        }
    }
    if node.disabled {
        line.push_str(" (disabled)");
    }
    line.push_str(&locator_suffix(&node.role, &full_name, frames));
    line
}

/// One tree being walked: its nodes by id, the frames inside it by their
/// iframe's id, and the steps of the frames it itself sits in.
struct Tree<'a> {
    by_id: HashMap<&'a str, &'a AxNode>,
    frames: HashMap<&'a str, &'a FrameTree>,
    path: Vec<Value>,
}

/// Depth-first, skipping folded nodes but still visiting their children -
/// at the folded node's own depth, since it never printed a line to
/// indent under. `seen` guards against a `childIds` cycle (or the same id
/// reachable two ways): a node already visited anywhere in this walk is
/// skipped rather than recursed into again, which would otherwise recurse
/// forever on a malformed tree. `missed` is set when a frame's contents
/// were not shown (a frame that could not be read, other than one from
/// another site, or one nested past `MAX_FRAME_DEPTH`): a read with such a
/// frame does not show the whole page.
fn walk(
    id: &str,
    depth: usize,
    tree: &Tree<'_>,
    out: &mut Vec<Printed>,
    seen: &mut std::collections::HashSet<String>,
    missed: &mut bool,
) {
    if !seen.insert(id.to_string()) {
        return;
    }
    let Some(node) = tree.by_id.get(id) else { return };
    let folded = node.ignored || FOLDED_ROLES.contains(&node.role.as_str());
    if folded {
        for child in &node.children {
            walk(child, depth, tree, out, seen, missed);
        }
        return;
    }
    out.push((format_line(node, depth, &tree.path), Some(snap_line(node, &tree.path))));
    for child in &node.children {
        walk(child, depth + 1, tree, out, seen, missed);
    }
    if let Some(frame) = tree.frames.get(id) {
        walk_frame(frame, depth + 1, &tree.path, out, missed);
    } else if node.role == "Iframe" && tree.path.len() >= MAX_FRAME_DEPTH {
        // `snapshot` follows frames only so deep: say so rather than let a
        // deeper frame's contents be silently missing.
        out.push((format!("{}{DEEP_FRAMES_NOTE}", " ".repeat((depth + 1).min(12))), None));
        *missed = true;
    }
}

/// How many frames deep `snapshot` follows frames inside frames.
pub const MAX_FRAME_DEPTH: usize = 3;

/// The line under an iframe sitting inside the deepest frame followed.
pub const DEEP_FRAMES_NOTE: &str = "(frames nested deeper than 3 are not shown)";

/// A frame's own tree, under its iframe's line, every locator behind the
/// frame's step.
fn walk_frame(frame: &FrameTree, depth: usize, path: &[Value], out: &mut Vec<Printed>, missed: &mut bool) {
    if let Some(why) = &frame.unreadable {
        out.push((format!("{}(frame contents could not be read: {})", " ".repeat(depth.min(12)), sanitize(why)), None));
        if !frame.cross_origin {
            *missed = true;
        }
        return;
    }
    let Some(root) = frame.nodes.first() else { return };
    let mut inner = path.to_vec();
    inner.push(frame.step.clone());
    let tree = Tree {
        by_id: frame.nodes.iter().map(|n| (n.id.as_str(), n)).collect(),
        frames: frame.frames.iter().map(|f| (f.iframe_id.as_str(), f)).collect(),
        path: inner,
    };
    walk(&root.id, depth, &tree, out, &mut std::collections::HashSet::new(), missed);
}

/// Pure rendering: every rule (folding, password redaction, the locator
/// suffix, the line cap, the depth cap, the empty-tree sentence) lives
/// here so it can be tested against hand-built nodes with no browser at
/// all. The root is `nodes[0]`, matching what `getFullAXTree` returns.
pub fn render(nodes: &[AxNode], limit: usize) -> String {
    render_frames(nodes, &[], limit)
}

/// `render`, with each frame's own tree printed under its iframe's line.
/// The line limit counts every printed line, frames included.
pub fn render_frames(nodes: &[AxNode], frames: &[FrameTree], limit: usize) -> String {
    render_frames_with_lines(nodes, frames, limit).0
}

/// `render_frames`, and the lines it printed as data (only those that carry
/// a locator, and only those inside the limit). The text is byte-identical.
pub fn render_frames_with_lines(nodes: &[AxNode], frames: &[FrameTree], limit: usize) -> (String, Vec<SnapLine>) {
    let read = render_frames_read(nodes, frames, limit);
    (read.text, read.lines)
}

/// One read of the page: the text, the lines it printed as data, and
/// whether `limit` cut it short (the text then ends "... and N more").
#[derive(Debug, Clone, PartialEq)]
pub struct PageRead {
    pub text: String,
    pub lines: Vec<SnapLine>,
    /// True when the page had more lines than the read printed, or a frame
    /// in it could not be read (other than one from another site, which
    /// nothing can reach) or sat deeper than `MAX_FRAME_DEPTH`: what it
    /// shows is not the whole page.
    pub cut: bool,
}

/// `render_frames_with_lines`, saying as well whether the limit cut it.
pub fn render_frames_read(nodes: &[AxNode], frames: &[FrameTree], limit: usize) -> PageRead {
    const EMPTY: &str = "the page has nothing a locator could name";
    let empty = || PageRead { text: EMPTY.to_string(), lines: vec![], cut: false };
    let Some(root) = nodes.first() else {
        return empty();
    };
    let tree = Tree {
        by_id: nodes.iter().map(|n| (n.id.as_str(), n)).collect(),
        frames: frames.iter().map(|f| (f.iframe_id.as_str(), f)).collect(),
        path: vec![],
    };
    let mut lines = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut missed = false;
    walk(&root.id, 0, &tree, &mut lines, &mut seen, &mut missed);
    if lines.is_empty() {
        return empty();
    }
    let total = lines.len();
    let mut out = lines.iter().take(limit).map(|(t, _)| t.clone()).collect::<Vec<_>>().join("\n");
    let data: Vec<SnapLine> = lines.into_iter().take(limit).filter_map(|(_, d)| d).collect();
    if total > limit {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("... and {} more (raise the limit, or scope the probe)", total - limit));
    }
    PageRead { text: out, lines: data, cut: total > limit || missed }
}

/// `Accessibility.getFullAXTree`, retried once after `Accessibility.enable`
/// if the domain was never switched on - real Edge answers the first call
/// that way rather than enabling it implicitly.
pub async fn snapshot<D: Driver>(d: &mut D, limit: usize) -> Result<String, CdpError> {
    snapshot_with_lines(d, limit).await.map(|(text, _)| text)
}

/// `snapshot`, and the printed lines as data.
pub async fn snapshot_with_lines<D: Driver>(d: &mut D, limit: usize) -> Result<(String, Vec<SnapLine>), CdpError> {
    snapshot_read(d, limit).await.map(|read| (read.text, read.lines))
}

/// `snapshot_with_lines`, saying as well whether the limit cut it.
pub async fn snapshot_read<D: Driver>(d: &mut D, limit: usize) -> Result<PageRead, CdpError> {
    let result = match d.call("Accessibility.getFullAXTree", json!({})).await {
        Ok(v) => v,
        Err(CdpError::Protocol { message, .. }) if message.to_lowercase().contains("enabled") => {
            d.call("Accessibility.enable", json!({})).await?;
            d.call("Accessibility.getFullAXTree", json!({})).await?
        }
        Err(e) => return Err(e),
    };
    let nodes = parse_nodes(&result);
    // Frames inside frames are followed three deep (`MAX_FRAME_DEPTH`; an
    // iframe deeper still prints `DEEP_FRAMES_NOTE`) - written out rather
    // than recursive, because a recursive async fn needs a boxed future,
    // and the driver's futures do not promise to be `Send`.
    let mut frames = frames_in(d, &nodes).await;
    for frame in &mut frames {
        frame.frames = frames_in(d, &frame.nodes).await;
        for inner in &mut frame.frames {
            inner.frames = frames_in(d, &inner.nodes).await;
        }
    }
    Ok(render_frames_read(&nodes, &frames, limit))
}

/// The trees of the `Iframe` nodes in `nodes` (one level), each with the
/// step that reaches its iframe. Never an error: a frame that cannot be
/// read becomes one line saying why, so one bad frame cannot cost the
/// whole snapshot.
async fn frames_in<D: Driver>(d: &mut D, nodes: &[AxNode]) -> Vec<FrameTree> {
    let mut out = vec![];
    let iframes: Vec<&AxNode> = nodes.iter().filter(|n| n.role == "Iframe" && !n.ignored).collect();
    for (k, node) in iframes.iter().enumerate() {
        let Some(backend) = node.backend else {
            // Not followed, so the read did not see what it holds: one line
            // says so, and the read counts as partial.
            out.push(FrameTree {
                iframe_id: node.id.clone(),
                step: frame_step(&node.name, &Value::Null, k),
                nodes: vec![],
                unreadable: Some(NO_FRAME_NODE.to_string()),
                cross_origin: false,
                frames: vec![],
            });
            continue;
        };
        let (step, read) = frame_tree(d, node, k, backend).await;
        let (nodes, unreadable, cross_origin) = match read {
            Ok(inner) => (inner, None, false),
            Err(Unread { why, cross_origin }) => (vec![], Some(why), cross_origin),
        };
        out.push(FrameTree { iframe_id: node.id.clone(), step, nodes, unreadable, cross_origin, frames: vec![] });
    }
    out
}

/// `this` is an iframe: its place among the visible iframes of its own
/// document, which is what `{"css": "iframe", "nth": k}` counts.
const IFRAME_INDEX_JS: &str = r#"function() {
  const seen = (e) => { const r = e.getBoundingClientRect(); return e.checkVisibility({ visibilityProperty: true }) && r.width > 0 && r.height > 0; };
  return Array.from(this.ownerDocument.querySelectorAll('iframe')).filter(seen).indexOf(this);
}"#;

/// Said under an iframe Chrome gave no DOM node for.
pub const NO_FRAME_NODE: &str = "Chrome gave no node for it";

/// Said under an iframe the page cannot reach into.
pub const CROSS_ORIGIN_FRAME: &str =
    "the frame holds a page from another site (or has not loaded), which Auto Run cannot reach";

/// Why a frame's tree could not be read, and whether that is because the
/// page cannot reach into it (`FrameTree::cross_origin`).
struct Unread {
    why: String,
    cross_origin: bool,
}

impl From<String> for Unread {
    fn from(why: String) -> Self {
        Unread { why, cross_origin: false }
    }
}

/// The step that reaches this iframe (see `frame_step`), and its tree - or
/// why it could not be read.
async fn frame_tree<D: Driver>(d: &mut D, node: &AxNode, k: usize, backend: i64) -> (Value, Result<Vec<AxNode>, Unread>) {
    let described = d.call("DOM.describeNode", json!({ "backendNodeId": backend })).await;
    let attrs = described.as_ref().map(|v| v["node"]["attributes"].clone()).unwrap_or(Value::Null);
    let handle = page::resolve_backend(d, backend).await.ok();
    // The accessibility tree does not list iframes in page order, so the
    // fallback `nth` is counted the way the CSS step will count it.
    let mut index = k;
    let mut reachable = None;
    if let Some(h) = &handle {
        if let Some(i) = page::call_value(d, h, IFRAME_INDEX_JS, &[]).await.ok().and_then(|v| v.as_u64()) {
            index = i as usize;
        }
        reachable = page::call_value(d, h, FRAME_JS, &[]).await.ok().and_then(|v| v.as_str().map(str::to_string));
    }
    let step = frame_step(&node.name, &attrs, index);
    if reachable.as_deref() != Some("frame") {
        // Only the page's own answer that it cannot reach the document is
        // another site; a frame that could not even be asked may still be
        // loading, and leaves the read partial.
        let cross_origin = reachable.as_deref() == Some("unreachable");
        return (step, Err(Unread { why: CROSS_ORIGIN_FRAME.to_string(), cross_origin }));
    }
    let Some(frame_id) = described.ok().and_then(|v| v["node"]["frameId"].as_str().map(str::to_string)) else {
        return (step, Err("Chrome gave no frame id for it".to_string().into()));
    };
    match d.call("Accessibility.getFullAXTree", json!({ "frameId": frame_id })).await {
        Ok(v) => (step, Ok(parse_nodes(&v))),
        Err(e) => (step, Err(e.to_string().into())),
    }
}

/// The locator step for an iframe: by role and name when it has a name
/// (Chrome's role `Iframe` matches visible iframes by their title - proven
/// in `browser_live::frame_spike_role_lookup_through_a_frame_document`),
/// else by its `id`, else by its `title`, else as the `k`th iframe. Any id
/// or title prints a valid selector that matches it (`css_quoted`).
pub fn frame_step(name: &str, attrs: &Value, k: usize) -> Value {
    if !name.trim().is_empty() {
        return json!({ "role": "Iframe", "name": name, "exact": true });
    }
    let attr = |want: &str| -> Option<String> {
        let list = attrs.as_array()?;
        list.chunks(2).find(|p| p[0].as_str() == Some(want)).and_then(|p| p.get(1)?.as_str().map(str::to_string))
    };
    if let Some(id) = attr("id").filter(|s| !s.trim().is_empty()) {
        if plain_css_identifier(&id) {
            return json!({ "css": format!("iframe#{id}") });
        }
        return json!({ "css": format!("iframe[id='{}']", css_quoted(&id)) });
    }
    if let Some(title) = attr("title").filter(|s| !s.trim().is_empty()) {
        return json!({ "css": format!("iframe[title='{}']", css_quoted(&title)) });
    }
    json!({ "css": "iframe", "nth": k })
}

/// Whether `id` can follow `#` in a selector as it is: a letter or `_`
/// first, then letters, digits, `_` and `-`. Anything else (a space, a
/// colon, a leading digit, a quote) goes in an attribute step instead.
fn plain_css_identifier(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// `value` as the inside of a single-quoted CSS string: `\` and `'`
/// escaped, and a control character (a line break among them), which a CSS
/// string may not hold raw, written as its code point.
fn css_quoted(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' | '\'' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push_str(&format!("\\{:x} ", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// `this` is the element. One call for everything `probe` prints besides
/// visibility, which reuses `VISIBLE_JS` - the same check every other
/// action already trusts, rather than a second copy of it here. A
/// password input's `.value` is never read: printing what someone typed
/// into a password field is exactly the leak this whole module exists to
/// avoid, so `[password]` stands in for it instead.
pub const PROBE_SUMMARY_JS: &str = r#"function() {
  const r = this.getBoundingClientRect();
  const text = this.type === 'password' ? '[password]' : (this.innerText || this.value || '').trim();
  return { tag: this.tagName.toLowerCase(), text, rect: [r.x, r.y, r.width, r.height] };
}"#;

fn trim_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// What a locator matches right now, for a person or an assistant deciding
/// whether it is safe to use. Empty is reported, not treated as an error:
/// a target that matches nothing is exactly what a caller needs to know.
pub async fn probe<D: Driver>(d: &mut D, target: &Target) -> Result<String, CdpError> {
    let found = resolve_explained(d, target).await?;
    let handles = found.handles;
    if handles.is_empty() {
        return Ok(match found.unreachable_frame {
            Some(why) => format!("matches: 0 - {why}"),
            None => format!("matches: 0 - nothing on the page answers to {}", target.describe()),
        });
    }
    let mut lines = vec![format!("matches: {}", handles.len())];
    for handle in handles.iter().take(10) {
        let visible = page::call_value(d, handle, VISIBLE_JS, &[]).await?.as_bool().unwrap_or(false);
        let summary = page::call_value(d, handle, PROBE_SUMMARY_JS, &[]).await?;
        let tag = summary["tag"].as_str().unwrap_or("");
        let text = trim_chars(&sanitize(summary["text"].as_str().unwrap_or("")), 60);
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
