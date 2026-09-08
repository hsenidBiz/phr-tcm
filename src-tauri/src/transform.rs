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
const FILTER_KEYS: [&str; 4] = ["title_contains", "has_tag", "module_is", "at_index"];

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
    /// Overwrite the local reviewer notes (never sent to Azure DevOps).
    SetReviewerNotes(String),
    /// Literal find/replace across the title.
    ReplaceInTitle { find: String, replace: String },
    /// Literal find/replace across the local reviewer notes - the bulk
    /// repair for what check_spec_coverage finds there (round 6 §1: 347
    /// findings, every one a literal replacement, and no op could reach
    /// the field).
    ReplaceInNotes { find: String, replace: String },
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
    /// Insert new cases at a chosen position. Position matters because a
    /// draft's array order IS its spec order - the one reading no field
    /// encodes - and an insert that always appended silently destroyed it
    /// once per edited case (round 5 §13).
    InsertCases { cases: Vec<TestCase>, position: InsertPos },
    /// Replace each step whose action contains `find` with the `into`
    /// sequence, at the same index.
    SplitStep { find: String, into: Vec<crate::steps_xml::Step> },
    /// Round 8 §7.2 - see normalise_citation_notes.
    NormaliseCitations,
}

/// Where `insert_cases` puts its cases.
#[derive(Debug, Clone, PartialEq)]
pub enum InsertPos {
    End,
    /// Zero-based index into the case array, clamped to the end.
    Index(usize),
    /// Before the first case whose title contains this (case-insensitive).
    Before(String),
    /// After the first case whose title contains this (case-insensitive).
    After(String),
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
    /// Zero-based position in the draft as it stands when the op runs -
    /// the same numbering `insert_cases`' at_index uses. The selector of
    /// last resort: two cases whose titles converged (a fan-out hazard,
    /// round 6 §5) match identically on every text filter, and this is
    /// the only way to address exactly one of them.
    pub at_index: Option<usize>,
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
    /// Everything the tool was GIVEN and did not use: unknown operation
    /// keys, unknown fields on inserted cases, unknown body arguments.
    /// Round 5 §15's rule - "echo what you ignored" - because a tool that
    /// silently drops input is the hand-rolled-script hazard one layer
    /// down, and harder to notice because the report looks clean.
    #[serde(default)]
    pub ignored: Vec<String>,
    pub cases_in: usize,
    pub cases_out: usize,
}

impl Filter {
    /// `i` is the case's position in the draft as this op sees it - the
    /// numbering the report and `insert_cases`' at_index already use.
    fn matches_at(&self, c: &TestCase, i: usize) -> bool {
        if let Some(want) = self.at_index {
            if i != want {
                return false;
            }
        }
        self.matches(c)
    }

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
    for key in ["title_contains", "has_tag", "module_is"] {
        if obj.get(key).is_some_and(|x| !x.is_string()) {
            return Err(format!("{label}: \"where.{key}\" must be a string."));
        }
    }
    let at_index = match obj.get("at_index") {
        None => None,
        Some(v) => Some(v.as_u64().ok_or_else(|| {
            format!("{label}: \"where.at_index\" must be a non-negative integer.")
        })? as usize),
    };
    Ok(Filter {
        title_contains: f["title_contains"].as_str().map(str::to_string),
        has_tag: f["has_tag"].as_str().map(str::to_string),
        module_is: f["module_is"].as_str().map(str::to_string),
        at_index,
    })
}

/// Read the `operations` array an assistant sends. Errors name the
/// offending entry rather than silently skipping it - a dropped edit that
/// looks applied is the worst outcome here.
pub fn parse_ops(raw: &serde_json::Value) -> Result<Vec<Operation>, String> {
    parse_ops_full(raw).map(|(ops, _)| ops)
}

