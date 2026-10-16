//! Formula source editing — `setFormula` / `setRecordSelection`.
//!
//! The format stores NO token bytecode: a `0x0077[0x0076]` formula record is its `0x0071`
//! name child + a reference list + the source text + kind words, nothing else (verified
//! across every 0x76 in the 128-file corpus and all fixtures — the engine recompiles from
//! source at load, as the vendored model's own docs state). Editing a formula is therefore
//! an ordinary rewrite of the text and the reference list, not a compiler problem.
//!
//! The reference list is DERIVED from the new source: every `{…}` group resolves through the
//! report's field pool — `{alias.field}` → database (pool 0), `{?name}` → parameter (pool 5,
//! written `?name`), `{@name}` → formula (pool 1), `Group #N` → group value (pool 4), special
//! display names → special pool (pool 3). The pool-kind vocabulary is pinned empirically from
//! donor formulas across the fixtures (validation-log §23); running-total `{#…}` and
//! SQL-expression `{%…}` references have no donor evidence for their wire kind and are
//! REFUSED naming the reference (fails closed).
//!
//! The rewrite preserves everything it does not own: the `0x71` child (name/result
//! type/length — engine-recomputed facts we keep as stored) and the whole tail after the text
//! (kind words, the seven fixed words, syntax/null-treatment optionals) are copied verbatim,
//! so only refcount + refs + text change.

use rpt_reader::fields::FieldValue;
use rpt_reader::model;
use rpt_reader::Rpt;
use rpt_reader::StreamId;

use crate::framing;

/// One derived reference: display name, wire pool kind, pool ordinal.
pub type DerivedRef = (String, u16, u16);

/// Every `{…}` group in `source`, resolved through the report's field pool. First appearance
/// order, deduplicated. The wire names use the report's own canonical forms.
///
/// # Errors
///
/// Names the first reference that cannot resolve (or whose kind has no wire evidence).
pub fn derive_refs(report: &model::Report, source: &str) -> Result<Vec<DerivedRef>, String> {
    let mut out: Vec<DerivedRef> = Vec::new();
    let defs: Vec<&model::FieldDef> = report.data_definition.field_definitions.iter().collect();
    for group in rpt_formula::token::brace_groups(source) {
        let inner = group.trim_start_matches('{').trim_end_matches('}');
        let debraced = inner.trim();
        let resolved: Option<DerivedRef> = if let Some(name) = debraced.strip_prefix('?') {
            let want = name.trim_start_matches('@').to_ascii_lowercase();
            let mut hit = None;
            let mut n = 0u16;
            for f in &defs {
                if let model::FieldKindData::Parameter(_) = &f.kind {
                    if f.name.trim_start_matches('@').to_ascii_lowercase() == want {
                        hit = Some((format!("?{}", f.name.trim_start_matches('@')), 5u16, n));
                        break;
                    }
                    n = n.saturating_add(1);
                }
            }
            hit
        } else if let Some(name) = debraced.strip_prefix('@') {
            let want = name.trim().to_ascii_lowercase();
            let mut hit = None;
            let mut n = 0u16;
            for f in &defs {
                if let model::FieldKindData::Formula(_) = &f.kind {
                    if f.name.trim_start_matches('@').to_ascii_lowercase() == want {
                        hit = Some((f.name.trim_start_matches('@').to_string(), 1u16, n));
                        break;
                    }
                    n = n.saturating_add(1);
                }
            }
            hit
        } else if debraced.starts_with('#') {
            return Err(format!(
                "`{{{debraced}}}` is a running-total reference; no donor evidence pins its wire \
                 pool kind in formula reference lists yet (refused rather than guessed)"
            ));
        } else if debraced.starts_with('%') {
            return Err(format!(
                "`{{{debraced}}}` is a SQL-expression reference; no donor evidence pins its wire \
                 pool kind in formula reference lists yet (refused rather than guessed)"
            ));
        } else if let Some(n) = group_number_of(debraced) {
            Some((format!("Group #{n}"), 4u16, n.saturating_sub(1).min(u16::MAX as usize) as u16))
        } else {
            // Special display name, else a database field. Specials match on the definition's
            // own name; database fields on the bare name (`alias.field` → `field`).
            let want = debraced.to_ascii_lowercase();
            let mut sp_hit = None;
            let mut sp_n = 0u16;
            let mut db_hit = None;
            let mut db_n = 0u16;
            for f in &defs {
                match &f.kind {
                    model::FieldKindData::Special(_) => {
                        if f.name.to_ascii_lowercase() == want {
                            sp_hit = Some((f.name.clone(), 3u16, sp_n));
                        }
                        sp_n = sp_n.saturating_add(1);
                    }
                    model::FieldKindData::Database(db) => {
                        let bare = f.name.rsplit('.').next().unwrap_or(&f.name).to_ascii_lowercase();
                        if bare == want.rsplit('.').next().unwrap_or(&want) && db_hit.is_none() {
                            let alias = if db.table_alias.is_empty() {
                                String::new()
                            } else {
                                format!("{}.", db.table_alias)
                            };
                            db_hit = Some((format!("{alias}{}", f.name), 0u16, db_n));
                        }
                        db_n = db_n.saturating_add(1);
                    }
                    _ => {}
                }
            }
            sp_hit.or(db_hit)
        };
        let Some(r) = resolved else {
            return Err(format!(
                "`{group}` does not resolve to a field of this report (database field, parameter, \
                 formula, special, or `Group #N`)"
            ));
        };
        if !out.iter().any(|(_, k, i)| *k == r.1 && *i == r.2) {
            out.push(r);
        }
    }
    Ok(out)
}

