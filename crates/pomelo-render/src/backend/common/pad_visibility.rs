//! Compact source-order visibility, intersected with camera-dependent ranges.
use std::ops::Range;

pub(super) struct Accepted {
    words: Vec<u64>,
    count: usize,
}
impl Accepted {
    pub(super) fn build(values: impl ExactSizeIterator<Item = bool>) -> anyhow::Result<Self> {
        let count = values.len();
        let mut words = Vec::new();
        words.try_reserve_exact(count.div_ceil(64))?;
        words.resize(count.div_ceil(64), 0);
        for (index, accepted) in values.enumerate() {
            if accepted {
                words[index / 64] |= 1 << (index % 64);
            }
        }
        Ok(Self { words, count })
    }

    pub(super) fn visible(
        &self,
        range: Range<usize>,
        mut emit: impl FnMut(Range<usize>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let end = range.end.min(self.count);
        let mut cursor = range.start.min(end);
        let mut pending: Option<Range<usize>> = None;
        while cursor < end {
            let word_start = cursor / 64 * 64;
            let word_end = (word_start + 64).min(end);
            let mut bits = self.words[cursor / 64] & (u64::MAX << (cursor % 64));
            if word_end - word_start < 64 {
                bits &= (1 << (word_end - word_start)) - 1;
            }
            while bits != 0 {
                let start = bits.trailing_zeros() as usize;
                let length = (bits >> start).trailing_ones() as usize;
                let next = word_start + start..word_start + start + length;
                if let Some(run) = pending.as_mut().filter(|run| run.end == next.start) {
                    run.end = next.end;
                } else {
                    if let Some(run) = pending.take() {
                        emit(run)?;
                    }
                    pending = Some(next);
                }
                if start + length == 64 {
                    break;
                }
                bits &= u64::MAX << (start + length);
            }
            cursor = word_end;
        }
        if let Some(run) = pending {
            emit(run)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bit_ranges_match_predicates_at_word_boundaries_and_partial_uploads() {
        for count in [0, 1, 63, 64, 65, 129, 4097] {
            for pattern in 0..4 {
                let accepted: Vec<_> = (0..count)
                    .map(|i| match pattern {
                        0 => false,
                        1 => true,
                        2 => i % 3 != 1,
                        _ => (60..133).contains(&i),
                    })
                    .collect();
                let mask = Accepted::build(accepted.iter().copied()).unwrap();
                for start in [0, 1, 61, 64, 128, count] {
                    for end in [0, 63, 65, 132, count] {
                        if start > end {
                            continue;
                        }
                        let mut actual = Vec::new();
                        let mut previous = None;
                        mask.visible(start..end, |range| {
                            if let Some(end) = previous {
                                assert!(end < range.start, "unmerged runs");
                            }
                            previous = Some(range.end);
                            actual.extend(range);
                            Ok(())
                        })
                        .unwrap();
                        let expected: Vec<_> = (start.min(count)..end.min(count))
                            .filter(|&i| accepted[i])
                            .collect();
                        assert_eq!(
                            actual, expected,
                            "count={count} pattern={pattern} range={start}..{end}"
                        );
                    }
                }
            }
        }
    }
}
