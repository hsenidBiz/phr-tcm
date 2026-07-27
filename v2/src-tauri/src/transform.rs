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
    /// Sort the whole draft: "title" | "module" | "tags".
    SortBy(String),
    /// Drop cases whose title repeats an earlier one.
    Dedupe,
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
        let value = str_of(v, "value");
        let op = match name {
            "set_tags" => Op::SetTags(value),
            "add_tags" => Op::AddTags(value),
            "remove_tags" => Op::RemoveTags(value),
            "set_module" => Op::SetModule(value),
            "set_automation_status" => {
                if value != "Not Automated" && value != "Planned" {
                    return Err(format!(
                        "{label}: automation status must be \"Not Automated\" or \"Planned\"."
                    ));
                }
                Op::SetAutomationStatus(value)
            }
            "set_preconditions" => Op::SetPreconditions(value),
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
            "sort_by" => {
                if !["title", "module", "tags"].contains(&value.as_str()) {
                    return Err(format!("{label}: sort_by takes \"title\", \"module\" or \"tags\"."));
                }
                Op::SortBy(value)
            }
            "dedupe" => Op::Dedupe,
            other => {
                return Err(format!(
                    "{label}: unknown op \"{other}\". Supported: set_tags, add_tags, \
                     remove_tags, set_module, set_automation_status, set_preconditions, \
                     replace_in_title, prefix_title, suffix_title, replace_in_steps, \
                     sort_by, dedupe."
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
        let f = &v["where"];
        out.push(Operation {
            op,
            filter: Filter {
                title_contains: f["title_contains"].as_str().map(str::to_string),
                has_tag: f["has_tag"].as_str().map(str::to_string),
                module_is: f["module_is"].as_str().map(str::to_string),
            },
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
                cases.sort_by_key(|c| match key.as_str() {
                    "module" => c.module_value.to_lowercase(),
                    "tags" => c.tags.to_lowercase(),
                    _ => c.title.to_lowercase(),
                });
                touched = cases.len();
                report.applied.push(format!("Sorted {} cases by {key}.", cases.len()));
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
                        Op::SortBy(_) | Op::Dedupe => unreachable!("handled above"),
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
        Op::Dedupe => "Deduped".to_string(),
    }
}