/// Build a complete `0x0077[0x0076[0x0071 …] refs text tail]` formula run from decoded field
/// pieces — the shared synth path behind `set_formula`'s rebuild and the group-order mint.
pub(crate) fn build_formula_run(
    child_fields: Vec<rpt_reader::fields::FieldRead>,
    refs: &[DerivedRef],
    text: &str,
    tail_fields: &[rpt_reader::fields::FieldRead],
) -> Result<Vec<u8>, String> {
    let mut child_body = Vec::new();
    for fl in &child_fields {
        encode_field(fl, &mut child_body)?;
    }
    let mut body = framing::build_record(0x0071, 0x0700, &child_body);
    body.extend_from_slice(&(refs.len() as u16).to_be_bytes());
    for (name, kind, index) in refs {
        body.extend(framing::build_string(name));
        body.extend(framing::build_narrowing(*kind));
        body.extend_from_slice(&index.to_be_bytes());
    }
    body.extend(framing::build_string(text));
    let mut tail = Vec::new();
    for fl in tail_fields {
        encode_field(fl, &mut tail)?;
    }
    body.extend_from_slice(&tail);
    let formula = framing::build_record(0x0076, 0x0700, &body);
    Ok(framing::build_record(0x0077, 0x0700, &formula))
}

/// Re-emit one decoded scalar/fieldref field by its wire kind (for rebuilding a record from
/// its read). Only the kinds a formula/NamedValue tail can carry are handled.
fn encode_field(fl: &rpt_reader::fields::FieldRead, out: &mut Vec<u8>) -> Result<(), String> {
    use rpt_reader::fields::{FieldKind, FieldValue};
    match (&fl.kind, &fl.value) {
        (FieldKind::Text, FieldValue::Text(t)) => out.extend(framing::build_string(t)),
        (FieldKind::VarU16, FieldValue::Uint(v)) => out.extend(framing::build_narrowing(u16::try_from(*v).unwrap_or(0))),
        (FieldKind::U16Be, FieldValue::Uint(v)) => {
            out.extend_from_slice(&u16::try_from(*v).unwrap_or(0).to_be_bytes())
        }
        (FieldKind::I16Be, FieldValue::Int(v)) => out.extend_from_slice(&v.to_be_bytes()),
        (FieldKind::I32Be, FieldValue::Int(v)) => out.extend_from_slice(&v.to_be_bytes()),
        (FieldKind::Bool, FieldValue::Uint(v)) => out.push(*v as u8),
        (FieldKind::FieldRef, FieldValue::FieldRef { name, kind, index }) => {
            out.extend(framing::build_string(name));
            out.extend(framing::build_narrowing(u16::try_from(*kind).unwrap_or(0)));
            out.extend_from_slice(&index.unwrap_or(0).to_be_bytes());
        }
        _ => {
            return Err(format!(
                "the formula record's `{}` field is a kind this rewriter does not re-emit",
                fl.name
            ))
        }
    }
    Ok(())
}

/// `Group #3` (case-insensitive) → `Some(3)`.
fn group_number_of(s: &str) -> Option<usize> {
    let lower = s.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix("group #")?;
    if let Some(_name) = rest.strip_suffix(" name") {
        return None; // `Group #N Name` is the group-NAME field, a different reference
    }
    rest.parse().ok()
}

