//! Cached source-size filtering shared by native backends, without rewriting geometry.
use super::{pad_visibility::Accepted, trace::InstanceSource};
use pomelo_core::{
    display::filter::{FilterKind, ObjectFilter},
    model::Bounds,
};
use std::{collections::BTreeMap, ops::Range};
#[derive(Default)]
pub(super) struct SourceFilter {
    sizes: Option<BTreeMap<(FilterKind, u64), f64>>,
    accepted: Option<((ObjectFilter, u64), Accepted)>,
}
impl SourceFilter {
    pub fn visible<S: InstanceSource>(
        &mut self,
        source: &S,
        filter: ObjectFilter,
        scale: f64,
        range: Range<usize>,
        emit: impl FnMut(Range<usize>) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let key = (filter, scale.to_bits());
        if self.accepted.as_ref().is_none_or(|(old, _)| *old != key) {
            if self.sizes.is_none() {
                let mut bounds = BTreeMap::<(FilterKind, u64), Bounds>::new();
                for i in 0..source.instances().len() {
                    if let Some(kind) = source.filter_kind(i)
                        && source.filter_extent(i).is_none()
                        && let Some(b) = source.bounds(i)
                    {
                        bounds
                            .entry((kind, source.filter_key(i)))
                            .and_modify(|old| {
                                old.include(b.min);
                                old.include(b.max);
                            })
                            .or_insert(b);
                    }
                }
                self.sizes = Some(
                    bounds
                        .into_iter()
                        .map(|(key, b)| (key, (b.max.x - b.min.x).min(b.max.y - b.min.y)))
                        .collect(),
                );
            }
            let sizes = self.sizes.as_ref();
            let accepted = Accepted::build((0..source.instances().len()).map(|i| {
                source.filter_kind(i).is_none_or(|kind| {
                    let size = source.filter_extent(i).or_else(|| {
                        sizes.and_then(|m| m.get(&(kind, source.filter_key(i))).copied())
                    });
                    let parent = source
                        .filter_parent(i)
                        .and_then(|key| sizes.and_then(|m| m.get(&key).map(|size| (key.0, *size))));
                    parent.is_none_or(|(kind, size)| filter.visible(kind, size, scale))
                        && size.is_none_or(|size| filter.visible(kind, size, scale))
                })
            }))?;
            self.accepted = Some((key, accepted));
        }
        self.accepted
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("GPU_OBJECT_FILTER_MISSING"))?
            .1
            .visible(range, emit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracks::{PreparedTracks, TraceLimits};
    use pomelo_core::{
        model::{LayerId, NetId, ObjectId, Point, Segment},
        task::CancellationToken,
    };
    #[test]
    fn owner_bounds_filter_whole_contours_and_recompute_after_zoom_or_category_change() {
        let line = |id, a, b| Segment {
            id: ObjectId(id),
            track_id: ObjectId(id),
            layer: LayerId(0),
            net: NetId(0),
            a,
            b,
            width: 0.2,
            arc: None,
            bond_wire: None,
        };
        let mut source = PreparedTracks::build(
            &[
                line(1, Point::new(0.0, 0.0), Point::new(10.0, 0.0)),
                line(1, Point::new(0.0, 5.0), Point::new(10.0, 5.0)),
                line(2, Point::new(1.0, 1.0), Point::new(2.0, 1.0)),
            ],
            TraceLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        for instance in &mut source.instances {
            instance.flags[3] |= 8;
        }
        let mut cache = SourceFilter::default();
        let mut filter = ObjectFilter {
            minimum_pixels: 2,
            ..Default::default()
        };
        let mut collect = |filter, scale| {
            let mut actual = Vec::new();
            cache
                .visible(&source, filter, scale, 0..3, |range| {
                    actual.extend(range);
                    Ok(())
                })
                .unwrap();
            actual
        };
        assert_eq!(collect(filter, 1.0), vec![0, 1]);
        assert!(collect(filter, 0.2).is_empty());
        assert_eq!(collect(filter, 1.0), vec![0, 1]);
        filter.copper = false;
        assert_eq!(collect(filter, 0.2), vec![0, 1, 2]);
    }
}

#[cfg(test)]
mod ring_tests {
    use super::*;
    use crate::tracks::{PreparedTracks, TraceLimits};
    use pomelo_core::{
        copper::{CopperMesh, MeshLimits},
        model::{LayerId, NetId, ObjectId, Point, Zone, ZoneKind},
        task::CancellationToken,
    };
    #[test]
    fn each_void_has_its_own_filter_extent_and_respects_parent_copper() {
        let rect = |x: f64, y: f64, w: f64| {
            vec![
                Point::new(x, y),
                Point::new(x + w, y),
                Point::new(x + w, y + w),
                Point::new(x, y + w),
            ]
        };
        let cancel = CancellationToken::default();
        let mesh = CopperMesh::build(
            &[
                rect(-5.0, -5.0, 10.0),
                rect(-3.0, -3.0, 0.5),
                rect(1.0, 1.0, 3.0),
            ],
            &[],
            &MeshLimits::default(),
            &cancel,
        )
        .unwrap();
        let source = PreparedTracks::build_zone_outlines(
            &[Zone {
                id: ObjectId(7),
                layer: LayerId(0),
                net: NetId(0),
                kind: ZoneKind::Dynamic,
                paths: vec![],
                mesh,
            }],
            TraceLimits::default(),
            &cancel,
        )
        .unwrap();
        let mut cache = SourceFilter::default();
        let mut filter = ObjectFilter {
            minimum_pixels: 2,
            ..Default::default()
        };
        let mut collect = |filter| {
            let mut n = 0;
            cache
                .visible(&source, filter, 1.0, 0..source.instances.len(), |r| {
                    n += r.len();
                    Ok(())
                })
                .unwrap();
            n
        };
        assert_eq!(
            collect(filter),
            8,
            "outer and large hole retained, small hole omitted"
        );
        filter.voids = false;
        assert_eq!(collect(filter), 12);
        filter.minimum_pixels = 11;
        assert_eq!(
            collect(filter),
            0,
            "hiding parent copper also hides its void edges"
        );
    }
}