/// As `parse_ops`, also returning what was IGNORED: keys an op does not
/// read, and fields `insert_cases` will discard. The route folds these
/// into the report's `ignored` list - §15's "echo what you ignored".
pub fn parse_ops_full(
    raw: &serde_json::Value,
) -> Result<(Vec<Operation>, Vec<String>), String> {
    let list = raw
        .as_array()
        .ok_or("\"operations\" must be a list of operation objects.")?;
    let mut out = vec![];
    let mut ignored: Vec<String> = vec![];
    for (i, v) in list.iter().enumerate() {
        let label = format!("operation {}", i + 1);
        if let (Some(obj), Some(name)) = (v.as_object(), v["op"].as_str()) {
            for k in obj.keys() {
                if !known_keys(name).contains(&k.as_str()) {
                    ignored.push(format!(
                        "{label}: \"{k}\" is not read by {name} - it was ignored. {name} reads: {}.",
                        known_keys(name).join(", ")
                    ));
                }
            }
        }
        if v["op"].as_str() == Some("insert_cases") {
            if let Some(list) = v["cases"].as_array() {
                for (j, rv) in list.iter().enumerate() {
                    let unknown = insert_case_unknown_keys(rv);
                    if !unknown.is_empty() {
                        ignored.push(format!(
                            "{label}: cases[{j}] carries fields the draft format does not keep - \
                             discarded: {}.",
                            unknown.join(", ")
                        ));
                    }
                }
            }
        }
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
            "set_reviewer_notes" => Op::SetReviewerNotes(required_str(v, "value", &label)?),
            "replace_in_title" => Op::ReplaceInTitle {
                find: str_of(v, "find"),
                replace: str_of(v, "replace"),
            },
            "replace_in_notes" => Op::ReplaceInNotes {
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
            "normalise_citations" => Op::NormaliseCitations,
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
                // `action` is accepted as a third alias because the schema's
                // own field list makes it LOOK right (prepend/append use it)
                // - round 5 §11 walked into exactly that trap. The error
                // names every accepted key, the way the unknown-op error
                // already names every op.
                let find = [value.clone(), str_of(v, "find"), str_of(v, "action")]
                    .into_iter()
                    .find(|s| !s.trim().is_empty())
                    .unwrap_or_default();
                if find.trim().is_empty() {
                    return Err(format!(
                        "{label}: remove_step_matching needs \"value\", \"find\" or \"action\" - \
                         the text to match against step actions."
                    ));
                }
                Op::RemoveStepMatching(find)
            }
            "split_step" => {
                // The commonest edit when tightening steps toward one action
                // each - round 5 §13 rebuilt 18 cases with remove+prepend
                // chains for want of this. Every step whose action contains
                // `find` is replaced, in place, by the `into` sequence.
                let find = if value.is_empty() { str_of(v, "find") } else { value.clone() };
                if find.trim().is_empty() {
                    return Err(format!(
                        "{label}: split_step needs \"value\" or \"find\" - the step text to split."
                    ));
                }
                let into_raw = v["into"]
                    .as_array()
                    .ok_or(format!(
                        "{label}: split_step needs \"into\" - a list of {{action, expected}} \
                         steps that replace the matched step."
                    ))?;
                let mut into = vec![];
                for (j, sv) in into_raw.iter().enumerate() {
                    let action = sv["action"].as_str().unwrap_or("").to_string();
                    if action.trim().is_empty() {
                        return Err(format!("{label}: into[{j}] has no \"action\"."));
                    }
                    into.push(crate::steps_xml::Step {
                        action,
                        expected: sv["expected"].as_str().unwrap_or("").to_string(),
                    });
                }
                if into.is_empty() {
                    return Err(format!("{label}: split_step got an empty \"into\" list."));
                }
                Op::SplitStep { find, into }
            }
            "remove_cases" => {
                let f = &v["where"];
                let has_filter = f["title_contains"].as_str().is_some()
                    || f["has_tag"].as_str().is_some()
                    || f["module_is"].as_str().is_some()
                    || f["at_index"].as_u64().is_some();
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
                // `index`/`at`/`position` used to be accepted and silently
                // dropped - the §15 defect class. Now `at_index`/`before`/
                // `after` are real, exactly one may be given, and the old
                // spellings are named in the error rather than swallowed.
                let mut positions: Vec<InsertPos> = vec![];
                if let Some(n) = v["at_index"].as_u64() {
                    positions.push(InsertPos::Index(n as usize));
                }
                if let Some(t) = v["before"].as_str().filter(|t| !t.trim().is_empty()) {
                    positions.push(InsertPos::Before(t.to_string()));
                }
                if let Some(t) = v["after"].as_str().filter(|t| !t.trim().is_empty()) {
                    positions.push(InsertPos::After(t.to_string()));
                }
                if positions.len() > 1 {
                    return Err(format!(
                        "{label}: insert_cases takes at most ONE of \"at_index\", \"before\", \"after\"."
                    ));
                }
                for legacy in ["index", "at", "position"] {
                    if v.get(legacy).is_some() {
                        return Err(format!(
                            "{label}: insert_cases does not read \"{legacy}\" - use \"at_index\" \
                             (zero-based), \"before\" or \"after\" (a title fragment)."
                        ));
                    }
                }
                Op::InsertCases {
                    cases,
                    position: positions.pop().unwrap_or(InsertPos::End),
                }
            }
            other => {
                return Err(format!(
                    "{label}: unknown op \"{other}\". Supported: set_tags, add_tags, \
                     remove_tags, set_module, set_automation_status, set_preconditions, \
                     set_reviewer_notes, replace_in_title, prefix_title, suffix_title, \
                     replace_in_steps, replace_in_notes, prepend_step, append_step, \
                     remove_step_matching, split_step, sort_by, group_by, dedupe, \
                     remove_cases, insert_cases, normalise_citations."
                ))
            }
        };
        // A find/replace with an empty needle would splice the replacement
        // between every character - refuse rather than mangle the draft.
        if let Op::ReplaceInTitle { find, .. }
        | Op::ReplaceInSteps { find, .. }
        | Op::ReplaceInNotes { find, .. } = &op
        {
            if find.is_empty() {
                return Err(format!("{label}: \"find\" must not be empty."));
            }
        }
        out.push(Operation {
            op,
            filter: parse_filter(v, &label)?,
        });
    }
    Ok((out, ignored))
}

