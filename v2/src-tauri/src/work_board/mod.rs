//! Work Manager board core, ported from v1 app/models/work_item.py and the
//! board-fetch/_state_for_column logic in mywork_screen.py. Columns derive
//! from each state's process *category*, never hardcoded state names - with
//! v1's two documented exceptions: a state literally named "Later" always
//! lands in Done, and unknown processes fall back to a name heuristic.
//!
//! Layout: this file owns the types and the pure column/WIQL logic;
//! `board` owns the WIQL + board fetch pipeline and team/member queries;
//! `detail` owns item detail, comments and avatars; `layout` owns the
//! process-layout discovery behind the extra form tabs.

mod board;
pub mod detail;
pub mod history;
mod layout;

use serde::Serialize;
use std::collections::HashMap;

pub const COLUMNS: [&str; 3] = ["To Do", "In Progress", "Done"];

/// Work-item types that never belong on the board.
pub const EXCLUDED_TYPES: [&str; 5] = [
    "Test Case",
    "Test Suite",
    "Test Plan",
    "Shared Steps",
    "Shared Parameter",
];

const MAX_ITEMS: u32 = 500;

/// Fields the board fetch asks for (rich-text fields deliberately excluded -
/// they are heavy and only the detail editor needs them).
const BOARD_FIELDS: &str = "System.Id,System.Title,System.WorkItemType,System.State,System.AssignedTo,System.ChangedDate,System.Tags,Microsoft.VSTS.Common.Priority";

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct StateInfo {
    pub name: String,
    pub color: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BoardItem {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
    pub state_color: String,
    /// "To Do" | "In Progress" | "Done"; None = hidden (Removed).
    pub column: Option<String>,
    pub assigned_to: String,
    pub tags: String,
    pub priority: Option<i32>,
    pub changed_date: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct BoardData {
    pub items: Vec<BoardItem>,
    pub states_by_type: HashMap<String, Vec<StateInfo>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct WorkItemDetail {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
    pub assigned_to: String,
    pub assigned_to_unique: String,
    pub activity: String,
    pub tags: String,
    pub area_path: String,
    pub iteration_path: String,
    pub remaining_work: Option<f64>,
    pub completed_work: Option<f64>,
    pub original_estimate: Option<f64>,
    pub start_date: String,
    pub target_date: String,
    /// Description (or ReproSteps for Bugs) flattened to plain text for the
    /// editor; saving wraps it back into a div like v1's preconditions.
    pub description_text: String,
    /// The same field's RAW HTML, so the editor can convert it to markdown
    /// and preserve the formatting ADO stored (bold, lists, links...).
    pub description_html: String,
    /// Which field the description came from (System.Description or
    /// Microsoft.VSTS.TCM.ReproSteps) so the save writes the right one.
    pub description_field: String,
    /// The process's extra form pages (Bug: RCA, Preventive Measures...)
    /// with every visible field on them, shown as editable tabs.
    pub extra_pages: Vec<ExtraPage>,
    /// Why extra_pages is empty when the layout lookup failed - surfaced in
    /// the drawer so a permissions/endpoint problem is visible, not silent.
    pub extra_pages_error: Option<String>,
    /// Authenticated attachment images from the rich-text fields, downloaded
    /// with the token so the preview can swap URLs for data: URIs (a plain
    /// <img> gets 401). Field values themselves stay byte-faithful.
    pub inline_images: Vec<InlineImage>,
}

/// One downloaded rich-text image: the (entity-unescaped) src URL and the
/// data: URI to show instead.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct InlineImage {
    pub url: String,
    pub data: String,
}

/// One custom form page (an ADO tab) and its editable fields, in form order.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ExtraPage {
    pub name: String,
    pub fields: Vec<ExtraField>,
}

/// A single field on an extra page.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ExtraField {
    pub label: String,
    pub reference_name: String,
    /// Which form section (column in ADO's layout) the field sits in.
    pub section: u32,
    /// "html" (rich text), "pick" (allowed values), or "text".
    pub kind: String,
    /// Allowed values when kind == "pick".
    pub allowed: Vec<String>,
    /// Current raw value (HTML for html fields).
    pub value: String,
}

