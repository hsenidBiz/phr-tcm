//! The plan for a selection of cases: the order they run in, and the
//! pauses that split that order into batches. Pure: the cases' marks and
//! settings and an optional saved order in, a plan out. The webview asks
//! for it to show it (`commands::autorun::auto_run_plan`), and a run works
//! it out again from the same inputs, so the two always agree.
//!
//! A pause falls where shared state has to be put back (a reset: a case
//! needs X unchanged and X was changed in this batch), or where a setting
//! a case needs (`needs_state`) differs from the one the batch already
//! runs with. A batch that needs settings before it starts pauses first,
//! even Batch 1. With no settings anywhere a plan is exactly the reset plan
//! it always was (`plan_for`, `Plan`).
//!
//! Names and values compare by `marks::normalise`. The spelling shown for
//! a name or a value is the first one seen walking the selected ids in
//! ascending order, trimmed.

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

/// One case's settings, as its script spells them: each setting it needs,
/// with the value it needs. A setting it does not name, it does not care
/// about.
pub type Needs = Vec<(String, String)>;

/// The settings a saved script needs.
pub fn needs_of(script: &CaseScript) -> Needs {
    script.needs_state.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
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

/// A pause before batch `batch` (1-based), which starts with
/// `before_case_id`: a person sets each setting in `set` to its value and
/// reverts `names` (`changed_by` as in [`Reset`]), then continues. Before
/// Batch 1 there is only `set`, and only when that batch needs settings.
/// Every spelling is a display one.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Pause {
    pub batch: u32,
    pub before_case_id: i32,
    pub set: Vec<(String, String)>,
    pub names: Vec<String>,
    pub changed_by: Vec<(String, Vec<i32>)>,
}

impl Pause {
    /// The reset part of this pause, when it has one.
    pub fn reset(&self) -> Option<Reset> {
        (!self.names.is_empty()).then(|| Reset {
            before_case_id: self.before_case_id,
            names: self.names.clone(),
            changed_by: self.changed_by.clone(),
        })
    }
}

/// The order, the same order split into batches, and the pauses: one
/// before each batch after the first, and one before Batch 1 when it needs
/// settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Batches {
    pub order: Vec<i32>,
    pub batches: Vec<Vec<i32>>,
    pub pauses: Vec<Pause>,
}

impl Batches {
    /// The reset plan: the batches as phases, and each pause's reset part.
    /// For a selection without settings, exactly the plan it always was.
    pub fn into_plan(self) -> Plan {
        let resets = self.pauses.iter().filter_map(Pause::reset).collect();
        Plan { order: self.order, phases: self.batches, resets }
    }
}

