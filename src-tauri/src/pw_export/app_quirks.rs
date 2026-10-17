//! What TCM has learned about the application, written into the Playwright
//! repo's `docs/app-quirks.md` in the repo's own entry shape.
//!
//! TCM owns only the entries it tags with `<!-- tcm:... -->` on the line
//! right after the `### ` heading: those are replaced in place on every
//! export. Everything else in the file - the repo's own entries, its intro,
//! a hand-edited quirk - is never touched; new entries go on the end.

use crate::autorun::quirks::{day_of, Quirk};
use crate::autorun::recipe::SignInRecipe;

/// Where the quirks catalogue lives in the Playwright repo.
pub const APP_QUIRKS_REL: &str = "docs/app-quirks.md";

/// The heading of a file TCM has to create.
const NEW_FILE_HEADING: &str = "# Known application quirks\n";

/// The longest an entry's `###` title is.
const TITLE_CHARS: usize = 80;

/// One entry, with the repo's six labelled fields. `tag` is what marks it
/// as TCM's (`tcm:<quirk id>`, `tcm:popup:<name>`, `tcm:loading:<name>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcmQuirkEntry {
    pub tag: String,
    pub title: String,
    pub symptom: String,
    pub cause: String,
    pub fix: String,
    pub established: String,
    pub permanent: String,
    pub class: String,
}