/// What `normalise_citations` did to one note.
#[derive(Debug, Clone, PartialEq)]
pub enum CitationOutcome {
    Normalised,
    /// The block was a table or code: the pointer got the exemption form
    /// and the block was KEPT beneath it for the writer to fold into prose.
    Exempted(&'static str),
    Unchanged,
    /// More than one pointer or more than one block - refused, with why.
    ByHand(String),
}

/// Put a note's citation into the one shape the checker reads: prose,
/// `Spec:` line, `> "quote"`, remaining prose - in that order.
///
/// Round 8 §5: 66 notes needed exactly this, and the only route was
/// retyping every one. This is deliberately narrow - one pointer, one
/// blockquote run, or hands off - because a note with two of either has no
/// single right answer, and a wrong guess here corrupts a citation silently.
pub fn normalise_citation_notes(notes: &str) -> (String, CitationOutcome) {
    let lines: Vec<&str> = notes.lines().collect();
    let is_spec = |l: &str| l.trim_start().to_lowercase().starts_with("spec:");
    let is_block = |l: &str| l.trim_start().starts_with('>');

    let spec_lines: Vec<usize> = (0..lines.len()).filter(|&i| is_spec(lines[i])).collect();
    // Runs of consecutive blockquote lines, as (start, end-exclusive).
    let mut runs: Vec<(usize, usize)> = vec![];
    let mut i = 0;
    while i < lines.len() {
        if is_block(lines[i]) {
            let start = i;
            while i < lines.len() && is_block(lines[i]) {
                i += 1;
            }
            runs.push((start, i));
        } else {
            i += 1;
        }
    }

    if spec_lines.len() > 1 || runs.len() > 1 {
        return (
            notes.to_string(),
            CitationOutcome::ByHand(format!(
                "{} Spec lines / {} blockquotes - normalise by hand",
                spec_lines.len(),
                runs.len()
            )),
        );
    }
    let (Some(&spec_at), Some(&(run_start, run_end))) = (spec_lines.first(), runs.first()) else {
        return (notes.to_string(), CitationOutcome::Unchanged);
    };

    let spec_line = lines[spec_at].trim().to_string();
    let block_lines: Vec<&str> = lines[run_start..run_end].to_vec();
    let stripped: Vec<String> = block_lines
        .iter()
        .map(|l| l.trim_start().trim_start_matches('>').trim().to_string())
        .collect();
    let first = stripped.first().map(String::as_str).unwrap_or("");
    let joined = stripped.join(" ");

    let unquotable: Option<&'static str> = if first.starts_with('|') {
        Some("table/diagram")
    } else if looks_like_code(&joined) {
        Some("code-not-prose")
    } else {
        None
    };