/// Each case once, where it first appears.
fn first_of_each<T>(cases: &[(i32, T)]) -> Vec<(i32, &T)> {
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

/// A case's settings as keys, `(name, value)`, each name once (the first
/// value given wins; a save refuses two values for one name).
fn setting_keys(needs: &[(String, String)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (n, v) in needs {
        let (k, val) = (normalise(n), normalise(v));
        if !k.is_empty() && !out.iter().any(|(o, _)| *o == k) {
            out.push((k, val));
        }
    }
    out
}

/// The constraints the suggested order keeps (design §2.1), as `after[i]`:
/// every case kept to run after case `i`, by list position.
fn kept(needs: &[Vec<String>], changes: &[Vec<String>]) -> Vec<Vec<usize>> {
    let n = needs.len();
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
    after
}

fn incoming_of(after: &[Vec<usize>]) -> Vec<usize> {
    let mut incoming = vec![0usize; after.len()];
    for js in after {
        for &j in js {
            incoming[j] += 1;
        }
    }
    incoming
}

/// The constraints `suggest` keeps, as `(needer, changer)` case ids: the
/// needer runs before the changer. One per pair at most, never both ways -
/// a backward constraint that would close a cycle is dropped, as `suggest`
/// drops it. For notes that must agree with the suggested order.
pub fn kept_constraints(cases_in_list_order: &[(i32, Marks)]) -> Vec<(i32, i32)> {
    let cases = first_of_each(cases_in_list_order);
    let needs: Vec<Vec<String>> = cases.iter().map(|(_, m)| keys(&m.needs)).collect();
    let changes: Vec<Vec<String>> = cases.iter().map(|(_, m)| keys(&m.changes)).collect();
    let after = kept(&needs, &changes);
    let mut out = Vec::new();
    for (i, js) in after.iter().enumerate() {
        out.extend(js.iter().map(|&j| (cases[i].0, cases[j].0)));
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
    let needs: Vec<Vec<String>> = cases.iter().map(|(_, m)| keys(&m.needs)).collect();
    let changes: Vec<Vec<String>> = cases.iter().map(|(_, m)| keys(&m.changes)).collect();
    let after = kept(&needs, &changes);
    stable_sort(&after).into_iter().map(|i| cases[i].0).collect()
}

/// The stable Kahn sort over the kept constraints: always the available
/// case that comes first in list order. List positions out.
fn stable_sort(after: &[Vec<usize>]) -> Vec<usize> {
    let n = after.len();
    let mut incoming = incoming_of(after);
    let mut placed = vec![false; n];
    let mut order = Vec::with_capacity(n);
    while let Some(next) = (0..n).find(|&i| !placed[i] && incoming[i] == 0) {
        placed[next] = true;
        order.push(next);
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

/// At most this many first batches are tried when settings are grouped.
const STARTS_TRIED: usize = 8;

/// The suggested order with settings (run batches). Without a setting
/// anywhere it is [`suggest`]'s order, unchanged.
///
/// With settings, the cases still keep every constraint `suggest` keeps.
/// Within them, a grouping walk takes the case that fits the batch so far
/// (it needs nothing changed in the batch, and agrees with every setting
/// the batch already names), first in list order; when none fits, it opens
/// a new batch. A few ways of opening batches are tried: first in list
/// order, the case most of the others on offer could join, and each
/// distinct first batch on offer. Every candidate is counted with the same
/// walk a run makes ([`batches`]), resets and settings pauses alike, and
/// the one with the fewest pauses among them wins; `suggest`'s own order is
/// the first candidate, so on a tie it stays. This tries to keep pauses
/// few: it is a heuristic, not a search for the smallest number possible.
pub fn suggest_batched(cases_in_list_order: &[(i32, Marks, Needs)]) -> Vec<i32> {
    let with: Vec<(i32, (&Marks, &Needs))> = cases_in_list_order.iter().map(|(id, m, s)| (*id, (m, s))).collect();
    let cases = first_of_each(&with);
    let n = cases.len();
    let needs: Vec<Vec<String>> = cases.iter().map(|(_, (m, _))| keys(&m.needs)).collect();
    let changes: Vec<Vec<String>> = cases.iter().map(|(_, (m, _))| keys(&m.changes)).collect();
    // The kept constraints, worked out once for the suggested order and
    // every grouping walk.
    let after = kept(&needs, &changes);
    let plain: Vec<i32> = stable_sort(&after).into_iter().map(|i| cases[i].0).collect();
    if cases.iter().all(|(_, (_, s))| setting_keys(s).is_empty()) {
        return plain;
    }
    let settings: Vec<Vec<(String, String)>> = cases.iter().map(|(_, (_, s))| setting_keys(s)).collect();
    let g = Grouping { after: &after, needs: &needs, changes: &changes, settings: &settings };

    let mut candidates: Vec<Vec<usize>> = vec![
        g.walk(None, false, false),
        g.walk(None, true, false),
        g.walk(None, false, true),
        g.walk(None, true, true),
    ];
    let incoming = incoming_of(&after);
    let mut profiles: Vec<&[(String, String)]> = Vec::new();
    for i in (0..n).filter(|&i| incoming[i] == 0) {
        if profiles.len() >= STARTS_TRIED {
            break;
        }
        if !profiles.iter().any(|p| same_settings(p, &settings[i])) {
            profiles.push(&settings[i]);
            candidates.push(g.walk(Some(i), true, false));
            candidates.push(g.walk(Some(i), true, true));
        }
    }

    let marks: HashMap<i32, Marks> = cases.iter().map(|(id, (m, _))| (*id, Marks::clone(m))).collect();
    let wants: HashMap<i32, Needs> = cases.iter().map(|(id, (_, s))| (*id, Needs::clone(s))).collect();
    let shown = Shown::default();
    let mut best = batches_spelled(&plain, &marks, &wants, &shown).pauses.len();
    let mut chosen = plain;
    for c in candidates {
        let order: Vec<i32> = c.iter().map(|&i| cases[i].0).collect();
        let pauses = batches_spelled(&order, &marks, &wants, &shown).pauses.len();
        if pauses < best {
            best = pauses;
            chosen = order;
        }
    }
    chosen
}

fn same_settings(a: &[(String, String)], b: &[(String, String)]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

/// Whether two settings lists agree on every setting both name.
fn agree(a: &[(String, String)], b: &[(String, String)]) -> bool {
    a.iter().all(|(k, v)| b.iter().all(|(k2, v2)| k2 != k || v2 == v))
}

/// The cases by list position, with the constraints `suggest` keeps.
struct Grouping<'a> {
    after: &'a [Vec<usize>],
    needs: &'a [Vec<String>],
    changes: &'a [Vec<String>],
    settings: &'a [Vec<(String, String)>],
}

impl Grouping<'_> {
    /// One grouping walk: a Kahn sort over the kept constraints that takes
    /// the available case that fits the batch so far, first in list order,
    /// and opens a new batch only when none fits. A new batch opens with
    /// `start` (the first one, when it is on offer), else with the case most
    /// of the others on offer could join (`widest`), else with the first.
    ///
    /// With `merge`, a new batch opens first with a case on offer that needs
    /// a reset anyway, so that reset rides the same pause as the settings.
    fn walk(&self, start: Option<usize>, widest: bool, merge: bool) -> Vec<usize> {
        let n = self.after.len();
        let mut incoming = incoming_of(self.after);
        let mut placed = vec![false; n];
        let mut order: Vec<usize> = Vec::with_capacity(n);
        let mut batch: Vec<(String, String)> = Vec::new();
        let mut changed: Vec<String> = Vec::new();
        loop {
            let open: Vec<usize> = (0..n).filter(|&i| !placed[i] && incoming[i] == 0).collect();
            if open.is_empty() {
                break;
            }
            let joining = if order.is_empty() {
                None
            } else {
                open.iter().copied().find(|&i| {
                    !self.needs[i].iter().any(|k| changed.contains(k)) && agree(&self.settings[i], &batch)
                })
            };
            let next = match joining {
                Some(i) => i,
                None => {
                    batch.clear();
                    let due: Vec<usize> = if merge {
                        open.iter().copied().filter(|&i| self.needs[i].iter().any(|k| changed.contains(k))).collect()
                    } else {
                        Vec::new()
                    };
                    let from = if due.is_empty() { &open } else { &due };
                    let s = match start.filter(|s| order.is_empty() && open.contains(s)) {
                        Some(s) => s,
                        None if widest => self.widest(from),
                        None => from[0],
                    };
                    // A pause that resets clears what was changed, as the
                    // run's walk does; one that only sets settings puts
                    // nothing back, so what was changed stays changed.
                    if self.needs[s].iter().any(|k| changed.contains(k)) {
                        changed.clear();
                    }
                    s
                }
            };
            placed[next] = true;
            order.push(next);
            for (k, v) in &self.settings[next] {
                if !batch.iter().any(|(b, _)| b == k) {
                    batch.push((k.clone(), v.clone()));
                }
            }
            for k in &self.changes[next] {
                if !changed.contains(k) {
                    changed.push(k.clone());
                }
            }
            for &j in &self.after[next] {
                incoming[j] -= 1;
            }
        }
        debug_assert_eq!(order.len(), n, "the kept constraints have no cycle");
        order
    }

    /// The case on offer most of the others on offer could run after in
    /// the same batch; the first in list order on a tie.
    fn widest(&self, open: &[usize]) -> usize {
        let joins = |i: usize| {
            open.iter()
                .filter(|&&j| {
                    j != i
                        && agree(&self.settings[j], &self.settings[i])
                        && !self.needs[j].iter().any(|k| self.changes[i].contains(k))
                })
                .count()
        };
        let mut best = open[0];
        let mut most = joins(best);
        for &i in &open[1..] {
            let c = joins(i);
            if c > most {
                best = i;
                most = c;
            }
        }
        best
    }
}

/// How names and values are shown: each key with the first spelling seen.
#[derive(Default)]
struct Shown {
    /// Marks (`changes`, `needs_unchanged`).
    marks: HashMap<String, String>,
    /// Setting names.
    settings: HashMap<String, String>,
    /// Setting values, by (name key, value key).
    values: HashMap<(String, String), String>,
}

impl Shown {
    fn mark(&self, k: &str) -> String {
        self.marks.get(k).cloned().unwrap_or_else(|| k.to_string())
    }
    fn setting(&self, k: &str, v: &str) -> (String, String) {
        (
            self.settings.get(k).cloned().unwrap_or_else(|| k.to_string()),
            self.values.get(&(k.to_string(), v.to_string())).cloned().unwrap_or_else(|| v.to_string()),
        )
    }
}

/// Each key with the first spelling seen walking `ids` (trimmed).
fn spellings(ids: &[i32], marks: &HashMap<i32, Marks>, wants: &HashMap<i32, Needs>) -> Shown {
    let mut shown = Shown::default();
    for id in ids {
        if let Some(m) = marks.get(id) {
            for name in m.changes.iter().chain(m.needs.iter()) {
                shown.marks.entry(normalise(name)).or_insert_with(|| name.trim().to_string());
            }
        }
        if let Some(s) = wants.get(id) {
            for (name, value) in s {
                let k = normalise(name);
                shown.settings.entry(k.clone()).or_insert_with(|| name.trim().to_string());
                shown.values.entry((k, normalise(value))).or_insert_with(|| value.trim().to_string());
            }
        }
    }
    shown
}

/// The phases of an order (design §2.2). Walk the order; a new phase starts
/// before a case that needs X unchanged when X has already been changed in
/// the current phase. A case's own changes count after it runs. A case
/// missing from `marks` has none.
///
/// A name is shown in the first spelling seen walking `order`; `plan_for`
/// shows the first spelling walking the ids in ascending order instead.
pub fn phases(order: &[i32], marks: &HashMap<i32, Marks>) -> Plan {
    let none = HashMap::new();
    batches_spelled(order, marks, &none, &spellings(order, marks, &none)).into_plan()
}

/// The batches of an order: [`phases`], with a new batch also before a
/// case that needs a setting at a value other than the one the current
/// batch already needs it at. The pause before each batch sets every
/// setting the batch names whose value differs from the last one set (or
/// that was never set), in the order the batch first names them. A case
/// missing from `wants` needs no settings.
pub fn batches(order: &[i32], marks: &HashMap<i32, Marks>, wants: &HashMap<i32, Needs>) -> Batches {
    batches_spelled(order, marks, wants, &spellings(order, marks, wants))
}

/// One batch as the walk closes it: its cases, the reset it opens with,
/// and the settings it names, as keys.
struct Walked {
    ids: Vec<i32>,
    reset: Vec<(String, Vec<i32>)>,
    settings: Vec<(String, String)>,
}

fn batches_spelled(
    order: &[i32],
    marks: &HashMap<i32, Marks>,
    wants: &HashMap<i32, Needs>,
    shown: &Shown,
) -> Batches {
    let none = Marks::default();
    let nothing: Needs = Vec::new();
    let mut walked: Vec<Walked> = Vec::new();
    let mut current = Walked { ids: Vec::new(), reset: Vec::new(), settings: Vec::new() };
    // Changed in this batch: each name's key, with the cases that changed it.
    let mut changed: Vec<(String, Vec<i32>)> = Vec::new();
    for &id in order {
        let m = marks.get(&id).unwrap_or(&none);
        let needs = setting_keys(wants.get(&id).unwrap_or(&nothing));
        let blocked: Vec<(String, Vec<i32>)> = keys(&m.needs)
            .into_iter()
            .filter_map(|k| changed.iter().find(|(c, _)| *c == k).cloned())
            .collect();
        let clash = !agree(&needs, &current.settings);
        if !current.ids.is_empty() && (!blocked.is_empty() || clash) {
            // A reset pause puts things back, and starts the count of what
            // is changed afresh (as it always has). A pause that only sets
            // settings puts nothing back: what is changed stays changed, so
            // a reset still due shows up at the pause where it is needed.
            if !blocked.is_empty() {
                changed.clear();
            }
            walked.push(std::mem::replace(
                &mut current,
                Walked { ids: Vec::new(), reset: blocked, settings: Vec::new() },
            ));
        }
        current.ids.push(id);
        for (k, v) in needs {
            if !current.settings.iter().any(|(s, _)| *s == k) {
                current.settings.push((k, v));
            }
        }
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
    if !current.ids.is_empty() {
        walked.push(current);
    }

    let mut out = Batches { order: order.to_vec(), batches: Vec::new(), pauses: Vec::new() };
    // The value each setting was last set to.
    let mut world: HashMap<String, String> = HashMap::new();
    for (b, w) in walked.into_iter().enumerate() {
        let set: Vec<(String, String)> = w
            .settings
            .iter()
            .filter(|(k, v)| world.get(k) != Some(v))
            .map(|(k, v)| shown.setting(k, v))
            .collect();
        for (k, v) in &w.settings {
            world.insert(k.clone(), v.clone());
        }
        if b > 0 || !set.is_empty() {
            let changed_by: Vec<(String, Vec<i32>)> =
                w.reset.into_iter().map(|(k, ids)| (shown.mark(&k), ids)).collect();
            out.pauses.push(Pause {
                batch: b as u32 + 1,
                before_case_id: w.ids[0],
                set,
                names: changed_by.iter().map(|(n, _)| n.clone()).collect(),
                changed_by,
            });
        }
        out.batches.push(w.ids);
    }
    out
}

/// The plan for a selection, in list order. With a saved order, that order
/// is used: ids not selected are dropped, and selected cases it misses go
/// at the end in list order. Without one, the suggested order is used.
///
/// The second value is `(saved resets, suggested resets)`, only when a
/// saved order is used and needs more resets than the suggestion.
pub fn plan_for(selected_in_list_order: &[(i32, Marks)], saved: Option<&[i32]>) -> (Plan, Option<(usize, usize)>) {
    let with: Vec<(i32, Marks, Needs)> =
        selected_in_list_order.iter().map(|(id, m)| (*id, m.clone(), Vec::new())).collect();
    let (batches, counts) = batches_for(&with, saved);
    (batches.into_plan(), counts)
}

/// [`plan_for`] with settings: the batches and pauses for a selection, in
/// list order, from [`suggest_batched`] or the saved order.
///
/// The second value is `(this order's pauses, the suggested order's
/// pauses)`, only when a saved order is used and needs more pauses than
/// the suggestion. Without settings, pauses are resets.
pub fn batches_for(
    selected_in_list_order: &[(i32, Marks, Needs)],
    saved: Option<&[i32]>,
) -> (Batches, Option<(usize, usize)>) {
    let with: Vec<(i32, (Marks, Needs))> =
        selected_in_list_order.iter().map(|(id, m, s)| (*id, (m.clone(), s.clone()))).collect();
    let cases = first_of_each(&with);
    let marks: HashMap<i32, Marks> = cases.iter().map(|(id, (m, _))| (*id, m.clone())).collect();
    let wants: HashMap<i32, Needs> = cases.iter().map(|(id, (_, s))| (*id, s.clone())).collect();
    // Spelled from the ids in ascending order, so a saved order and the
    // suggestion show a name the same way, and so does every caller - the
    // run dialog, the run itself, its record and its report - whatever
    // order the ids arrive in.
    let mut ascending: Vec<i32> = cases.iter().map(|(id, _)| *id).collect();
    ascending.sort_unstable();
    let shown = spellings(&ascending, &marks, &wants);
    let suggested = batches_spelled(&suggest_batched(selected_in_list_order), &marks, &wants, &shown);
    let Some(saved) = saved else {
        return (suggested, None);
    };
    let mut order: Vec<i32> = Vec::with_capacity(cases.len());
    for id in saved.iter().chain(cases.iter().map(|(id, _)| id)) {
        if marks.contains_key(id) && !order.contains(id) {
            order.push(*id);
        }
    }
    let plan = batches_spelled(&order, &marks, &wants, &shown);
    let counts = (plan.pauses.len() > suggested.pauses.len()).then(|| (plan.pauses.len(), suggested.pauses.len()));
    (plan, counts)
}
