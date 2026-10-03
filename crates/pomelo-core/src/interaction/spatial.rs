//! Immutable bounds hierarchy shared by canvas picking categories.
use crate::{geometry::PathError, model::Bounds, task::CancellationToken};
#[derive(Clone, Copy)]
struct Entry {
    index: usize,
    bounds: Bounds,
}
struct Node {
    bounds: Bounds,
    range: std::ops::Range<usize>,
    children: Option<[usize; 2]>,
}
pub(crate) struct BoundsIndex {
    entries: Vec<Entry>,
    nodes: Vec<Node>,
}
impl BoundsIndex {
    pub fn build(
        bounds: impl IntoIterator<Item = Option<Bounds>>,
        cancel: &CancellationToken,
    ) -> Result<Self, super::picking_index::IndexError> {
        use super::picking_index::IndexError;
        let mut entries = Vec::new();
        for (index, bounds) in bounds.into_iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled.into());
            }
            if let Some(bounds) = bounds {
                if !bounds.is_valid() {
                    return Err(PathError::Invalid(crate::model::ObjectId(index as u32)).into());
                }
                entries.try_reserve(1).map_err(|_| IndexError::Allocation)?;
                entries.push(Entry { index, bounds });
            }
        }
        let mut nodes = Vec::new();
        fn build(
            entries: &mut [Entry],
            base: usize,
            nodes: &mut Vec<Node>,
            cancel: &CancellationToken,
        ) -> Result<usize, IndexError> {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled.into());
            }
            let mut bounds = entries[0].bounds;
            for e in &entries[1..] {
                bounds.include(e.bounds.min);
                bounds.include(e.bounds.max);
            }
            let id = nodes.len();
            nodes.try_reserve(1).map_err(|_| IndexError::Allocation)?;
            nodes.push(Node {
                bounds,
                range: base..base + entries.len(),
                children: None,
            });
            if entries.len() > 16 {
                let mid = entries.len() / 2;
                let axis = bounds.max.x - bounds.min.x >= bounds.max.y - bounds.min.y;
                entries.select_nth_unstable_by(mid, |a, b| {
                    let center = |e: &Entry| {
                        if axis {
                            e.bounds.min.x + e.bounds.max.x
                        } else {
                            e.bounds.min.y + e.bounds.max.y
                        }
                    };
                    center(a).total_cmp(&center(b))
                });
                let (a, b) = entries.split_at_mut(mid);
                nodes[id].children = Some([
                    build(a, base, nodes, cancel)?,
                    build(b, base + mid, nodes, cancel)?,
                ]);
            }
            Ok(id)
        }
        if !entries.is_empty() {
            build(&mut entries, 0, &mut nodes, cancel)?;
        }
        Ok(Self { entries, nodes })
    }
    pub fn query(
        &self,
        bounds: Bounds,
        cancel: &CancellationToken,
    ) -> Result<Vec<usize>, PathError> {
        let overlap = |b: Bounds| {
            bounds.min.x <= b.max.x
                && bounds.max.x >= b.min.x
                && bounds.min.y <= b.max.y
                && bounds.max.y >= b.min.y
        };
        let mut result = Vec::new();
        let mut pending = vec![0];
        while let Some(i) = pending.pop() {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            let Some(node) = self.nodes.get(i) else {
                continue;
            };
            if !overlap(node.bounds) {
                continue;
            }
            if let Some([a, b]) = node.children {
                pending.push(a);
                pending.push(b);
            } else {
                result.extend(
                    self.entries[node.range.clone()]
                        .iter()
                        .filter(|e| overlap(e.bounds))
                        .map(|e| e.index),
                );
            }
        }
        result.sort_unstable();
        Ok(result)
    }
}