    // Already in the accepted shape? The run must be the first non-blank
    // line after the pointer, and either the pointer carries an exemption
    // (block kept as is) or the run is one `> "..."` line.
    let next_nonblank = (spec_at + 1..lines.len()).find(|&j| !lines[j].trim().is_empty());
    let run_follows = next_nonblank == Some(run_start);
    let spec_exempt = spec_line.to_lowercase().contains("no quotable text");
    let one_quoted_line = block_lines.len() == 1 && first.starts_with('"') && first.ends_with('"') && first.len() >= 2;
    if run_follows && ((unquotable.is_some() && spec_exempt) || (unquotable.is_none() && one_quoted_line && !spec_exempt)) {
        return (notes.to_string(), CitationOutcome::Unchanged);
    }

    let (new_spec, new_block, outcome) = match unquotable {
        Some(why) => {
            let spec = if spec_exempt { spec_line.clone() } else { format!("{spec_line} - no quotable text ({why})") };
            (spec, block_lines.iter().map(|l| l.trim_end().to_string()).collect::<Vec<_>>(), CitationOutcome::Exempted(why))
        }
        None => {
            let text = joined.trim_matches(|c| c == '"' || c == '\u{201c}' || c == '\u{201d}').trim().to_string();
            (spec_line.clone(), vec![format!("> \"{text}\"")], CitationOutcome::Normalised)
        }
    };

    // Everything that is neither the pointer nor the run, split at the
    // earlier of the two: what came before stays before, the rest follows.
    let cut = spec_at.min(run_start);
    let mut before: Vec<&str> = vec![];
    let mut after: Vec<&str> = vec![];
    for (j, l) in lines.iter().enumerate() {
        if j == spec_at || (run_start..run_end).contains(&j) {
            continue;
        }
        if j < cut { before.push(l) } else { after.push(l) }
    }
    let trim_blank = |v: &[&str]| -> Vec<String> {
        let s = v.iter().position(|l| !l.trim().is_empty()).unwrap_or(v.len());
        let e = v.iter().rposition(|l| !l.trim().is_empty()).map(|p| p + 1).unwrap_or(s);
        v[s..e].iter().map(|l| l.trim_end().to_string()).collect()
    };
    let before = trim_blank(&before);
    let after = trim_blank(&after);

    let mut out: Vec<String> = vec![];
    if !before.is_empty() {
        out.extend(before);
        out.push(String::new());
    }
    out.push(new_spec);
    out.push(String::new());
    out.extend(new_block);
    if !after.is_empty() {
        out.push(String::new());
        out.extend(after);
    }
    (out.join("\n"), outcome)
}

