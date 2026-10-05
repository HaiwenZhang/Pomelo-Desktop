//! Contiguous analytic-pad ranges. Traversal preserves translucent source order.
use std::ops::Range;

const LEAF: usize = 8;
// Offscreen gaps of one leaf cost less than another native draw submission.
// Their quads clamp to degenerate viewport edges; source order is unchanged.
const MAX_GAP: usize = LEAF;
const EMPTY: [f64; 4] = [
    f64::INFINITY,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NEG_INFINITY,
];

pub(super) struct PadRanges {
    bounds: Vec<[f64; 4]>,
    leaves: usize,
    count: usize,
}

impl PadRanges {
    pub(super) fn build(values: impl ExactSizeIterator<Item = [f64; 4]>) -> anyhow::Result<Self> {
        let count = values.len();
        let leaves = count.div_ceil(LEAF).max(1).next_power_of_two();
        let mut bounds = Vec::new();
        bounds.try_reserve_exact(leaves * 2)?;
        bounds.resize(leaves * 2, EMPTY);
        for (index, mut value) in values.enumerate() {
            // Invalid metadata must never hide a previously submitted primitive.
            if !value.into_iter().all(f64::is_finite) || value[0] > value[2] || value[1] > value[3]
            {
                value = [
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                    f64::INFINITY,
                    f64::INFINITY,
                ];
            }
            include(&mut bounds[leaves + index / LEAF], value);
        }
        for node in (1..leaves).rev() {
            let mut value = bounds[node * 2];
            include(&mut value, bounds[node * 2 + 1]);
            bounds[node] = value;
        }
        Ok(Self {
            bounds,
            leaves,
            count,
        })
    }

    /// Conservative allowance for double-single camera subtraction in GPU shaders.
    pub(super) fn coordinate_magnitude(&self) -> f64 {
        self.bounds[1].into_iter().map(f64::abs).fold(0.0, f64::max)
    }

    pub(super) fn visible(
        &self,
        view: [f64; 4],
        range: Range<usize>,
        mut emit: impl FnMut(Range<usize>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let mut pending: Option<Range<usize>> = None;
        self.visit(1, 0..self.leaves * LEAF, &range, view, &mut |next| {
            if let Some(current) = &mut pending {
                if next.start - current.end <= MAX_GAP {
                    current.end = next.end;
                    return Ok(());
                }
                emit(current.clone())?;
            }
            pending = Some(next);
            Ok(())
        })?;
        if let Some(pending) = pending {
            emit(pending)?;
        }
        Ok(())
    }

    fn visit(
        &self,
        node: usize,
        span: Range<usize>,
        range: &Range<usize>,
        view: [f64; 4],
        emit: &mut impl FnMut(Range<usize>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if span.end <= range.start || span.start >= range.end || span.start >= self.count {
            return Ok(());
        }
        let b = self.bounds[node];
        if b[0] > view[2] || b[2] < view[0] || b[1] > view[3] || b[3] < view[1] {
            return Ok(());
        }
        if node >= self.leaves
            || (b[0] >= view[0] && b[2] <= view[2] && b[1] >= view[1] && b[3] <= view[3])
        {
            return emit(span.start.max(range.start)..span.end.min(range.end).min(self.count));
        }
        let middle = (span.start + span.end) / 2;
        self.visit(node * 2, span.start..middle, range, view, emit)?;
        self.visit(node * 2 + 1, middle..span.end, range, view, emit)
    }
}

fn include(bounds: &mut [f64; 4], value: [f64; 4]) {
    bounds[0] = bounds[0].min(value[0]);
    bounds[1] = bounds[1].min(value[1]);
    bounds[2] = bounds[2].max(value[2]);
    bounds[3] = bounds[3].max(value[3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_offscreen_leaf_gap_merges_but_larger_gaps_remain_culled() {
        let values: Vec<_> = (0..64)
            .map(|i| {
                if i < 8 || (16..24).contains(&i) || (48..56).contains(&i) {
                    [-0.5, -0.5, 0.5, 0.5]
                } else {
                    [100.0, 100.0, 101.0, 101.0]
                }
            })
            .collect();
        let ranges = PadRanges::build(values.into_iter()).unwrap();
        let mut emitted = Vec::new();
        ranges
            .visible([-1.0, -1.0, 1.0, 1.0], 3..53, |range| {
                emitted.push(range);
                Ok(())
            })
            .unwrap();
        assert_eq!(emitted, [3..24, 48..53]);
    }

    #[test]
    fn ranges_retain_all_intersections_and_source_order_at_partial_boundaries() {
        for count in [0usize, 1, 7, 8, 9, 31, 4096] {
            let values: Vec<_> = (0..count)
                .map(|i| {
                    let x = (i % 37) as f64 - 18.0;
                    let y = (i / 37) as f64;
                    [x - 0.2, y - 0.3, x + 0.2, y + 0.3]
                })
                .collect();
            let index = PadRanges::build(values.iter().copied()).unwrap();
            for first in [0, count / 3, count.saturating_sub(1)] {
                let range = first..count.saturating_sub(count / 7).max(first);
                for view in [
                    [-1.0, -1.0, 1.0, 1.0],
                    [-100.0, -100.0, 1000.0, 1000.0],
                    [1000.0; 4],
                ] {
                    let mut actual = Vec::new();
                    index
                        .visible(view, range.clone(), |r| {
                            actual.extend(r);
                            Ok(())
                        })
                        .unwrap();
                    assert!(actual.windows(2).all(|pair| pair[0] < pair[1]));
                    assert!(actual.iter().all(|i| range.contains(i)));
                    for i in range.clone() {
                        let b = values[i];
                        if b[0] <= view[2] && b[2] >= view[0] && b[1] <= view[3] && b[3] >= view[1]
                        {
                            assert!(actual.contains(&i), "missing visible {i}/{count}");
                        }
                    }
                }
            }
        }
    }
}
