//! Microsoft.VSTS.TCM.Steps XML build/parse, ported from v1
//! app/utils/xml_builder.py against tests/test_xml_builder.py as golden
//! vectors. Semantics preserved exactly:
//! - step ids start at 2; `last` == last step id
//! - empty steps list builds the single placeholder step
//! - a step with an Expected Result is a `ValidateStep`, one without is an
//!   `ActionStep` - the one place v2 deliberately departs from v1, which
//!   wrote every step as an ActionStep (see `step_type`)
//! - parse strips REAL HTML markup only. v1 removed every `<...>` run, so a
//!   literal "<cycleId>" a user typed did not survive a round-trip; that was
//!   silent data loss, and `strip_tags` explains what replaced it.
//! - malformed XML parses to an empty list, never an error

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::{Deserialize, Serialize};

/// Tag names Azure DevOps' rich-text editor actually emits into a step.
///
/// The list is the whole trick. ADO stores each step's HTML *escaped*
/// inside `parameterizedString`, so by the time it has been unescaped, real
/// markup (`<P>`, `<BR/>`) and text somebody typed (`<cycleId>`) look
/// exactly alike - there is no structural difference left to use. Matching
/// against what the editor can actually produce is the only thing that
/// separates them.
const HTML_TAGS: [&str; 42] = [
    "a", "b", "big", "blockquote", "br", "caption", "center", "code", "col", "colgroup", "dd",
    "div", "dl", "dt", "em", "font", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "li",
    "ol", "p", "pre", "s", "small", "span", "strike", "strong", "sub", "sup", "table", "tbody",
    "td", "th", "thead", "tr",
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Step {
    pub action: String,
    pub expected: String,
    /// A Shared Steps reference - `<compref ref="N">` in the Steps XML - as
    /// the id of the Shared Steps work item. Its steps live in THAT item,
    /// so `action` and `expected` stay empty and the app never edits them;
    /// it only keeps, moves or drops the reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<i32>,
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The HTML layer. Azure DevOps renders a step's `parameterizedString` as
/// HTML, so plain text written without this is read as markup: a typed
/// `<cycleId>` vanished from ADO's own view, and a typed `&lt;` came back as
/// `<`. The parser already undoes both layers, and it reads older
/// single-escaped steps the same as before.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Plain step text as it goes into the XML: HTML-escaped, then XML-escaped.
fn step_text(text: &str) -> String {
    escape_xml(&escape_html(text))
}

/// One `<step>` element built from plain text.
fn step_node(id: &str, step: &Step) -> String {
    format!(
        "<step id=\"{id}\" type=\"{}\"><parameterizedString isformatted=\"true\">{}</parameterizedString><parameterizedString isformatted=\"true\">{}</parameterizedString></step>",
        step_type(&step.expected),
        step_text(&step.action),
        step_text(&step.expected),
    )
}

/// A Shared Steps reference with no copy of its steps. Azure DevOps expands
/// it from the referenced work item.
fn compref_node(id: &str, reference: i32) -> String {
    format!("<compref id=\"{id}\" ref=\"{reference}\" />")
}

/// The attribute's raw value, or "" when it is absent.
fn attr(e: &BytesStart, key: &[u8]) -> String {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| String::from_utf8(a.value.to_vec()).ok())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Step,
    Compref,
}

fn node_kind(e: &BytesStart) -> Option<NodeKind> {
    match e.name().as_ref() {
        b"step" => Some(NodeKind::Step),
        b"compref" => Some(NodeKind::Compref),
        _ => None,
    }
}

/// One top-level child of `<steps>`: where it sits in the source and the
/// attributes the writers need. `start..open_end` is its opening tag;
/// `start..end` the whole element (equal ends for a self-closing one).
#[derive(Debug, Clone)]
struct Node {
    kind: NodeKind,
    start: usize,
    open_end: usize,
    end: usize,
    id: String,
    ty: String,
    /// A compref's `ref` attribute ("" on a step).
    reference: String,
}

/// A Steps document, tokenised for editing in place.
struct Doc {
    /// Byte range of the root's opening tag.
    root_open: (usize, usize),
    /// The root's `last` attribute (0 when absent or unreadable).
    last: i32,
    /// The highest step/compref id anywhere, nested ones included - they
    /// occupy the same id space.
    max_id: i32,
    nodes: Vec<Node>,
}

fn node_at(kind: NodeKind, e: &BytesStart, start: usize, end: usize) -> Node {
    Node {
        kind,
        start,
        open_end: end,
        end,
        id: attr(e, b"id"),
        ty: attr(e, b"type"),
        reference: attr(e, b"ref"),
    }
}

fn note_id(e: &BytesStart, max_id: &mut i32) {
    if node_kind(e).is_some() {
        if let Ok(n) = attr(e, b"id").trim().parse::<i32>() {
            *max_id = (*max_id).max(n);
        }
    }
}

/// The document's top-level `<step>` / `<compref>` nodes with their byte
/// ranges, the root tag and the id bookkeeping. `None` on empty or
/// malformed input. A compref's nested steps are part of that compref.
fn tokenize(xml: &str) -> Option<Doc> {
    if xml.trim().is_empty() {
        return None;
    }
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut depth = 0usize;
    let mut root_open = None;
    let mut last = 0;
    let mut max_id = 0;
    let mut nodes = vec![];
    let mut open: Option<Node> = None;
    loop {
        let before = reader.buffer_position() as usize;
        let event = reader.read_event().ok()?;
        let after = reader.buffer_position() as usize;
        match event {
            Event::Eof => break,
            Event::Start(e) => {
                depth += 1;
                note_id(&e, &mut max_id);
                if depth == 1 {
                    root_open = Some((before, after));
                    last = attr(&e, b"last").trim().parse().unwrap_or(0);
                } else if depth == 2 {
                    open = node_kind(&e).map(|kind| node_at(kind, &e, before, after));
                }
            }
            Event::Empty(e) => {
                note_id(&e, &mut max_id);
                if depth == 0 {
                    root_open = Some((before, after));
                } else if depth == 1 {
                    if let Some(kind) = node_kind(&e) {
                        nodes.push(node_at(kind, &e, before, after));
                    }
                }
            }
            Event::End(_) => {
                if depth == 2 {
                    if let Some(mut n) = open.take() {
                        n.end = after;
                        nodes.push(n);
                    }
                }
                depth = depth.checked_sub(1)?;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    Some(Doc { root_open: root_open?, last, max_id, nodes })
}

/// The `<step>` and `<compref>` children of the root, in document order.
fn top_level_nodes(xml: &str) -> Option<Vec<Node>> {
    tokenize(xml).map(|d| d.nodes)
}

/// The Steps XML to write when a case with an existing `original` is saved
/// as `steps`: the original edited in place, never rebuilt.
///
/// Rebuilding from the parsed plain text - what every step edit used to do
/// - deleted every other step's formatting and screenshots, renumbered the
/// ids that run results point at, and dropped Shared Steps outright. Here:
/// - old and new local steps are aligned by the longest common subsequence
///   of (action, expected); then a leftover step whose text is unchanged
///   (a move) is its original; then, within each gap between aligned
///   steps, leftovers pair up by position (an edit in place);
/// - an aligned step with the same text is the original node verbatim (its
///   `type` corrected if wrong); one with new text is rebuilt under its old
///   id; a new step gets a fresh id;
/// - a shared step is the first unused original `<compref>` with that ref,
///   or a fresh `<compref>` if the original had none;
/// - original nodes nothing aligned to are dropped;
/// - fresh ids start above both `last` and the highest id anywhere in the
///   original, and `last` never goes down (a deleted step's id is never
///   reissued - its run results would attach to the new step).
///
/// An original that is empty, malformed, has no step nodes or does not
/// parse node-for-node: the build.
///
/// `None` when a shared step's reference is unreadable (`shared: Some(0)`,
/// what `parse_steps_xml` makes of a `<compref>` whose `ref` is not a
/// number) and no unused original `<compref>` with an unreadable `ref` is
/// there to be kept in its place. `ref="0"` names no work item, so there is
/// nothing safe to write.
pub fn merge_steps_xml(original: &str, steps: &[Step]) -> Option<String> {
    let merged = merge_into(original, steps);
    if merged.is_none() && steps.iter().any(|s| s.shared == Some(0)) {
        return None;
    }
    merged.or_else(|| Some(build_steps_xml(steps)))
}

/// The merge proper, or `None` when it cannot be done and the caller falls
/// back to a build. A fresh compref is never written for reference 0.
fn merge_into(original: &str, steps: &[Step]) -> Option<String> {
    let doc = tokenize(original)?;
    let old = parse_steps_xml(original);
    let lines_up = old.len() == doc.nodes.len()
        && old
            .iter()
            .zip(&doc.nodes)
            .all(|(s, n)| s.shared.is_some() == (n.kind == NodeKind::Compref));
    if steps.is_empty() || doc.nodes.is_empty() || !lines_up {
        return None;
    }
    let same = |i: usize, j: usize| old[i].action == steps[j].action && old[i].expected == steps[j].expected;
    let old_local: Vec<usize> = (0..old.len()).filter(|&i| old[i].shared.is_none()).collect();
    let new_local: Vec<usize> = (0..steps.len()).filter(|&j| steps[j].shared.is_none()).collect();

    // 1. Longest common subsequence over the local steps' text.
    let (n, m) = (old_local.len(), new_local.len());
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for a in (0..n).rev() {
        for b in (0..m).rev() {
            dp[a][b] = if same(old_local[a], new_local[b]) {
                dp[a + 1][b + 1] + 1
            } else {
                dp[a + 1][b].max(dp[a][b + 1])
            };
        }
    }
    // origin[j]: the original node new step j continues, if any.
    let mut origin: Vec<Option<usize>> = vec![None; steps.len()];
    let mut used = vec![false; old.len()];
    // gap_*[x]: how many matched pairs precede local step x. Matches pair
    // up in order, so gap g on one side is gap g on the other.
    let mut gap_old = vec![0usize; n];
    let mut gap_new = vec![0usize; m];
    let (mut a, mut b, mut anchors) = (0, 0, 0);
    while a < n || b < m {
        if a < n && b < m && same(old_local[a], new_local[b]) {
            origin[new_local[b]] = Some(old_local[a]);
            used[old_local[a]] = true;
            anchors += 1;
            a += 1;
            b += 1;
        } else if b >= m || (a < n && dp[a + 1][b] >= dp[a][b + 1]) {
            gap_old[a] = anchors;
            a += 1;
        } else {
            gap_new[b] = anchors;
            b += 1;
        }
    }
    // 2. A step moved with its text unchanged is the same node.
    for &j in &new_local {
        if origin[j].is_none() {
            if let Some(&i) = old_local.iter().find(|&&i| !used[i] && same(i, j)) {
                origin[j] = Some(i);
                used[i] = true;
            }
        }
    }
    // 3. An edit in place: within one gap, the k-th leftover old step is
    //    the k-th leftover new step. Leftovers in different gaps are a
    //    deletion and an unrelated new step - pairing those would hand the
    //    deleted step's id, and its run results, to the new one.
    for g in 0..=anchors {
        let spare_old: Vec<usize> =
            (0..n).filter(|&x| gap_old[x] == g && !used[old_local[x]]).map(|x| old_local[x]).collect();
        let spare_new: Vec<usize> =
            (0..m).filter(|&x| gap_new[x] == g && origin[new_local[x]].is_none()).map(|x| new_local[x]).collect();
        for (i, j) in spare_old.into_iter().zip(spare_new) {
            origin[j] = Some(i);
            used[i] = true;
        }
    }

    // 3. Emit in the new order.
    let mut next = doc.last.max(doc.max_id);
    let mut body = String::with_capacity(original.len() + 256);
    for (j, s) in steps.iter().enumerate() {
        if let Some(reference) = s.shared {
            // An unreadable `ref` reads as 0 here exactly as it does in
            // parse_steps_xml, so a reference-0 step takes the first unused
            // unreadable original: they pair up in document order.
            let hit = (0..doc.nodes.len()).find(|&i| {
                !used[i]
                    && doc.nodes[i].kind == NodeKind::Compref
                    && doc.nodes[i].reference.trim().parse::<i32>().unwrap_or(0) == reference
            });
            match hit {
                Some(i) => {
                    used[i] = true;
                    body.push_str(&original[doc.nodes[i].start..doc.nodes[i].end]);
                }
                None if reference == 0 => return None,
                None => {
                    next += 1;
                    body.push_str(&compref_node(&next.to_string(), reference));
                }
            }
            continue;
        }
        match origin[j] {
            Some(i) if same(i, j) => {
                let node = &doc.nodes[i];
                let open = &original[node.start..node.open_end];
                body.push_str(&with_attr(open, "type", step_type(&s.expected)));
                body.push_str(&original[node.open_end..node.end]);
            }
            Some(i) if !doc.nodes[i].id.trim().is_empty() => {
                body.push_str(&step_node(doc.nodes[i].id.trim(), s));
            }
            _ => {
                next += 1;
                body.push_str(&step_node(&next.to_string(), s));
            }
        }
    }
    let (root_start, root_end) = doc.root_open;
    let root = with_attr(&original[root_start..root_end], "last", &next.to_string());
    Some(format!("{root}{body}</steps>"))
}

/// `open` (an opening tag, `<x ...>` or `<x .../>`) with attribute `name`
/// set to `value`: replaced in place when present, otherwise added before
/// the closing `>` or `/>`. The `/>` case is D4: appending after the `/`
/// wrote `<step id="2"/ type="...">`, which is not XML.
fn with_attr(open: &str, name: &str, value: &str) -> String {
    for q in ['"', '\''] {
        let pat = format!(" {name}={q}");
        if let Some(at) = open.find(&pat) {
            let value_start = at + pat.len();
            if let Some(len) = open[value_start..].find(q) {
                return format!("{}{}{}", &open[..value_start], value, &open[value_start + len..]);
            }
        }
    }
    match open.strip_suffix("/>") {
        Some(head) => format!("{} {name}=\"{value}\"/>", head.trim_end()),
        None => format!("{} {name}=\"{value}\">", &open[..open.len() - 1]),
    }
}

/// The `type` Azure DevOps gives a step: a `ValidateStep` carries an Expected
/// Result and is marked Pass/Fail during a run - it is also the only kind an
/// execution-automation runner can judge - while an `ActionStep` is "do
/// this" with nothing to check. The web form decides this from the Expected
/// Result; every step used to be written here as an ActionStep, which
/// silently undid the form's choice on the next bulk update (case 154599,
/// fixed in the form at 05:50 and set back at 09:58 by an upload).
pub fn step_type(expected: &str) -> &'static str {
    if expected.trim().is_empty() {
        "ActionStep"
    } else {
        "ValidateStep"
    }
}

/// The `type` attribute of each top-level node in document order, aligned
/// with `parse_steps_xml`'s output: "" for a shared-step reference and for
/// a step with no type attribute.
pub fn parse_step_types(xml_str: &str) -> Vec<String> {
    top_level_nodes(xml_str)
        .map(|nodes| {
            nodes
                .into_iter()
                .map(|n| if n.kind == NodeKind::Step { n.ty } else { String::new() })
                .collect()
        })
        .unwrap_or_default()
}

/// The original Steps XML with each `<step>`'s `type` corrected to what its
/// Expected Result calls for, and NOTHING else touched - ids, markup,
/// embedded images and `<description/>` all stay as Azure DevOps holds
/// them. `None` when every type is already right (nothing to write), or
/// when `steps` does not line up with the XML one-for-one (a guess here
/// could retype the wrong step, so refuse).
///
/// This is how a case the app once wrote with every step as an ActionStep
/// gets repaired by the next save that touches it, without paying the
/// markup loss that rebuilding the XML from plain text would cost.
///
/// Shared-step references, and the steps nested in them, are never touched.
pub fn retype_steps_xml(xml: &str, steps: &[Step]) -> Option<String> {
    let nodes = top_level_nodes(xml)?;
    if nodes.len() != steps.len() {
        return None;
    }
    let mut out = String::with_capacity(xml.len() + 16);
    let mut copied = 0;
    let mut changed = false;
    for (n, s) in nodes.iter().zip(steps) {
        match (n.kind, s.shared) {
            (NodeKind::Compref, Some(_)) => continue,
            (NodeKind::Step, None) => {}
            // The list says "shared" where the XML has a step, or the
            // other way round: it does not line up, so do not guess.
            _ => return None,
        }
        let open = &xml[n.start..n.open_end];
        let retagged = with_attr(open, "type", step_type(&s.expected));
        if retagged != open {
            changed = true;
            out.push_str(&xml[copied..n.start]);
            out.push_str(&retagged);
            copied = n.open_end;
        }
    }
    out.push_str(&xml[copied..]);
    changed.then_some(out)
}

/// Build the XML string for the Microsoft.VSTS.TCM.Steps field. A shared
/// step becomes a `<compref>` that takes its place in the id sequence.
pub fn build_steps_xml(steps: &[Step]) -> String {
    if steps.is_empty() {
        return "<steps id=\"0\" last=\"1\"><step id=\"2\" type=\"ActionStep\"><parameterizedString isformatted=\"true\"></parameterizedString><parameterizedString isformatted=\"true\"></parameterizedString></step></steps>".to_string();
    }
    let mut out = format!("<steps id=\"0\" last=\"{}\">", steps.len() + 1);
    for (i, step) in steps.iter().enumerate() {
        let id = (i + 2).to_string();
        out.push_str(&match step.shared {
            Some(reference) => compref_node(&id, reference),
            None => step_node(&id, step),
        });
    }
    out.push_str("</steps>");
    out
}

/// The index of the `>` that closes a real HTML tag opening at `open`, or
/// `None` when this `<` is just a less-than sign.
fn html_tag_end(c: &[char], open: usize) -> Option<usize> {
    let mut i = open + 1;
    if i < c.len() && c[i] == '/' {
        i += 1;
    }
    let name_start = i;
    while i < c.len() && c[i].is_ascii_alphanumeric() {
        i += 1;
    }
    // "a < b" and "start_date < GETUTCDATE()": no name, so no tag.
    if i == name_start {
        return None;
    }
    let name: String = c[name_start..i].iter().collect::<String>().to_ascii_lowercase();
    if !HTML_TAGS.contains(&name.as_str()) {
        return None;
    }
    // Attributes may quote a '>' (`<img alt="a>b">`), so track quoting
    // rather than scanning for the first '>'.
    let mut quote: Option<char> = None;
    while i < c.len() {
        match (quote, c[i]) {
            (Some(q), ch) if ch == q => quote = None,
            (Some(_), _) => {}
            (None, ch @ ('"' | '\'')) => quote = Some(ch),
            (None, '>') => return Some(i),
            // An unterminated `<p` followed by another `<` was never a tag.
            (None, '<') => return None,
            (None, _) => {}
        }
        i += 1;
    }
    None
}

/// Remove real HTML markup, and only that.
///
/// This used to delete every `<...>` run, which is what v1 did and what
/// this module's doc comment used to promise. It cost a developer 62
/// fragments across 20 test cases: SQL steps written as
/// `WHERE performance_cycle_id = <cycleId>` came back as
/// `WHERE performance_cycle_id =` - a query the tester cannot run, in a
/// step whose entire purpose is to run it. Silently, with the file still
/// valid and the text still reading plausibly enough to skim past.
///
/// So `<cycleId>`, `<next assessment stage>` and `a < b` now survive, while
/// `<P>` and `<BR/>` still go. A spec quote naming a real HTML element
/// (`<div>`, `<img>`, and note `<textarea>` is NOT in the list precisely
/// because a rich-text editor never emits one) is the residual ambiguity -
/// the app cannot tell that from markup, and `validate_cases` warns about
/// it instead.
fn strip_tags(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(end) = html_tag_end(&chars, i) {
                out.push(' ');
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Whether `text` contains something `strip_tags` will treat as markup.
/// Used by `validate_cases` so an author quoting `<div>` from a spec finds
/// out before the round trip eats it, rather than during review.
pub fn contains_html_markup(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    (0..chars.len()).any(|i| chars[i] == '<' && html_tag_end(&chars, i).is_some())
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A `<compref>` as a step: the reference only. An unreadable `ref` reads
/// as 0, so the node still counts as a shared step. It is never mistaken
/// for an empty local step, which a save would rebuild without the
/// reference.
fn shared_step(e: &BytesStart) -> Step {
    Step {
        shared: Some(attr(e, b"ref").trim().parse::<i32>().unwrap_or(0)),
        ..Default::default()
    }
}

/// Parse the Steps XML into Step structs: one per top-level `<step>` or
/// `<compref>`, in document order. A compref's nested steps belong to the
/// Shared Steps work item and are not flattened into the case. Returns []
/// on empty or malformed input, mirroring v1 parse_steps_xml.
pub fn parse_steps_xml(xml_str: &str) -> Vec<Step> {
    if xml_str.trim().is_empty() {
        return vec![];
    }
    let mut reader = Reader::from_str(xml_str);
    reader.config_mut().trim_text(false);

    let mut out: Vec<Step> = vec![];
    // Element depth: the root `<steps>` is 1, its children 2, theirs 3.
    let mut depth = 0usize;
    // The text fields of the top-level `<step>` being read.
    let mut parts: Option<Vec<String>> = None;
    let mut capture: Option<(String, usize)> = None; // (buffer, nested depth)

    loop {
        match reader.read_event() {
            Err(_) => return vec![], // malformed
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                depth += 1;
                if let Some((buf, nested)) = capture.as_mut() {
                    *nested += 1;
                    buf.push(' ');
                    continue;
                }
                match (depth, e.name().as_ref()) {
                    (2, b"step") => parts = Some(vec![]),
                    (2, b"compref") => out.push(shared_step(&e)),
                    (3, b"parameterizedString") if parts.is_some() => {
                        capture = Some((String::new(), 0))
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                if let Some((buf, nested)) = capture.as_mut() {
                    if *nested == 0 {
                        // Two layers of escaping: quick-xml (and the
                        // GeneralRef arm below) undid the XML's, leaving the
                        // HTML that ADO stores escaped inside it - `&quot;`,
                        // `&amp;`, `&lt;P&gt;`. Undo that layer too, THEN
                        // strip tags, so real markup goes and a typed
                        // `<cycleId>` stays (see HTML_TAGS).
                        let text = collapse_ws(&strip_tags(&unescape_html(buf)));
                        if let Some(p) = parts.as_mut() {
                            p.push(text);
                        }
                        capture = None;
                    } else {
                        *nested -= 1;
                        buf.push(' ');
                    }
                } else if depth == 2 && e.name().as_ref() == b"step" {
                    if let Some(p) = parts.take() {
                        out.push(Step {
                            action: p.first().cloned().unwrap_or_default(),
                            expected: p.get(1).cloned().unwrap_or_default(),
                            shared: None,
                        });
                    }
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Empty(e)) => {
                if let Some((buf, _)) = capture.as_mut() {
                    buf.push(' ');
                    continue;
                }
                match (depth + 1, e.name().as_ref()) {
                    (2, b"step") => out.push(Step::default()),
                    (2, b"compref") => out.push(shared_step(&e)),
                    // An empty field is still a field: without this, an
                    // empty action moved the expected result into its slot.
                    (3, b"parameterizedString") => {
                        if let Some(p) = parts.as_mut() {
                            p.push(String::new());
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if let Some((buf, _)) = capture.as_mut() {
                    match t.decode() {
                        Ok(s) => buf.push_str(&s),
                        Err(_) => return vec![],
                    }
                }
            }
            // quick-xml surfaces &amp; / &lt; / &#... as separate events.
            // Resolving them here gives one unescape pass (like v1's
            // html.unescape); strip_tags then removes anything tag-shaped.
            Ok(Event::GeneralRef(r)) => {
                if let Some((buf, _)) = capture.as_mut() {
                    let name = String::from_utf8_lossy(r.as_ref()).to_string();
                    buf.push_str(&unescape_html(&format!("&{name};")));
                }
            }
            Ok(_) => {}
        }
    }
    // A dangling capture or step means the XML was truncated mid-element.
    if capture.is_some() || parts.is_some() {
        return vec![];
    }
    out
}

/// The step ids in document order from a Steps XML blob, index-aligned with
/// `parse_steps_xml`. ADO assigns arbitrary ids (not 2,3,4...) once a case
/// has been edited in the web UI, and iterationDetails must reference the
/// REAL ids - never index math. A shared-step reference (and a step with
/// no id) holds "": no per-step result can be recorded against it, and
/// `build_iteration_details` skips it.
pub fn parse_step_ids(xml_str: &str) -> Vec<String> {
    top_level_nodes(xml_str)
        .map(|nodes| {
            nodes
                .into_iter()
                .map(|n| if n.kind == NodeKind::Step { n.id } else { String::new() })
                .collect()
        })
        .unwrap_or_default()
}

/// Flatten an ADO rich-text/HTML field to clean plain text, ported from v1
/// html_to_text: list items become bullets, block ends become line breaks,
/// all tags stripped, entities unescaped, blank runs collapsed.
pub fn html_to_text(html: &str) -> String {
    if html.trim().is_empty() {
        return String::new();
    }
    let li = regex::Regex::new(r"(?i)<\s*li[^>]*>").unwrap();
    let block = regex::Regex::new(r"(?i)<\s*(br|/p|/div|/h[1-6]|/tr)\s*/?>").unwrap();
    let tag = regex::Regex::new(r"<[^>]+>").unwrap();

    let s = li.replace_all(html, "\n\u{2022} ");
    let s = block.replace_all(&s, "\n");
    let s = tag.replace_all(&s, "");
    let s = unescape_html(&s).replace('\u{a0}', " ");

    let mut lines: Vec<String> = vec![];
    let mut blank = false;
    for ln in s.split('\n') {
        let ln = ln.trim();
        if !ln.is_empty() {
            lines.push(ln.to_string());
            blank = false;
        } else if !lines.is_empty() && !blank {
            lines.push(String::new());
            blank = true;
        }
    }
    lines.join("\n").trim().to_string()
}

/// Minimal HTML entity unescape covering what ADO fields actually contain:
/// the five XML entities, &nbsp;, and numeric references.
fn unescape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let end = match rest.find(';') {
            Some(e) if e <= 12 => e,
            _ => {
                out.push('&');
                rest = &rest[1..];
                continue;
            }
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ if entity.starts_with('#') => {
                let num = entity.trim_start_matches('#');
                let cp = if let Some(hex) = num.strip_prefix(['x', 'X']) {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    num.parse().ok()
                };
                cp.and_then(char::from_u32)
            }
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