/// A blockquote that is code rather than a sentence: SQL, a comment
/// marker, or a fence. A quote of code is not a quote of the requirement.
fn looks_like_code(s: &str) -> bool {
    let t = s.trim_start();
    if t.starts_with("--") || t.contains("```") {
        return true;
    }
    let first = t.split_whitespace().next().unwrap_or("").to_uppercase();
    matches!(
        first.as_str(),
        "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "CREATE" | "ALTER" | "EXEC" | "DECLARE" | "WITH"
    )
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
                // retain visits in order, so the running counter is each
                // case's position - what where.at_index addresses.
                let mut i = 0usize;
                cases.retain(|c| {
                    let keep = !operation.filter.matches_at(c, i);
                    i += 1;
                    keep
                });
                touched = before - cases.len();
                report.applied.push(format!("Removed {touched} case(s)."));
            }
            Op::InsertCases { cases: new_cases, position } => {
                touched = new_cases.len();
                // Resolved at APPLY time, against the array as it stands
                // after earlier operations. A before/after fragment that
                // matches nothing appends WITH a warning - dropping the
                // cases would lose data, and doing it silently would be
                // the §15 defect all over again.
                let at = match position {
                    InsertPos::End => cases.len(),
                    InsertPos::Index(i) => (*i).min(cases.len()),
                    InsertPos::Before(t) | InsertPos::After(t) => {
                        let needle = t.to_lowercase();
                        match cases.iter().position(|c| c.title.to_lowercase().contains(&needle)) {
                            Some(i) => {
                                if matches!(position, InsertPos::Before(_)) { i } else { i + 1 }
                            }
                            None => {
                                report.warnings.push(format!(
                                    "insert_cases: no case title contains '{t}' - inserted at \
                                     the end instead."
                                ));
                                cases.len()
                            }
                        }
                    }
                };
                let where_txt = match position {
                    InsertPos::End => "at the end".to_string(),
                    _ if at == cases.len() && !matches!(position, InsertPos::Index(_)) =>
                        "at the end".to_string(),
                    _ => format!("at index {at}"),
                };
                for (offset, c) in new_cases.iter().cloned().enumerate() {
                    cases.insert(at + offset, c);
                }
                report.applied.push(format!("Inserted {touched} case(s) {where_txt}."));
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
                // For the find-driven ops, `touched` (how many cases the
                // filter selected) says nothing about whether any TEXT
                // matched - with no filter it is always the whole draft,
                // which is no information at all (round 5 §12). `modified`
                // counts cases whose content actually changed, and that is
                // the number the report gives for those ops.
                let mut modified = 0usize;
                let (mut normalised, mut exempted, mut unchanged, mut by_hand) =
                    (0usize, 0usize, 0usize, 0usize);
                for (i, c) in cases.iter_mut().enumerate() {
                    if !operation.filter.matches_at(c, i) {
                        continue;
                    }
                    touched += 1;
                    let before_snapshot = matches!(
                        other,
                        Op::ReplaceInTitle { .. }
                            | Op::ReplaceInSteps { .. }
                            | Op::ReplaceInNotes { .. }
                            | Op::RemoveStepMatching(_)
                            | Op::SplitStep { .. }
                            | Op::NormaliseCitations
                    )
                    .then(|| c.clone());
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
                        Op::SetReviewerNotes(v) => c.reviewer_notes = v.clone(),
                        Op::ReplaceInTitle { find, replace } => {
                            c.title = c.title.replace(find.as_str(), replace);
                        }
                        Op::ReplaceInNotes { find, replace } => {
                            c.reviewer_notes = c.reviewer_notes.replace(find.as_str(), replace);
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
                        Op::SplitStep { find, into } => {
                            let needle = find.to_lowercase();
                            let mut rebuilt: Vec<crate::steps_xml::Step> = vec![];
                            for s in c.steps.drain(..) {
                                if s.action.to_lowercase().contains(&needle) {
                                    rebuilt.extend(into.iter().cloned());
                                } else {
                                    rebuilt.push(s);
                                }
                            }
                            c.steps = rebuilt;
                        }
                        Op::NormaliseCitations => {
                            let (text, outcome) = normalise_citation_notes(&c.reviewer_notes);
                            match &outcome {
                                CitationOutcome::Normalised => normalised += 1,
                                CitationOutcome::Exempted(why) => {
                                    exempted += 1;
                                    report.warnings.push(format!(
                                        "'{}': the blockquote is a {why} - the pointer now carries the \
                                         exemption and the block was kept beneath it; fold it into \
                                         prose if you would rather.",
                                        c.title
                                    ));
                                }
                                CitationOutcome::Unchanged => unchanged += 1,
                                CitationOutcome::ByHand(why) => {
                                    by_hand += 1;
                                    report.warnings.push(format!("'{}': {why}.", c.title));
                                }
                            }
                            c.reviewer_notes = text;
                        }
                        Op::SortBy(_)
                        | Op::GroupBy(_)
                        | Op::Dedupe
                        | Op::RemoveCases
                        | Op::InsertCases { .. } => unreachable!("handled above"),
                    }
                    if let Some(before) = before_snapshot {
                        if before.title != c.title
                            || before.steps != c.steps
                            || before.reviewer_notes != c.reviewer_notes
                        {
                            modified += 1;
                        }
                    }
                }
                let find_driven = matches!(
                    other,
                    Op::ReplaceInTitle { .. }
                        | Op::ReplaceInSteps { .. }
                        | Op::ReplaceInNotes { .. }
                        | Op::RemoveStepMatching(_)
                        | Op::SplitStep { .. }
                );
                if matches!(other, Op::NormaliseCitations) {
                    report.applied.push(format!(
                        "Normalised citations: {normalised} normalised, {exempted} exempted, \
                         {unchanged} unchanged, {by_hand} left for hand."
                    ));
                } else if find_driven {
                    report
                        .applied
                        .push(format!("{} modified {modified} case(s).", describe(other)));
                    // The find hit nothing anywhere it looked. Same shape as
                    // the empty-filter line below, because it is the same
                    // failure: an operation that quietly did nothing. This
                    // is what makes an order-dependent find - one op
                    // rewriting the text a later op looks for - visible.
                    if touched > 0 && modified == 0 {
                        report
                            .applied
                            .push("  (the find matched no text - check it)".to_string());
                    }
                } else {
                    report
                        .applied
                        .push(format!("{} applied to {touched} case(s).", describe(other)));
                }
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
        Op::SetReviewerNotes(v) => format!("Set reviewer notes to '{v}'"),
        Op::ReplaceInTitle { find, replace } => format!("Replaced '{find}' with '{replace}' in titles"),
        Op::ReplaceInNotes { find, replace } => {
            format!("Replaced '{find}' with '{replace}' in reviewer notes")
        }
        Op::PrefixTitle(v) => format!("Prefixed titles with '{v}'"),
        Op::SuffixTitle(v) => format!("Suffixed titles with '{v}'"),
        Op::ReplaceInSteps { find, replace } => format!("Replaced '{find}' with '{replace}' in steps"),
        Op::SortBy(k) => format!("Sorted by {k}"),
        Op::GroupBy(k) => format!("Grouped by {k}"),
        Op::Dedupe => "Deduped".to_string(),
        Op::PrependStep { action, .. } => format!("Prepended step '{action}'"),
        Op::AppendStep { action, .. } => format!("Appended step '{action}'"),
        Op::RemoveStepMatching(f) => format!("Removed steps matching '{f}'"),
        Op::SplitStep { find, into } => {
            format!("Split steps matching '{find}' into {} step(s)", into.len())
        }
        Op::RemoveCases => "Removed cases".to_string(),
        Op::InsertCases { cases, .. } => format!("Inserted {} case(s)", cases.len()),
        Op::NormaliseCitations => "Normalised citations".to_string(),
    }
}

