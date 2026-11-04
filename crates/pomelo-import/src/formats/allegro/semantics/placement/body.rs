//! Source placement areas are already placed: apply rectangle rotation once, never footprint transforms again.
use super::*;
use crate::formats::allegro::semantics::geometry::GeometryDecoder;
use pomelo_core::model::{ComponentBody, ComponentRegion, Segment};
use std::mem::size_of;

impl PlacementDecoder<'_> {
    pub(super) fn component_body(
        &mut self,
        fp: &FootprintInstance,
        context: &ImportContext<'_>,
    ) -> Result<Option<ComponentBody>, ImportError> {
        let origin = self
            .database
            .index()
            .record(fp.key)
            .map_or(super::super::super::index::FileOffset(0), |r| r.offset);
        let mut candidates = Vec::new();
        let mut score = u8::MAX;
        let bottom = fp.layer != 0;
        self.database.walk_chain(
            &ChainRequest {
                start: RecordKey(fp.areas_ptr),
                terminator: fp.key,
                expected_types: &[0x0e, 0x28],
                origin: ReferenceLocation {
                    offset: origin,
                    field: "AreasPtr",
                },
                link_field: "Next",
                limits: self.chain.clone(),
            },
            context,
            |record| Ok(record.fields.next_key().unwrap_or(RecordKey(0))),
            |record| {
                let raw = match &record.fields {
                    DecodedRecord::Fixed(FixedRecord::FootprintRectangle(r)) => r.layer,
                    DecodedRecord::Fixed(FixedRecord::Shape(r)) => r.layer,
                    _ => return Ok(()),
                };
                let Some(priority) = placement_priority(raw, bottom) else {
                    return Ok(());
                };
                if priority < score {
                    candidates.clear();
                    score = priority;
                }
                if priority == score {
                    self.budget.check(
                        (candidates.len() + 1) * size_of::<(RecordKey, u16)>(),
                        record.span.offset.0 as usize,
                    )?;
                    candidates.push((record.span.key, raw));
                }
                Ok(())
            },
        )?;
        if candidates.is_empty() {
            return Ok(None);
        }
        let mut regions = Vec::new();
        for (key, raw) in candidates {
            context.check_cancelled()?;
            let record = self.database.require_record(
                key,
                &[0x0e, 0x28],
                ReferenceLocation {
                    offset: origin,
                    field: "PlacementArea",
                },
                context,
            )?;
            let offset = record.span.offset.0 as usize;
            let mut limits = self.geometry_limits.clone();
            limits.max_allocation_bytes = limits.max_allocation_bytes.min(self.budget.available());
            let geometry = GeometryDecoder::new(self.database, limits)?;
            let paths = match record.fields {
                DecodedRecord::Fixed(FixedRecord::FootprintRectangle(r)) => {
                    let [x, y, u, v] = r.coords.as_slice() else {
                        return Err(ImportError::InvalidGeometry {
                            key: key.0,
                            offset,
                            field: "PLACEMENT_RECTANGLE_COORDS",
                        });
                    };
                    let corners = rectangle_corners([*x, *y, *u, *v], r.rotation, geometry.scale());
                    vec![
                        (0..4)
                            .map(|i| Segment {
                                id: ObjectId(key.0),
                                track_id: ObjectId(key.0),
                                layer: LayerId::allegro_drawing(raw),
                                net: NetId(0),
                                a: corners[i],
                                b: corners[(i + 1) % 4],
                                width: 0.0,
                                arc: None,
                                bond_wire: None,
                            })
                            .collect(),
                    ]
                }
                DecodedRecord::Fixed(FixedRecord::Shape(_)) => {
                    geometry.read_shape_paths(key, context)?
                }
                _ => continue,
            };
            if paths.first().is_none_or(Vec::is_empty) {
                continue;
            }
            let bytes = size_of::<ComponentRegion>() * 2
                + paths.capacity() * size_of::<Vec<Segment>>()
                + paths
                    .iter()
                    .map(|p| p.capacity() * size_of::<Segment>())
                    .sum::<usize>();
            self.budget.check(bytes, offset)?;

            regions.push(ComponentRegion {
                source: ObjectId(key.0),
                source_layer: LayerId::allegro_drawing(raw),
                paths,
            });
            self.budget.commit(bytes);
        }
        Ok((!regions.is_empty()).then_some(ComponentBody {
            layer: LayerId(if bottom { self.layers - 1 } else { 0 }),
            regions,
        }))
    }
}

fn placement_priority(raw: u16, bottom: bool) -> Option<u8> {
    if raw & 255 != 9 {
        return None;
    }
    match (raw >> 8, bottom) {
        (251, false) | (250, true) => Some(0),
        (239, false) | (238, true) => Some(1),
        _ => None,
    }
}

fn rectangle_corners([x, y, u, v]: [i32; 4], rotation: u32, scale: f64) -> [Point; 4] {
    let (sin, cos) = super::rotation(rotation).sin_cos();
    let dx = f64::from(u) - f64::from(x);
    let dy = f64::from(v) - f64::from(y);
    [(0.0, 0.0), (dx, 0.0), (dx, dy), (0.0, dy)].map(|(a, b)| {
        Point::new(
            (f64::from(x) + a * cos - b * sin) * scale,
            (f64::from(y) + a * sin + b * cos) * scale,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rectangles_rotate_about_the_stored_first_corner_only_once() {
        let points = rectangle_corners([1108939, 132584, 1124639, 155584], 270000, 1e-4);
        let center = Point::new(
            points.iter().map(|p| p.x).sum::<f64>() / 4.0,
            points.iter().map(|p| p.y).sum::<f64>() / 4.0,
        );
        assert!((center.x - 112.0439).abs() < 1e-8);
        assert!((center.y - 12.4734).abs() < 1e-8);
        assert!(points.iter().all(|p| p.is_finite()));
    }
    #[test]
    fn placement_body_roles_use_source_codes_and_sides_instead_of_names() {
        assert_eq!(placement_priority(0xfb09, false), Some(0));
        assert_eq!(placement_priority(0xfa09, true), Some(0));
        assert_eq!(placement_priority(0xef09, false), Some(1));
        assert_eq!(placement_priority(0xee09, true), Some(1));
        for raw in [0xfb09, 0xef09, 0xfd09, 0x0006] {
            assert_eq!(placement_priority(raw, true), None);
        }
    }
}