/// Replace a named user formula's source text (and derive its reference list).
///
/// # Errors
///
/// No formula of that name; an unresolvable reference in the new source; or a writer failure.
pub fn set_formula(rpt: &Rpt, target: &str, source: &str) -> Result<Vec<u8>, String> {
    let want = target.trim().trim_start_matches('@').to_ascii_lowercase();
    rewrite_formula(rpt, source, |name, kind| {
        kind == 0 && name.trim_start_matches('@').to_ascii_lowercase() == want
    })
    .and_then(|r| r.ok_or_else(|| format!("no formula named `{target}` in this report")))
}

/// Replace the report's Record Selection formula source.
///
/// # Errors
///
/// The report carries no Record Selection formula (creating one is a separate splice); an
/// unresolvable reference; or a writer failure.
pub fn set_record_selection(rpt: &Rpt, source: &str) -> Result<Vec<u8>, String> {
    rewrite_formula(rpt, source, |_name, kind| kind == 1)
        .and_then(|r| r.ok_or_else(|| "this report has no Record Selection formula to edit".to_string()))
}

/// Shared rewrite: find the `0x0077` wrapper whose formula matches, rebuild its `0x0076`
/// (same `0x71` child bytes, same tail after `text`; new refcount + refs + text), splice.
fn rewrite_formula(
    rpt: &Rpt,
    source: &str,
    matches: impl Fn(&str, u32) -> bool,
) -> Result<Option<Vec<u8>>, String> {
    let report = rpt.report();
    let stream = rpt.stream(&StreamId::Contents).ok_or("no Contents stream")?;
    let logical = stream.logical_bytes();
    let tree = stream.record_tree();

    for w in tree.iter().filter(|n| n.rtype == 0x0077) {
        let Some(f) = w.children.iter().find(|c| c.rtype == 0x0076) else { continue };
        let read = rpt_reader::fields::read(f, logical, rpt_reader::raw::Dialect::Contents)
            .ok_or("the formula record's field table refused it")?;
        let kind = u32::try_from(
            read.fields
                .iter()
                .find(|x| x.name == "formula_kind")
                .and_then(|x| match x.value {
                    FieldValue::Uint(v) => Some(v),
                    _ => None,
                })
                .unwrap_or(0),
        )
        .unwrap_or(0);
        // The 0x71 child's name.
        let Some(child) = f.children.iter().find(|c| c.rtype == 0x0071) else { continue };
        let child_read =
            rpt_reader::fields::read(child, logical, rpt_reader::raw::Dialect::Contents)
                .ok_or("the formula's name record refused to read")?;
        let name = child_read
            .fields
            .iter()
            .find(|x| x.name == "name")
            .and_then(|x| match &x.value {
                FieldValue::Text(t) => Some(t.clone()),
                _ => None,
            })
            .unwrap_or_default();
        if !matches(&name, kind) {
            continue;
        }

        let refs = derive_refs(&report, source)?;

        // Rebuild: the 0x71 child and the tail after `text` are RE-EMITTED from their decoded
        // field values — copied stored bytes carry position masks a fresh build cannot reuse
        // (the child's run is not uniformly maskable; the sub-side builder proved the
        // rebuild-from-values recipe).
        let mut child_body = Vec::new();
        for fl in &child_read.fields {
            encode_field(fl, &mut child_body)?;
        }
        let child = framing::build_record(0x0071, 0x0700, &child_body);
        let mut tail = Vec::new();
        for fl in read.fields.iter().skip_while(|x| x.path != "text").skip(1) {
            encode_field(fl, &mut tail)?;
        }
        let mut body = child;
        body.extend_from_slice(&(refs.len() as u16).to_be_bytes());
        for (name, kind, index) in &refs {
            body.extend(framing::build_string(name));
            body.extend(framing::build_narrowing(*kind));
            body.extend_from_slice(&index.to_be_bytes());
        }
        body.extend(framing::build_string(source));
        body.extend_from_slice(&tail);
        let formula = framing::build_record(0x0076, 0x0700, &body);
        let run = framing::build_record(0x0077, 0x0700, &formula);
        let new_logical = framing::splice(&logical, &tree, w.offset..w.content_end, &run)
            .map_err(|e| format!("splice formula: {e}"))?;
        return rpt
            .write_contents_logical(&new_logical)
            .map(Some)
            .map_err(|e| format!("write formula: {e}"));
    }
    Ok(None)
}
