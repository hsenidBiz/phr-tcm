//! The plan for a selection of cases: the order they run in, and the
//! reset points that split that order into phases. Pure: the cases' marks
//! and an optional saved order in, a `Plan` out. The webview asks for it
//! to show it (`commands::autorun::auto_run_plan`), and a run works it out
//! again from the same inputs, so the two always agree.
//!
//! Names compare by `marks::normalise`. The spelling shown for a name is
//! the first one seen in list order, trimmed.

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
/// List order is kept wherever no mark forces a move. A constraint that
/// agrees with list order (the needer is listed before the changer) is
/// always kept. A backward one (the needer is listed after the changer)
/// moves the needer ahead, so it is kept only when it closes no cycle with
/// the constraints kept so far; the backward ones are tried by the needer's
/// list position, then the changer's. A dropped backward constraint is
/// where a reset falls: the needer may then run after the changer. So a
/// cycle (A changes X and needs Y unchanged, B changes Y and needs X
/// unchanged) costs one reset, and nothing moves to avoid a reset that no
/// order avoids.
///
/// The kept constraints have no cycle, and a stable Kahn sort over them
/// (always taking the available case that comes first in list order) gives
/// the order.
pub fn suggest(cases_in_list_order: &[(i32, Marks)]) -> Vec<i32> {
    let cases = first_of_each(cases_in_list_order);
    let n = cases.len();
    let needs: Vec<Vec<String>> = cases.iter().map(|(_, m)| keys(&m.needs)).collect();
    let changes: Vec<Vec<String>> = cases.iter().map(|(_, m)| keys(&m.changes)).collect();

    // Every constraint (needer, changer), by list position. Generated with
    // the needer outer and the changer inner, so the backward ones come out
    // already ordered by the needer's position, then the changer's.
    let mut forward: Vec<(usize, usize)> = Vec::new();
    let mut backward: Vec<(usize, usize)> = Vec::new();
    for i in 0..n {
        for j in 0..n {
            if i != j && needs[i].iter().any(|k| changes[j].contains(k)) {
                if i < j {
                    forward.push((i, j));
                } else {
                    backward.push((i, j));
                }
            }
        }
    }

    // after[i] holds every j kept to run after i.
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); n];
    for &(i, j) in &forward {
        after[i].push(j);
    }
    for &(i, j) in &backward {
        if !reaches(&after, j, i) {
            after[i].push(j);
        }
    }

    let mut incoming = vec![0usize; n];
    for js in &after {
        for &j in js {
            incoming[j] += 1;
        }
    }
    let mut placed = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while let Some(next) = (0..n).find(|&i| !placed[i] && incoming[i] == 0) {
        placed[next] = true;
        order.push(cases[next].0);
        for &j in &after[next] {
            incoming[j] -= 1;
        }
    }
    debug_assert_eq!(order.len(), n, "the kept constraints have no cycle");
    order
}

/// Whether `to` can be reached from `from` over the kept constraints: an
/// edge `to -> from` added now would close a cycle.
fn reaches(after: &[Vec<usize>], from: usize, to: usize) -> bool {
    let mut seen = vec![false; after.len()];
    let mut stack = vec![from];
    while let Some(at) = stack.pop() {
        if at == to {
            return true;
        }
        if !std::mem::replace(&mut seen[at], true) {
            stack.extend(after[at].iter().copied());
        }
    }
    false
}

/// The phases of an order (design §2.2). Walk the order; a new phase starts
/// before a case that needs X unchanged when X has already been changed in
/// the current phase. A case's own changes count after it runs. A case
/// missing from `marks` has none.
///
/// A name is shown in the first spelling seen walking `order`; `plan_for`
/// shows the first spelling in list order instead.
pub fn phases(order: &[i32], marks: &HashMap<i32, Marks>) -> Plan {
    phases_spelled(order, marks, &spellings(order, marks))
}

/// Each name's key, with the first spelling seen walking `ids` (trimmed).
fn spellings(ids: &[i32], marks: &HashMap<i32, Marks>) -> HashMap<String, String> {
    let mut shown: HashMap<String, String> = HashMap::new();
    for id in ids {
        if let Some(m) = marks.get(id) {
            for name in m.changes.iter().chain(m.needs.iter()) {
                shown.entry(normalise(name)).or_insert_with(|| name.trim().to_string());
            }
        }
    }
    shown
}

fn phases_spelled(order: &[i32], marks: &HashMap<i32, Marks>, shown: &HashMap<String, String>) -> Plan {
    let none = Marks::default();
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
    // Spelled from list order, so a saved order and the suggestion show a
    // name the same way.
    let list: Vec<i32> = cases.iter().map(|(id, _)| *id).collect();
    let shown = spellings(&list, &marks);
    let suggested = phases_spelled(&suggest(selected_in_list_order), &marks, &shown);
    let Some(saved) = saved else {
        return (suggested, None);
    };
    let mut order: Vec<i32> = Vec::with_capacity(cases.len());
    for id in saved.iter().chain(cases.iter().map(|(id, _)| id)) {
        if marks.contains_key(id) && !order.contains(id) {
            order.push(*id);
        }
    }
    let plan = phases_spelled(&order, &marks, &shown);
    let counts = (plan.resets.len() > suggested.resets.len()).then(|| (plan.resets.len(), suggested.resets.len()));
    (plan, counts)
}
