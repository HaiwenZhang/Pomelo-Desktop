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
    /// Upper bound when every source item has valid bounds. Entries and tree
    /// nodes are reserved once, so construction needs no overlapping growth buffers.
    pub(crate) fn max_allocation_bytes(count: usize) -> usize {
        count
            .saturating_mul(size_of::<Entry>())
            .saturating_add(node_count(count).saturating_mul(size_of::<Node>()))
    }

    pub fn build(
        bounds: impl IntoIterator<Item = Option<Bounds>>,
        cancel: &CancellationToken,
    ) -> Result<Self, super::picking_index::IndexError> {
        use super::picking_index::IndexError;
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled.into());
        }
        let bounds = bounds.into_iter();
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(bounds.size_hint().0)
            .map_err(|_| IndexError::Allocation)?;
        for (index, bounds) in bounds.enumerate() {
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
        nodes
            .try_reserve_exact(node_count(entries.len()))
            .map_err(|_| IndexError::Allocation)?;
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

fn node_count(entries: usize) -> usize {
    if entries == 0 {
        0
    } else if entries <= 16 {
        1
    } else {
        let left = entries / 2;
        let left_nodes = node_count(left);
        let right_nodes = if entries.is_multiple_of(2) {
            left_nodes
        } else {
            node_count(entries - left)
        };
        1usize
            .saturating_add(left_nodes)
            .saturating_add(right_nodes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Point;

    #[test]
    fn large_owner_hierarchies_fit_the_budget_and_preserve_source_candidates() {
        let cancel = CancellationToken::default();
        for count in [0, 1, 16, 17, 33, 1024, 4097] {
            let source: Vec<_> = (0..count)
                .map(|index| {
                    (index % 5 != 0).then_some(Bounds {
                        min: Point::new(index as f64, 0.0),
                        max: Point::new(index as f64 + 2.0, 1.0),
                    })
                })
                .collect();
            let index = BoundsIndex::build(source.iter().copied(), &cancel).unwrap();
            let bytes = index.entries.capacity() * size_of::<Entry>()
                + index.nodes.capacity() * size_of::<Node>();
            assert!(bytes <= BoundsIndex::max_allocation_bytes(count));
            let query = Bounds {
                min: Point::new(12.5, -1.0),
                max: Point::new(25.5, 2.0),
            };
            let expected: Vec<_> = source
                .iter()
                .enumerate()
                .filter_map(|(position, bounds)| {
                    bounds
                        .filter(|bounds| bounds.min.x <= query.max.x && bounds.max.x >= query.min.x)
                        .map(|_| position)
                })
                .collect();
            assert_eq!(index.query(query, &cancel).unwrap(), expected);
        }
    }
}
