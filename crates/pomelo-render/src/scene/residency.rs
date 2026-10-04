//! Source-ordered spatial runs with visible pinning and offscreen cache eviction.
use pomelo_core::model::{Bounds, Point};
use std::ops::Range;

pub struct ResidentSlot<T> {
    pub range: Range<usize>,
    pub bounds: Option<Bounds>,
    pub payload: Option<T>,
    required: bool,
    nearby: bool,
    last_used: u64,
}
pub struct ResidencyCache<T> {
    pub slots: Vec<ResidentSlot<T>>,
    stride: usize,
    soft_bytes: usize,
    epoch: u64,
    pub evictions: u64,
}
fn intersects(a: Bounds, b: Bounds) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}
impl<T> ResidencyCache<T> {
    pub fn new(
        count: usize,
        chunk_size: usize,
        stride: usize,
        soft_bytes: usize,
        mut bounds: impl FnMut(Range<usize>) -> Option<Bounds>,
    ) -> Result<Self, crate::tracks::PrepareError> {
        if chunk_size == 0 || stride == 0 {
            return Err(crate::tracks::PrepareError::Allocation);
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(count.div_ceil(chunk_size))
            .map_err(|_| crate::tracks::PrepareError::Allocation)?;
        for start in (0..count).step_by(chunk_size) {
            let range = start..start.saturating_add(chunk_size).min(count);
            slots.push(ResidentSlot {
                bounds: bounds(range.clone()).filter(|bounds| bounds.is_valid()),
                range,
                payload: None,
                required: false,
                nearby: false,
                last_used: 0,
            });
        }
        Ok(Self {
            slots,
            stride,
            soft_bytes,
            epoch: 0,
            evictions: 0,
        })
    }
    pub fn resident_bytes(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.payload.is_some())
            .fold(0usize, |bytes, slot| {
                bytes.saturating_add(slot.range.len().saturating_mul(self.stride))
            })
    }
    pub fn over_budget_bytes(&self) -> usize {
        self.resident_bytes().saturating_sub(self.soft_bytes)
    }
    pub fn ready(&self) -> bool {
        self.slots
            .iter()
            .all(|slot| !slot.required || slot.payload.is_some())
    }
    pub fn update(&mut self, view: Option<Bounds>) -> Result<(), crate::tracks::PrepareError> {
        let view = view.filter(|view| view.is_valid());
        self.epoch = self.epoch.saturating_add(1);
        let nearby = view.map(|view| {
            let x = (view.max.x - view.min.x) * 0.25;
            let y = (view.max.y - view.min.y) * 0.25;
            Bounds {
                min: Point::new(view.min.x - x, view.min.y - y),
                max: Point::new(view.max.x + x, view.max.y + y),
            }
        });
        let mut missing = 0usize;
        for slot in &mut self.slots {
            // Unknown bounds stay visible; culling must never invent empty geometry.
            slot.required = view
                .zip(slot.bounds)
                .is_none_or(|(view, bounds)| intersects(view, bounds));
            slot.nearby = nearby
                .zip(slot.bounds)
                .is_none_or(|(view, bounds)| intersects(view, bounds));
            if slot.required {
                slot.last_used = self.epoch;
                if slot.payload.is_none() {
                    missing = missing.saturating_add(slot.range.len().saturating_mul(self.stride));
                }
            }
        }
        // Reserve space for all visible misses first. Visible chunks remain pinned,
        // even if a whole-board view exceeds the soft cache target.
        let mut bytes = self.resident_bytes().saturating_add(missing);
        if bytes <= self.soft_bytes {
            return Ok(());
        }
        // Sort eviction candidates once. Re-scanning the whole cache for each
        // victim makes a large pan quadratic in the number of resident chunks.
        let candidates = self
            .slots
            .iter()
            .filter(|slot| !slot.required && slot.payload.is_some())
            .count();
        let mut victims = Vec::new();
        victims
            .try_reserve_exact(candidates)
            .map_err(|_| crate::tracks::PrepareError::Allocation)?;
        victims.extend(
            self.slots
                .iter()
                .enumerate()
                .filter(|(_, slot)| !slot.required && slot.payload.is_some())
                .map(|(index, slot)| (slot.nearby, slot.last_used, index)),
        );
        victims.sort_unstable();
        for (_, _, victim) in victims {
            if bytes <= self.soft_bytes {
                break;
            }
            let slot = &mut self.slots[victim];
            slot.payload = None;
            bytes = bytes.saturating_sub(slot.range.len().saturating_mul(self.stride));
            self.evictions += 1;
        }
        Ok(())
    }
    pub fn pending(&self, max_chunks: usize) -> impl Iterator<Item = usize> + '_ {
        let visible = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.required && slot.payload.is_none());
        let nearby = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| !slot.required && slot.nearby && slot.payload.is_none());
        visible
            .chain(nearby)
            .map(|(index, _)| index)
            .take(max_chunks)
    }
    pub fn can_prefetch(&self, index: usize) -> bool {
        self.slots[index].required
            || self
                .resident_bytes()
                .saturating_add(self.slots[index].range.len().saturating_mul(self.stride))
                <= self.soft_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn bounds(x: f64) -> Bounds {
        Bounds {
            min: Point::new(x, 0.0),
            max: Point::new(x + 1.0, 1.0),
        }
    }
    #[test]
    fn panning_evicts_offscreen_chunks_but_preserves_visible_source_order() {
        let mut cache = ResidencyCache::<usize>::new(6, 2, 8, 16, |range| {
            Some(bounds(range.start as f64 * 5.0))
        })
        .unwrap();
        cache.update(Some(bounds(10.0))).unwrap();
        assert_eq!(cache.pending(10).collect::<Vec<_>>(), [1]);
        cache.slots[1].payload = Some(1);
        assert!(cache.ready());
        cache.update(Some(bounds(20.0))).unwrap();
        assert!(cache.slots[1].payload.is_none());
        assert_eq!(cache.evictions, 1);
        assert_eq!(cache.pending(10).collect::<Vec<_>>(), [2]);
        assert!(!cache.ready());
    }
    #[test]
    fn whole_board_view_pins_all_geometry_and_reports_soft_budget_overage() {
        let mut cache = ResidencyCache::new(4, 2, 8, 16, |_| None).unwrap();
        cache.update(Some(bounds(0.0))).unwrap();
        for slot in &mut cache.slots {
            slot.payload = Some(());
        }
        cache.update(Some(bounds(100.0))).unwrap();
        assert!(cache.ready());
        assert_eq!(cache.resident_bytes(), 32);
        assert_eq!(cache.over_budget_bytes(), 16);
        assert_eq!(cache.evictions, 0);
    }
    #[test]
    fn visible_requests_precede_neighbor_prefetch_without_reordering_storage() {
        let mut cache = ResidencyCache::<()>::new(2, 1, 8, 8, |range| {
            Some(bounds(if range.start == 0 { 2.1 } else { 0.5 }))
        })
        .unwrap();
        let view = Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(2.0, 2.0),
        };
        cache.update(Some(view)).unwrap();
        assert_eq!(cache.pending(10).collect::<Vec<_>>(), [1, 0]);
        cache.slots[1].payload = Some(());
        assert!(!cache.can_prefetch(0));
        assert_eq!(cache.slots[0].range, 0..1);
        assert_eq!(cache.slots[1].range, 1..2);
    }
}
