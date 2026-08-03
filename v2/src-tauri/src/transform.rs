//! Declarative bulk edits to a draft: the restructuring an assistant
//! would otherwise do by writing a one-off script and re-emitting the
//! whole file.
//!
//! The problem with the script route is that every edit is bespoke, so
//! every edit is a fresh chance to drop a field, mangle a step, or lose
//! a work-item id. Here the operations are a fixed, tested vocabulary:
//! the assistant says WHAT to change, and this decides how.
//!
//! Every operation takes an optional `where` filter, so an edit can hit
//! one case, a tagged subset, or everything. Pure - no I/O, no ADO.

use crate::model::TestCase;

/// The only keys a `where` clause may carry. An unrecognised one used to be
/// ignored, which left an all-None filter - and `Filter::matches` reads that
/// as "every case", so one typo turned a targeted edit into a draft-wide
/// rewrite that reported success.
const FILTER_KEYS: [&str; 3] = ["title_contains", "has_tag", "module_is"];

/// First non-empty value among `keys`, using the file importer's own
/// lookup so the two paths cannot disagree about what a key means.
fn pick(v: &serde_json::Value, keys: &[&str]) -> String {
    crate::import_parser::json_value(v, keys)
        .map(crate::import_parser::value_to_string)
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[derive(Debug, Clone)]
pub enum Op {
    SetTags(String),
    AddTags(String),
    RemoveTags(String),
    SetModule(String),
    SetAutomationStatus(String),
    SetPreconditions(String),
    /// Literal find/replace across the title.
    ReplaceInTitle { find: String, replace: String },
    PrefixTitle(String),
    SuffixTitle(String),
    /// Literal find/replace across every step's action and expected.
    ReplaceInSteps { find: String, replace: String },
    /// Sort the whole draft: "title" | "module" | "tags" | "preconditions".
    SortBy(String),
    /// Stable grouping: cases sharing the field's value become contiguous,
    /// groups ordered by first appearance and within-group order kept -
    /// grouping that never destroys a deliberate sequence.
    GroupBy(String),
    /// Drop cases whose title repeats an earlier one.
    Dedupe,
    /// Add a step at the front of each matched case.
    PrependStep { action: String, expected: String },
    /// Add a step at the end of each matched case.
    AppendStep { action: String, expected: String },
    /// Remove steps whose action contains this text (case-insensitive) -
    /// the repair for a duplicated preamble.
    RemoveStepMatching(String),
    /// Drop the matched cases. Requires a `where` filter: an unfiltered
    /// remove would delete the whole draft, and nobody means that.
    RemoveCases,
    /// Append new cases to the draft.
    InsertCases(Vec<TestCase>),
}

/// Which cases an operation applies to. Absent means all of them.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Case-insensitive substring of the title.
    pub title_contains: Option<String>,
    /// Case-insensitive exact tag match.
    pub has_tag: Option<String>,
    /// Case-insensitive exact module match.
    pub module_is: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub op: Op,
    pub filter: Filter,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct TransformReport {
    /// One line per operation: what it did and how many cases it touched.
    pub applied: Vec<String>,
    /// Anything an operation declined to do. `applied` counts cases it
    /// touched, which said nothing about a case it deliberately left
    /// alone - and the caller reads this report instead of diffing the
    /// draft by hand.
    #[serde(default)]
    pub warnings: Vec<String>,
    pub cases_in: usize,
    pub cases_out: usize,
}

impl Filter {
    fn matches(&self, c: &TestCase) -> bool {
        if let Some(t) = &self.title_contains {
            if !c.title.to_lowercase().contains(&t.to_lowercase()) {
                return false;
            }
        }
        if let Some(tag) = &self.has_tag {
            if !split_tags(&c.tags).iter().any(|x| x.eq_ignore_ascii_case(tag)) {
                return false;
            }
        }
        if let Some(m) = &self.module_is {
            if !c.module_value.trim().eq_ignore_ascii_case(m.trim()) {
                return false;
            }
        }
        true
    }
}

fn split_tags(raw: &str) -> Vec<String> {
    raw.split([';', ','])
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

fn join_tags(tags: &[String]) -> String {
    tags.join("; ")
}

fn str_of(v: &serde_json::Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}

/// A string the caller MUST have written, distinguishing "absent" from
/// "deliberately empty".
///
/// `str_of` cannot tell those apart - both come back as "" - and every
/// set_* op then wrote that empty string over the field on every matched
/// case and reported it as applied. Clearing a field is a legitimate edit,
/// so the fix is not to reject "": it is to insist the key be present, so
/// a typo like "vlaue" fails loudly instead of blanking the draft.
fn required_str(v: &serde_json::Value, key: &str, label: &str) -> Result<String, String> {
    match v.get(key) {
        Some(serde_json::Value::String(s)) => Ok(s.to_string()),
        Some(_) => Err(format!("{label}: \"{key}\" must be a string.")),
        None => Err(format!(
            "{label}: requires a \"{key}\". Pass \"\" explicitly to clear the field."
        )),
    }
}

fn parse_filter(v: &serde_json::Value, label: &str) -> Result<Filter, String> {
    let f = &v["where"];
    if f.is_null() {
        return Ok(Filter::default()); // no clause = every case, as documented
    }
    let obj = f
        .as_object()
        .ok_or_else(|| format!("{label}: \"where\" must be an object."))?;
    for key in obj.keys() {
        if !FILTER_KEYS.contains(&key.as_str()) {
            return Err(format!(
                "{label}: \"where\" has an unknown key \"{key}\". Use one of: {}.",
                FILTER_KEYS.join(", ")
            ));
        }
    }
    for key in FILTER_KEYS {
        if obj.get(key).is_some_and(|x| !x.is_string()) {
            return Err(format!("{label}: \"where.{key}\" must be a string."));
        }
    }
    Ok(Filter {
        title_contains: f["title_contains"].as_str().map(str::to_string),
        has_tag: f["has_tag"].as_str().map(str::to_string),
        module_is: f["module_is"].as_str().map(str::to_string),
    })
}

/// Read the `operations` array an assistant sends. Errors name the
/// offending entry rather than silently skipping it - a dropped edit that
/// looks applied is the worst outcome here.
pub fn parse_ops(raw: &serde_json::Value) -> Result<Vec<Operation>, String> {
    let list = raw
        .as_array()
        .ok_or("\"operations\" must be a list of operation objects.")?;
    let mut out = vec![];
    for (i, v) in list.iter().enumerate() {
        let label = format!("operation {}", i + 1);
        let name = v["op"].as_str().ok_or(format!("{label}: missing \"op\"."))?;
        // Only read for the ops that take one; required_str below is what
        // actually guards them.
        let value = str_of(v, "value");
        let op = match name {
            "set_tags" => Op::SetTags(required_str(v, "value", &label)?),
            "add_tags" => Op::AddTags(required_str(v, "value", &label)?),
            "remove_tags" => Op::RemoveTags(required_str(v, "value", &label)?),
            "set_module" => Op::SetModule(required_str(v, "value", &label)?),
            "set_automation_status" => {
                if value != "Not Automated" && value != "Planned" {
                    return Err(format!(
                        "{label}: automation status must be \"Not Automated\" or \"Planned\"."
                    ));
                }
                Op::SetAutomationStatus(value)
            }
            "set_preconditions" => Op::SetPreconditions(required_str(v, "value", &label)?),
            "replace_in_title" => Op::ReplaceInTitle {
                find: str_of(v, "find"),
                replace: str_of(v, "replace"),
            },
            "prefix_title" => Op::PrefixTitle(value),
            "suffix_title" => Op::SuffixTitle(value),
            "replace_in_steps" => Op::ReplaceInSteps {
                find: str_of(v, "find"),
                replace: str_of(v, "replace"),
            },
            "sort_by" | "group_by" => {
                if !["title", "module", "tags", "preconditions"].contains(&value.as_str()) {
                    return Err(format!(
                        "{label}: {name} takes \"title\", \"module\", \"tags\" or \"preconditions\"."
                    ));
                }
                if name == "sort_by" { Op::SortBy(value) } else { Op::GroupBy(value) }
            }
            "dedupe" => Op::Dedupe,
            "prepend_step" | "append_step" => {
                let action = str_of(v, "action");
                if action.trim().is_empty() {
                    return Err(format!("{label}: {name} needs a non-empty \"action\"."));
                }
                let expected = str_of(v, "expected");
                if name == "prepend_step" {
                    Op::PrependStep { action, expected }
                } else {
                    Op::AppendStep { action, expected }
                }
            }
            "remove_step_matching" => {
                let find = if value.is_empty() { str_of(v, "find") } else { value.clone() };
                if find.trim().is_empty() {
                    return Err(format!("{label}: remove_step_matching needs the text to match."));
                }
                Op::RemoveStepMatching(find)
            }
            "remove_cases" => {
                let f = &v["where"];
                let has_filter = f["title_contains"].as_str().is_some()
                    || f["has_tag"].as_str().is_some()
                    || f["module_is"].as_str().is_some();
                if !has_filter {
                    return Err(format!(
                        "{label}: remove_cases requires a \"where\" filter - an unfiltered remove would delete every case."
                    ));
                }
                Op::RemoveCases
            }
            "insert_cases" => {
                // Lenient, like the importer: only a title is required,
                // and "module" is what draft JSON actually calls the
                // field. Strict serde would reject every real draft.
                let raw = v["cases"]
                    .as_array()
                    .ok_or(format!("{label}: \"cases\" must be a list of test-case objects."))?;
                let mut cases = vec![];
                for (j, rv) in raw.iter().enumerate() {
                    let title = rv["title"].as_str().unwrap_or("").trim().to_string();
                    if title.is_empty() {
                        return Err(format!("{label}: cases[{j}] has no title."));
                    }
                    let steps = rv["steps"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|sv| crate::steps_xml::Step {
                                    action: sv["action"].as_str().unwrap_or("").to_string(),
                                    expected: sv["expected"].as_str().unwrap_or("").to_string(),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    cases.push(TestCase {
                        title,
                        steps,
                        tags: rv["tags"].as_str().unwrap_or("").to_string(),
                        automation_status: rv["automation_status"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or("Not Automated")
                            .to_string(),
                        module_value: rv["module"]
                            .as_str()
                            .or_else(|| rv["module_value"].as_str())
                            .unwrap_or("")
                            .to_string(),
                        preconditions: rv["preconditions"].as_str().unwrap_or("").to_string(),
                        update_id: rv["id"]
                            .as_i64()
                            .or_else(|| rv["update_id"].as_i64())
                            .map(|n| n as i32),
                        // The file importer's own alias lists, not a copy of
                        // them: the copy that used to live here was already
                        // one spelling short, and a case inserted by an
                        // assistant just lost its review context silently.
                        comment: pick(rv, &crate::import_parser::COMMENT_KEYS),
                        reviewer_notes: pick(rv, &crate::import_parser::REVIEWER_NOTES_KEYS),
                        // A case inserted mid-draft has no position in
                        // either reading yet; the next optimize stamps both.
                        spec_order: None,
                        tester_order: None,
                    });
                }
                if cases.is_empty() {
                    return Err(format!("{label}: insert_cases got an empty \"cases\" list."));
                }
                Op::InsertCases(cases)
            }
            other => {
                return Err(format!(
                    "{label}: unknown op \"{other}\". Supported: set_tags, add_tags, \
                     remove_tags, set_module, set_automation_status, set_preconditions, \
                     replace_in_title, prefix_title, suffix_title, replace_in_steps, \
                     prepend_step, append_step, remove_step_matching, sort_by, group_by, \
                     dedupe, remove_cases, insert_cases."
                ))
            }
        };
        // A find/replace with an empty needle would splice the replacement
        // between every character - refuse rather than mangle the draft.
        if let Op::ReplaceInTitle { find, .. } | Op::ReplaceInSteps { find, .. } = &op {
            if find.is_empty() {
                return Err(format!("{label}: \"find\" must not be empty."));
            }
        }
        out.push(Operation {
            op,
            filter: parse_filter(v, &label)?,
        });
    }
    Ok(out)
}

pub fn apply(cases: Vec<TestCase>, ops: &[Operation]) -> (Vec<TestCase>, TransformReport) {
    let mut report = TransformReport {
        cases_in: cases.len(),
        ..Default::default()
    };
    let mut cases = cases;

    for operation in ops {
        let mut touched = 0usize;
        match &operation.op {
            // Whole-list operations ignore the filter by nature.
            Op::SortBy(key) => {
                cases.sort_by_key(|c| field_key(c, key));
                touched = cases.len();
                report.applied.push(format!("Sorted {} cases by {key}.", cases.len()));
            }
            Op::GroupBy(key) => {
                // Stable: groups appear in first-encounter order, and the
                // order WITHIN each group is exactly the input order.
                let mut buckets: Vec<(String, Vec<TestCase>)> = vec![];
                for c in cases.drain(..) {
                    let k = field_key(&c, key);
                    match buckets.iter_mut().find(|(bk, _)| *bk == k) {
                        Some((_, list)) => list.push(c),
                        None => buckets.push((k, vec![c])),
                    }
                }
                let groups = buckets.len();
                cases = buckets.into_iter().flat_map(|(_, list)| list).collect();
                touched = cases.len();
                report
                    .applied
                    .push(format!("Grouped {} cases by {key} into {groups} group(s).", cases.len()));
            }
            Op::RemoveCases => {
                let before = cases.len();
                cases.retain(|c| !operation.filter.matches(c));
                touched = before - cases.len();
                report.applied.push(format!("Removed {touched} case(s)."));
            }
            Op::InsertCases(new_cases) => {
                touched = new_cases.len();
                cases.extend(new_cases.iter().cloned());
                report.applied.push(format!("Inserted {touched} case(s) at the end."));
            }
            Op::Dedupe => {
                let before = cases.len();
                let mut seen: Vec<String> = vec![];
                cases.retain(|c| {
                    let k = c.title.trim().to_lowercase();
                    if seen.contains(&k) {
                        false
                    } else {
                        seen.push(k);
                        true
                    }
                });
                touched = before - cases.len();
                report.applied.push(format!("Removed {touched} duplicate case(s)."));
            }
            other => {
                for c in cases.iter_mut() {
                    if !operation.filter.matches(c) {
                        continue;
                    }
                    touched += 1;
                    match other {
                        Op::SetTags(v) => c.tags = join_tags(&split_tags(v)),
                        Op::AddTags(v) => {
                            let mut tags = split_tags(&c.tags);
                            for t in split_tags(v) {
                                if !tags.iter().any(|x| x.eq_ignore_ascii_case(&t)) {
                                    tags.push(t);
                                }
                            }
                            c.tags = join_tags(&tags);
                        }
                        Op::RemoveTags(v) => {
                            let drop = split_tags(v);
                            let tags: Vec<String> = split_tags(&c.tags)
                                .into_iter()
                                .filter(|t| !drop.iter().any(|d| d.eq_ignore_ascii_case(t)))
                                .collect();
                            c.tags = join_tags(&tags);
                        }
                        Op::SetModule(v) => c.module_value = v.clone(),
                        Op::SetAutomationStatus(v) => c.automation_status = v.clone(),
                        Op::SetPreconditions(v) => c.preconditions = v.clone(),
                        Op::ReplaceInTitle { find, replace } => {
                            c.title = c.title.replace(find.as_str(), replace);
                        }
                        Op::PrefixTitle(v) => c.title = format!("{v}{}", c.title),
                        Op::SuffixTitle(v) => c.title = format!("{}{v}", c.title),
                        Op::ReplaceInSteps { find, replace } => {
                            for s in c.steps.iter_mut() {
                                s.action = s.action.replace(find.as_str(), replace);
                                s.expected = s.expected.replace(find.as_str(), replace);
                            }
                        }
                        Op::PrependStep { action, expected } => {
                            c.steps.insert(
                                0,
                                crate::steps_xml::Step {
                                    action: action.clone(),
                                    expected: expected.clone(),
                                },
                            );
                        }
                        Op::AppendStep { action, expected } => {
                            c.steps.push(crate::steps_xml::Step {
                                action: action.clone(),
                                expected: expected.clone(),
                            });
                        }
                        Op::RemoveStepMatching(find) => {
                            let needle = find.to_lowercase();
                            let kept: Vec<_> = c
                                .steps
                                .iter()
                                .filter(|s| !s.action.to_lowercase().contains(&needle))
                                .cloned()
                                .collect();
                            // A case with no steps does not survive the
                            // importer - it is skipped - so emptying one
                            // here deleted it from the draft while the
                            // report said the operation had been applied.
                            // The same rule the writer already keeps: never
                            // let an empty step list stand in for a real one.
                            if kept.is_empty() {
                                report.warnings.push(format!(
                                    "'{}': every step matches '{find}', and a case with no steps \
                                     cannot be imported - its steps were left alone.",
                                    c.title
                                ));
                            } else {
                                c.steps = kept;
                            }
                        }
                        Op::SortBy(_)
                        | Op::GroupBy(_)
                        | Op::Dedupe
                        | Op::RemoveCases
                        | Op::InsertCases(_) => unreachable!("handled above"),
                    }
                }
                report
                    .applied
                    .push(format!("{} applied to {touched} case(s).", describe(other)));
            }
        }
        if touched == 0 {
            report
                .applied
                .push("  (nothing matched that filter - check it)".to_string());
        }
    }

    report.cases_out = cases.len();
    (cases, report)
}

/// The sortable/groupable value of one field, lowercased for stability.
fn field_key(c: &TestCase, key: &str) -> String {
    match key {
        "module" => c.module_value.to_lowercase(),
        "tags" => c.tags.to_lowercase(),
        "preconditions" => c.preconditions.to_lowercase(),
        _ => c.title.to_lowercase(),
    }
}

fn describe(op: &Op) -> String {
    match op {
        Op::SetTags(v) => format!("Set tags to '{v}'"),
        Op::AddTags(v) => format!("Added tags '{v}'"),
        Op::RemoveTags(v) => format!("Removed tags '{v}'"),
        Op::SetModule(v) => format!("Set module to '{v}'"),
        Op::SetAutomationStatus(v) => format!("Set automation status to '{v}'"),
        Op::SetPreconditions(v) => format!("Set preconditions to '{v}'"),
        Op::ReplaceInTitle { find, replace } => format!("Replaced '{find}' with '{replace}' in titles"),
        Op::PrefixTitle(v) => format!("Prefixed titles with '{v}'"),
        Op::SuffixTitle(v) => format!("Suffixed titles with '{v}'"),
        Op::ReplaceInSteps { find, replace } => format!("Replaced '{find}' with '{replace}' in steps"),
        Op::SortBy(k) => format!("Sorted by {k}"),
        Op::GroupBy(k) => format!("Grouped by {k}"),
        Op::Dedupe => "Deduped".to_string(),
        Op::PrependStep { action, .. } => format!("Prepended step '{action}'"),
        Op::AppendStep { action, .. } => format!("Appended step '{action}'"),
        Op::RemoveStepMatching(f) => format!("Removed steps matching '{f}'"),
        Op::RemoveCases => "Removed cases".to_string(),
        Op::InsertCases(list) => format!("Inserted {} case(s)", list.len()),
    }
}
