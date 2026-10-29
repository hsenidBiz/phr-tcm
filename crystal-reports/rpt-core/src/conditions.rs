//! Section visibility formulas — the conditional `Section_Visibility` reserved formula
//! that decides, per record, whether a section prints.
//!
//! Wire shape (pinned against the engine-authored donors in every subreport of the
//! IndDetailedEval family): each section is followed by top-level `0x00ff
//! SectionFormatWrapper` records whose `visibility_formula` slot is a FieldRef
//! (`@Section_Visibility`, kind 1, index = the GLOBAL ordinal of the `0x76` formula body
//! in pre-order — the same index space the reader's condition resolver uses). The body
//! itself is an ordinary `0x0077`-wrapped formula record, so rewriting it reuses the
//! setFormula rebuild machinery, addressed by ordinal instead of by name.
//!
//! Works at any scope: main-report sections directly, subreport sections through
//! `EditOp::InSubreport` (the lift makes the sub a full report, visibility slots included).

use rpt_reader::fields::FieldValue;
use rpt_reader::raw::RecordNode;
use rpt_reader::{Rpt, StreamId};

use crate::edit::SecSel;

/// Rewrite the `Section_Visibility` formula of the selected section to `source` (a Crystal
/// Boolean expression evaluated per record; `True` always shows, `False` always hides).
///
/// # Errors
///
/// The section does not resolve; it carries no visibility formula (creation from nothing is
/// a separate donor splice — none of this workspace's reports needs it); an unresolvable
/// reference in the new source; or a writer failure.
pub fn set_section_visibility(rpt: &Rpt, sel: &SecSel, source: &str) -> Result<Vec<u8>, String> {
    let located = crate::locate::locate(rpt);
    let target = crate::edit::find_section_by_names(&located, sel)?;
    let sec = target.section_rec.ok_or("the section's own record was not located")?;

    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();

    // The section's 0xff wrappers: in document order, the 0xffs that follow its 0x8c before
    // any other section's 0x8c — the same association the reader's walk applies. The first
    // one whose visibility slot names a formula carries the section's visibility body.
    let mut current: Option<usize> = None; // offset of the last 0x8c seen
    let mut visibility_index: Option<usize> = None;
    fn walk(
        nodes: &[RecordNode],
        current: &mut Option<usize>,
        want_offset: usize,
        logical: &[u8],
        hit: &mut Option<usize>,
    ) {
        for n in nodes {
            if n.rtype == 0x008c {
                *current = Some(n.offset);
            } else if n.rtype == 0x00ff && *current == Some(want_offset) {
                let Some(read) =
                    rpt_reader::fields::read(n, logical, rpt_reader::raw::Dialect::Contents)
                else {
                    continue;
                };
                for f in &read.fields {
                    if f.name == "visibility_formula" {
                        if let FieldValue::FieldRef { index: Some(i), .. } = &f.value {
                            *hit = Some(*i as usize);
                        }
                    }
                }
            }
            walk(&n.children, current, want_offset, logical, hit);
        }
    }
    for n in &tree {
        // Resolve the 0x8c node's offset from its (tag, nth) address: the nth 0x8c in pre-order.
        let mut seen = 0usize;
        let mut want_offset = None;
        fn find_nth(nodes: &[RecordNode], nth: usize, seen: &mut usize, hit: &mut Option<usize>) {
            for n in nodes {
                if n.rtype == 0x008c {
                    if *seen == nth {
                        *hit = Some(n.offset);
                    }
                    *seen += 1;
                }
                find_nth(&n.children, nth, seen, hit);
            }
        }
        find_nth(&tree, sec.nth, &mut seen, &mut want_offset);
        let want_offset = want_offset.ok_or("the section record could not be re-found")?;
        walk(std::slice::from_ref(n), &mut current, want_offset, logical, &mut visibility_index);
    }
    let index = visibility_index.ok_or_else(|| {
        format!(
            "section `{}` carries no Section_Visibility formula to edit (it prints by its \
             static flags alone)",
            sel.section
        )
    })?;

    crate::formula::set_formula_by_ordinal(rpt, index, source)
}


/// Edit a declared parameter's presentation: the prompt text and/or the optional-prompt
/// flag, patched on the parameter's `0x7a` record (matched by its `0x71` child's name).
///
/// # Errors
///
/// No parameter of that name; or a writer failure.
pub fn set_parameter(
    rpt: &Rpt,
    name: &str,
    prompt_text: Option<&str>,
    optional: Option<bool>,
) -> Result<Vec<u8>, String> {
    let want = name.trim().trim_start_matches('@').to_ascii_lowercase();
    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();

    let mut nth = 0usize;
    for n in tree.iter().filter(|n| n.rtype == 0x007a) {
        let named = n.children.iter().find(|c| c.rtype == 0x0071).and_then(|c| {
            rpt_reader::fields::read(c, logical, rpt_reader::raw::Dialect::Contents).and_then(|r| {
                r.fields.iter().find(|x| x.name == "name").map(|x| match &x.value {
                    rpt_reader::fields::FieldValue::Text(t) => t.clone(),
                    _ => String::new(),
                })
            })
        }).unwrap_or_default();
        if named.trim_start_matches('@').to_ascii_lowercase() == want {
            let mut current = rpt.reencode().map_err(|e| format!("reencode: {e}"))?;
            if let Some(t) = prompt_text {
                current = rpt_reader::Rpt::patch_record_field_with(
                    &crate::open_bytes(&current)?,
                    rpt_reader::raw::RecordTag(0x007a), nth, "prompt_text",
                    &rpt_reader::fields::FieldEdit::Text(t.to_string()),
                    rpt_reader::EditPolicy::Forced,
                ).map_err(|e| format!("patch prompt_text: {e}"))?;
            }
            if let Some(v) = optional {
                current = rpt_reader::Rpt::patch_record_field_with(
                    &crate::open_bytes(&current)?,
                    rpt_reader::raw::RecordTag(0x007a), nth, "optional_prompt",
                    &rpt_reader::fields::FieldEdit::Int(v as i64),
                    rpt_reader::EditPolicy::Forced,
                ).map_err(|e| format!("patch optional_prompt: {e}"))?;
            }
            return Ok(current);
        }
        nth += 1;
    }
    Err(format!("no parameter named `{name}` in this report"))
}

/// Remove the sort on a field: splice out the matching top-level `0x29` record.
///
/// # Errors
///
/// No sort on that field.
pub fn remove_sort(rpt: &Rpt, field_ref: &str) -> Result<Vec<u8>, String> {
    let display = crate::grouping::field_display(rpt.report(), field_ref);
    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();
    for (_nth, n) in tree.iter().filter(|n| n.rtype == 0x0029).enumerate() {
        if let Some(read) = rpt_reader::fields::read(n, logical, rpt_reader::raw::Dialect::Contents) {
            let field = read.fields.iter().find(|x| x.name == "field").and_then(|x| match &x.value {
                rpt_reader::fields::FieldValue::Text(t) => Some(t.clone()),
                _ => None,
            }).unwrap_or_default();
            if field.eq_ignore_ascii_case(&display) {
                let new_logical = crate::framing::splice(
                    logical, &tree, n.offset..n.content_end, b"",
                ).map_err(|e| format!("splice sort removal: {e}"))?;
                return rpt.write_contents_logical(&new_logical)
                    .map_err(|e| format!("write sort removal: {e}"));
            }
        }
    }
    Err(format!("no sort on `{field_ref}`"))
}
