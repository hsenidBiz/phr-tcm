//! The plan for a selection of cases: the order they run in, and the
//! reset points that split that order into phases. Pure: the cases' marks
//! and an optional saved order in, a `Plan` out. The webview asks for it
//! to show it (`commands::autorun::auto_run_plan`), and a run works it out
//! again from the same inputs, so the two always agree.
//!
//! Names compare by `marks::normalise`. The spelling shown for a name is
//! the first one seen, trimmed.

use super::marks::normalise;
use super::CaseScript;
use std::collections::{HashMap, HashSet};

/// One case's marks, as its script spells them: `changes` and
/// `needs_unchanged`. Compared through `normalise`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Marks {
    pub changes: Vec<String>,
    pub needs: Vec<String>,
}

impl Marks {
    /// The marks a saved script carries.
    pub fn of(script: &CaseScript) -> Marks {
        Marks { changes: script.changes.clone(), needs: script.needs_unchanged.clone() }
    }
}

/// A reset point: before `before_case_id` runs, a person reverts `names`.
/// `names` are display spellings, in the order the case lists them;
/// `changed_by` gives, for each of those names, the cases that changed it
/// since the last reset, in run order.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Reset {
    pub before_case_id: i32,
    pub names: Vec<String>,
    pub changed_by: Vec<(String, Vec<i32>)>,
}

/// The order, the same order split at each reset point, and the reset
/// points themselves (one fewer than the phases).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Plan {
    pub order: Vec<i32>,
    pub phases: Vec<Vec<i32>>,
    pub resets: Vec<Reset>,
}

/// Each case once, where it first appears.
fn first_of_each(cases: &[(i32, Marks)]) -> Vec<(i32, &Marks)> {
    let mut seen = HashSet::new();
    cases.iter().filter(|(id, _)| seen.insert(*id)).map(|(id, m)| (*id, m)).collect()
}

fn keys(names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for k in names.iter().map(|n| normalise(n)) {
        if !k.is_empty() && !out.contains(&k) {
            out.push(k);
        }
    }
    out
}

/// The suggested order (design §2.1): a stable topological sort of the
/// cases in list order over the constraints "a case that needs X unchanged
/// runs before a case that changes X", for every name X. A case that both
/// needs and changes X is a needer and a changer of X, with no constraint
/// on itself, so it runs after the other needers of X and before the other
/// changers.
///
/// Stable: Kahn's algorithm, always taking the available case that comes
/// first in list order, so cases no mark forces to move keep their places
/// relative to each other.
///
/// A cycle (A changes X and needs Y unchanged, B changes Y and needs X
/// unchanged): when no remaining case is free, the earliest remaining case
/// in list order is taken anyway, its unmet constraints ignored. A cycle is
/// only broken once nothing else can run, so every case that can run
/// before the break does, and the reset the break becomes falls as late in
/// the order as the constraints allow.
pub fn suggest(cases_in_list_order: &[(i32, Marks)]) -> Vec<i32> {
    let cases = first_of_each(cases_in_list_order);
    let n = cases.len();
    let needs: Vec<Vec<String>> = cases.iter().map(|(_, m)| keys(&m.needs)).collect();
    let changes: Vec<Vec<String>> = cases.iter().map(|(_, m)| keys(&m.changes)).collect();

    // after[i] holds every j that must run after i; incoming[j] counts them.
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut incoming = vec![0usize; n];
    for i in 0..n {
        for j in 0..n {
            if i != j && needs[i].iter().any(|k| changes[j].contains(k)) {
                after[i].push(j);
                incoming[j] += 1;
            }
        }
    }

    let mut placed = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while order.len() < n {
        let next = (0..n)
            .find(|&i| !placed[i] && incoming[i] == 0)
            .or_else(|| (0..n).find(|&i| !placed[i]))
            .expect("a case is left while the order is short");
        placed[next] = true;
        order.push(cases[next].0);
        for &j in &after[next] {
            incoming[j] = incoming[j].saturating_sub(1);
        }
    }
    order
}

/// The phases of an order (design §2.2). Walk the order; a new phase starts
/// before a case that needs X unchanged when X has already been changed in
/// the current phase. A case's own changes count after it runs. A case
/// missing from `marks` has none.
pub fn phases(order: &[i32], marks: &HashMap<i32, Marks>) -> Plan {
    let none = Marks::default();
    let mut shown: HashMap<String, String> = HashMap::new();
    let mut show = |name: &str| -> String {
        shown.entry(normalise(name)).or_insert_with(|| name.trim().to_string()).clone()
    };
    // Every spelling, in run order, so the first one seen is the one shown.
    for id in order {
        let m = marks.get(id).unwrap_or(&none);
        for name in m.changes.iter().chain(m.needs.iter()) {
            show(name);
        }
    }

    let mut plan = Plan { order: order.to_vec(), phases: Vec::new(), resets: Vec::new() };
    let mut phase: Vec<i32> = Vec::new();
    // Changed in this phase: each name's key, with the cases that changed it.
    let mut changed: Vec<(String, Vec<i32>)> = Vec::new();
    for &id in order {
        let m = marks.get(&id).unwrap_or(&none);
        let blocked: Vec<(String, Vec<i32>)> = keys(&m.needs)
            .into_iter()
            .filter_map(|k| changed.iter().find(|(c, _)| *c == k).cloned())
            .collect();
        if !blocked.is_empty() {
            plan.phases.push(std::mem::take(&mut phase));
            let changed_by: Vec<(String, Vec<i32>)> =
                blocked.into_iter().map(|(k, ids)| (shown.get(&k).cloned().unwrap_or(k), ids)).collect();
            plan.resets.push(Reset {
                before_case_id: id,
                names: changed_by.iter().map(|(n, _)| n.clone()).collect(),
                changed_by,
            });
            changed.clear();
        }
        phase.push(id);
        for k in keys(&m.changes) {
            match changed.iter_mut().find(|(c, _)| *c == k) {
                Some((_, ids)) => {
                    if !ids.contains(&id) {
                        ids.push(id);
                    }
                }
                None => changed.push((k, vec![id])),
            }
        }
    }
    if !phase.is_empty() {
        plan.phases.push(phase);
    }
    plan
}

/// The plan for a selection, in list order. With a saved order, that order
/// is used: ids not selected are dropped, and selected cases it misses go
/// at the end in list order. Without one, the suggested order is used.
///
/// The second value is `(saved resets, suggested resets)`, only when a
/// saved order is used and needs more resets than the suggestion.
pub fn plan_for(selected_in_list_order: &[(i32, Marks)], saved: Option<&[i32]>) -> (Plan, Option<(usize, usize)>) {
    let cases = first_of_each(selected_in_list_order);
    let marks: HashMap<i32, Marks> = cases.iter().map(|(id, m)| (*id, (*m).clone())).collect();
    let suggested = phases(&suggest(selected_in_list_order), &marks);
    let Some(saved) = saved else {
        return (suggested, None);
    };
    let mut order: Vec<i32> = Vec::with_capacity(cases.len());
    for id in saved.iter().chain(cases.iter().map(|(id, _)| id)) {
        if marks.contains_key(id) && !order.contains(id) {
            order.push(*id);
        }
    }
    let plan = phases(&order, &marks);
    let counts = (plan.resets.len() > suggested.resets.len()).then(|| (plan.resets.len(), suggested.resets.len()));
    (plan, counts)
}