/// An iteration path plus its sprint window, for DevOps-style pickers.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct IterationRef {
    pub path: String,
    pub start_date: Option<String>,
    pub finish_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Member {
    pub display_name: String,
    pub unique_name: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct TeamRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct WorkComment {
    pub id: i32,
    pub text: String,
    pub created_by: String,
    pub created_date: String,
    pub avatar_url: String,
}

/// One field's before/after inside a revision.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct FieldChange {
    pub reference_name: String,
    /// Human label ("Remaining Work"), not the reference name.
    pub label: String,
    /// Empty when the field had no previous value.
    pub old: String,
    /// Empty when the field was cleared.
    pub new: String,
}

/// One entry in a work item's history: everything one save changed.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct WorkRevision {
    pub rev: i32,
    pub by: String,
    pub avatar_url: String,
    /// ISO 8601; empty if ADO gave no usable date.
    pub at: String,
    pub fields: Vec<FieldChange>,
    pub links_added: Vec<String>,
    pub links_removed: Vec<String>,
    /// Pulled out of `fields` so the timeline can lead with the state
    /// move, which is what people scan history for.
    pub state_from: String,
    pub state_to: String,
    pub comment_added: bool,
}

#[derive(Debug, Clone, serde::Deserialize, specta::Type)]
pub struct FieldPatch {
    pub reference_name: String,
    pub value: String,
}

fn column_for_category(category: &str) -> Option<&'static str> {
    match category {
        "Proposed" => Some("To Do"),
        "InProgress" => Some("In Progress"),
        // Resolved and Completed both land in Done.
        "Resolved" | "Completed" => Some("Done"),
        _ => None, // Removed -> hidden
    }
}

/// Board column for an item, ported from v1 WorkItem.column().
pub fn column_for_state(
    wi_type: &str,
    state: &str,
    states_by_type: &HashMap<String, Vec<StateInfo>>,
) -> Option<String> {
    let name = state.trim().to_lowercase();
    // A state literally named "Later" always lands in Done - parked items
    // sit with the finished work regardless of process category.
    if name == "later" {
        return Some("Done".to_string());
    }
    if let Some(states) = states_by_type.get(wi_type) {
        if let Some(s) = states.iter().find(|s| s.name == state) {
            if !s.category.is_empty() {
                return column_for_category(&s.category).map(String::from);
            }
        }
    }
    // Unknown process/type: name heuristic, same order as v1.
    match name.as_str() {
        "new" | "to do" | "proposed" | "open" | "approved" | "design" => {
            Some("To Do".to_string())
        }
        "removed" => None,
        "done" | "closed" | "completed" | "resolved" => Some("Done".to_string()),
        _ => Some("In Progress".to_string()),
    }
}

fn column_categories(col: &str) -> &'static [&'static str] {
    match col {
        "To Do" => &["Proposed"],
        "In Progress" => &["InProgress"],
        "Done" => &["Completed", "Resolved"],
        _ => &[],
    }
}

/// The state a drop on `col` should move an item of `wi_type` to, ported
/// from v1 _state_for_column: prefer a state named exactly like the column
/// within the column's own categories (so Task/Bug dropped on In Progress
/// becomes "In Progress", not merely the first InProgress state like
/// "Active"); otherwise the first state of the column's categories in
/// workflow order. None when the process defines no such state.
pub fn state_for_column(
    wi_type: &str,
    col: &str,
    states_by_type: &HashMap<String, Vec<StateInfo>>,
) -> Option<String> {
    let states = states_by_type.get(wi_type)?;
    let cats = column_categories(col);
    let target = col.trim().to_lowercase();
    for s in states {
        if cats.contains(&s.category.as_str()) && s.name.trim().to_lowercase() == target {
            return Some(s.name.clone());
        }
    }
    for cat in cats {
        for s in states {
            if s.category == *cat {
                return Some(s.name.clone());
            }
        }
    }
    None
}

/// A WIQL string literal with quotes escaped (v1 _wiql_str).
pub fn wiql_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// A WIQL clause scoping to a team's area(s), ported from v1
/// _team_area_clause: tree fields use UNDER when includeChildren, else '='.
pub fn team_area_clause(field_ref: &str, values: &[(String, bool)]) -> String {
    let field = format!("[{field_ref}]");
    let tree = field_ref.ends_with("AreaPath") || field_ref.ends_with("IterationPath");
    values
        .iter()
        .filter(|(v, _)| !v.is_empty())
        .map(|(v, include_children)| {
            let op = if tree && *include_children { "UNDER" } else { "=" };
            format!("{field} {op} {}", wiql_str(v))
        })
        .collect::<Vec<_>>()
        .join(" OR ")
}