/// The keys each op actually reads - the mapping §11 found missing from
/// the schema, now enforced: a key an op does not read lands in the
/// report's `ignored` list instead of vanishing.
fn known_keys(op_name: &str) -> &'static [&'static str] {
    match op_name {
        "set_tags" | "add_tags" | "remove_tags" | "set_module" | "set_automation_status"
        | "set_preconditions" | "set_reviewer_notes" | "prefix_title" | "suffix_title"
        | "sort_by" | "group_by" => &["op", "where", "value"],
        "replace_in_title" | "replace_in_steps" | "replace_in_notes" => {
            &["op", "where", "find", "replace"]
        }
        "prepend_step" | "append_step" => &["op", "where", "action", "expected"],
        "remove_step_matching" => &["op", "where", "value", "find", "action"],
        "split_step" => &["op", "where", "value", "find", "into"],
        "remove_cases" | "dedupe" | "normalise_citations" => &["op", "where"],
        "insert_cases" => &["op", "where", "cases", "at_index", "before", "after"],
        _ => &["op", "where"],
    }
}

/// The case-object keys `insert_cases` reads (importer aliases included).
/// Everything else on an inserted case is DISCARDED by the rebuild, and
/// §15's evidence showed `author`, `priority` and `step_number` vanishing
/// with `warnings: []` - so the discard is now echoed.
fn insert_case_unknown_keys(rv: &serde_json::Value) -> Vec<String> {
    const KNOWN: &[&str] = &[
        "title", "steps", "tags", "automation_status", "module", "module_value",
        "preconditions", "id", "update_id", "spec_order", "tester_order",
    ];
    let mut out = vec![];
    if let Some(obj) = rv.as_object() {
        for k in obj.keys() {
            let known = KNOWN.contains(&k.as_str())
                || crate::import_parser::COMMENT_KEYS.contains(&k.as_str())
                || crate::import_parser::REVIEWER_NOTES_KEYS.contains(&k.as_str());
            if !known {
                out.push(k.clone());
            }
        }
    }
    if let Some(steps) = rv["steps"].as_array() {
        for sv in steps {
            if let Some(obj) = sv.as_object() {
                for k in obj.keys() {
                    if k != "action" && k != "expected" && !out.contains(&format!("steps.{k}")) {
                        out.push(format!("steps.{k}"));
                    }
                }
            }
        }
    }
    out
}