/// One line: every run of whitespace, line breaks included, becomes a
/// single space, so a note can never start a heading or a `---` rule inside
/// the file.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The first sentence of a note, at most `TITLE_CHARS` characters.
fn title_of(text: &str) -> String {
    let text = one_line(text);
    let end = text
        .char_indices()
        .find(|(i, c)| matches!(c, '.' | '!' | '?') && text[i + c.len_utf8()..].starts_with(' '))
        .map_or(text.len(), |(i, _)| i);
    let first = text[..end].trim_end_matches(['.', '!', '?']).trim();
    if first.chars().count() <= TITLE_CHARS {
        return first.to_string();
    }
    let cut: String = first.chars().take(TITLE_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

const SYMPTOM: &str = "a step on this page fails without this handling (from TCM)";
const NOT_PROMOTED: &str = "not yet promoted";
const CLASS: &str = "app quirk";

/// The entries TCM hands over: its active quirks that came out of one of
/// the exported cases, then the sign-in recipe's pop-ups and loading
/// indicators (which belong to the whole project, so they travel with any
/// export).
pub fn quirk_entries(quirks: &[Quirk], case_ids: &[i32], recipe: Option<&SignInRecipe>) -> Vec<TcmQuirkEntry> {
    let mut out: Vec<TcmQuirkEntry> = Vec::new();
    for q in quirks {
        if !q.is_active() || !q.sources.iter().any(|s| case_ids.contains(&s.case_id)) {
            continue;
        }
        let text = one_line(&q.text);
        out.push(TcmQuirkEntry {
            tag: format!("tcm:{}", q.id),
            title: title_of(&text),
            symptom: SYMPTOM.into(),
            cause: text.clone(),
            fix: format!("Handle as TCM does: {text}"),
            established: day_of(&q.at),
            permanent: NOT_PROMOTED.into(),
            class: CLASS.into(),
        });
    }
    let Some(recipe) = recipe else { return out };
    // The recipe keeps no date for these.
    let established = "not recorded (from TCM's sign-in recipe)".to_string();
    for p in &recipe.close_when_shown {
        let name = one_line(&p.name);
        out.push(TcmQuirkEntry {
            tag: format!("tcm:popup:{name}"),
            title: format!("{name} pops up and blocks the page"),
            symptom: SYMPTOM.into(),
            cause: format!(
                "the {name} can be up over the page's controls, so an action behind it fails                  (TCM's sign-in recipe closes it)"
            ),
            fix: format!(
                "close it with {} whenever {} is visible",
                one_line(&p.close.describe()),
                one_line(&p.shown.describe())
            ),
            established: established.clone(),
            permanent: NOT_PROMOTED.into(),
            class: CLASS.into(),
        });
    }
    for l in &recipe.wait_while_shown {
        let name = one_line(&l.name);
        out.push(TcmQuirkEntry {
            tag: format!("tcm:loading:{name}"),
            title: format!("{name} loading indicator"),
            symptom: SYMPTOM.into(),
            cause: format!(
                "while the {name} is up the page is still loading, so an action started then times out                  (TCM's sign-in recipe waits it out)"
            ),
            fix: format!(
                "wait while {} is visible, up to {} s",
                one_line(&l.shown.describe()),
                l.limit_ms() / 1000
            ),
            established: established.clone(),
            permanent: NOT_PROMOTED.into(),
            class: CLASS.into(),
        });
    }
    out
}

/// An entry in the repo's shape, without a trailing newline. The tag sits
/// on the line right after the heading.
fn render(e: &TcmQuirkEntry) -> String {
    format!(
        "### {}\n<!-- {} -->\n\n**Symptom in a failing run:** {}\n\n**Cause:** {}\n\n**Fix pattern (raw spec):** {}\n\n\
         **Established live:** {}\n\n**Permanent fix:** {}\n\n**Class:** {}",
        e.title, e.tag, e.symptom, e.cause, e.fix, e.established, e.permanent, e.class
    )
}

/// Where the entry whose tag is on line `tag_at` ends: the line after its
/// last non-blank line, before the next rule or heading (a rule or heading
/// inside a code fence does not end it).
fn entry_end(lines: &[String], tag_at: usize) -> usize {
    let mut fenced = false;
    let mut end = lines.len();
    for (i, l) in lines.iter().enumerate().skip(tag_at + 1) {
        let t = l.trim();
        if t.starts_with("```") {
            fenced = !fenced;
        } else if !fenced && (t == "---" || t.starts_with("### ")) {
            end = i;
            break;
        }
    }
    while end > tag_at + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    end
}

/// `existing` (the repo's `docs/app-quirks.md`, if it has one) with
/// `entries` merged in: an entry whose tag is already there is replaced
/// where it stands, a new one goes on the end after a `---` rule, and
/// nothing untagged is touched. `None` when the file would not change -
/// no entries, or every one already written exactly so.
pub fn merge_app_quirks(existing: Option<&str>, entries: &[TcmQuirkEntry]) -> Option<String> {
    // One entry per tag; the first of two with the same tag wins.
    let mut seen: Vec<&str> = Vec::new();
    let entries: Vec<&TcmQuirkEntry> = entries
        .iter()
        .filter(|e| {
            let fresh = !seen.contains(&e.tag.as_str());
            seen.push(&e.tag);
            fresh
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    let base = existing.filter(|s| !s.trim().is_empty());
    let text = base.unwrap_or(NEW_FILE_HEADING);
    let crlf = text.contains("\r\n");
    let own = |s: String| if crlf { s.replace('\n', "\r\n") } else { s };

    let mut lines: Vec<String> = text.split('\n').map(String::from).collect();
    let mut appended: Vec<&TcmQuirkEntry> = Vec::new();
    for e in entries {
        let tag = format!("<!-- {} -->", e.tag);
        let Some(at) = lines.iter().position(|l| l.trim() == tag) else {
            appended.push(e);
            continue;
        };
        let start = if at > 0 && lines[at - 1].trim_start().starts_with("### ") { at - 1 } else { at };
        let end = entry_end(&lines, at);
        // Each line of the block ends as the lines it replaces do.
        let last_has_cr = lines[end - 1].ends_with('\r');
        let mut fresh: Vec<String> = render(e).split('\n').map(String::from).collect();
        if crlf {
            let last = fresh.len() - 1;
            for (i, l) in fresh.iter_mut().enumerate() {
                if i < last || last_has_cr {
                    l.push('\r');
                }
            }
        }
        lines.splice(start..end, fresh);
    }
    let mut out = lines.join("\n");
    if !appended.is_empty() {
        // One blank line, then a rule, whatever the file ended in; the file
        // ends in exactly one line break.
        out.truncate(out.trim_end_matches(['\r', '\n']).len());
        out.push_str(&own("\n\n".to_string()));
        for e in appended {
            out.push_str(&own(format!("---\n\n{}\n\n", render(e))));
        }
        out.truncate(out.trim_end_matches(['\r', '\n']).len());
        out.push_str(&own("\n".to_string()));
    }
    (Some(out.as_str()) != existing).then_some(out)
}
