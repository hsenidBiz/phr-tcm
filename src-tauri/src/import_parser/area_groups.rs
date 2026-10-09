//! The Queue's "Group by area", for the page View in browser writes: a
//! case's `area` path read as nested groups, one level per segment.
//!
//! The same tree the app's Queue draws (`src/lib/areaGroups.ts`), built the
//! same way and tested on the same cases. Unlike the Test map
//! (`test_map::build_tree`, A-Z), the order is the queue's own: a group
//! appears where its first case appears, cases inside a group keep queue
//! order, and cases with no area sit under "No area", last (not
//! "Ungrouped": a real area may be called that). Segments
//! compare ignoring case and the spaces around them, so "Display" and
//! "display " are one group, shown with the first spelling seen.

use crate::model::TestCase;

pub const NO_AREA: &str = "No area";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AreaGroup {
    /// This level's segment, as first spelled in the queue.
    pub name: String,
    /// The whole path, as first spelled: "Manage Events / Create".
    pub path: String,
    /// The path folded for comparison (lower case, trimmed segments).
    /// Empty for "No area", which no real path can be.
    pub key: String,
    /// Every case under this group, nested ones included.
    pub count: usize,
    /// Queue indices of the cases directly in this group, in queue order.
    pub indices: Vec<usize>,
    pub children: Vec<AreaGroup>,
}

impl AreaGroup {
    fn new(name: &str, path: String, key: String) -> Self {
        AreaGroup { name: name.to_string(), path, key, count: 0, indices: vec![], children: vec![] }
    }

    /// Every queue index under this group, nested included, in the order
    /// the grouped page shows them: its own cases, then each subgroup's.
    pub fn all_indices(&self) -> Vec<usize> {
        let mut out = self.indices.clone();
        for c in &self.children {
            out.extend(c.all_indices());
        }
        out
    }

    fn tally(&mut self) -> usize {
        self.count = self.indices.len() + self.children.iter_mut().map(AreaGroup::tally).sum::<usize>();
        self.count
    }
}

/// The tree of areas for these cases, in queue order.
pub fn build(cases: &[TestCase]) -> Vec<AreaGroup> {
    let mut roots: Vec<AreaGroup> = vec![];
    let mut no_area: Option<AreaGroup> = None;
    for (i, tc) in cases.iter().enumerate() {
        let segments: Vec<&str> = tc.area.split('/').map(str::trim).filter(|s| !s.is_empty()).collect();
        if segments.is_empty() {
            no_area
                .get_or_insert_with(|| AreaGroup::new(NO_AREA, NO_AREA.to_string(), String::new()))
                .indices
                .push(i);
            continue;
        }
        let mut siblings = &mut roots;
        let (mut parent_path, mut parent_key) = (String::new(), String::new());
        let last = segments.len() - 1;
        for (depth, segment) in segments.iter().enumerate() {
            let folded = segment.to_lowercase();
            let pos = match siblings.iter().position(|g| g.name.to_lowercase() == folded) {
                Some(p) => p,
                None => {
                    let (path, key) = if depth == 0 {
                        (segment.to_string(), folded.clone())
                    } else {
                        (format!("{parent_path} / {segment}"), format!("{parent_key} / {folded}"))
                    };
                    siblings.push(AreaGroup::new(segment, path, key));
                    siblings.len() - 1
                }
            };
            let node = &mut siblings[pos];
            if depth == last {
                node.indices.push(i);
            }
            parent_path.clone_from(&node.path);
            parent_key.clone_from(&node.key);
            siblings = &mut node.children;
        }
    }
    roots.extend(no_area);
    for g in &mut roots {
        g.tally();
    }
    roots
}
