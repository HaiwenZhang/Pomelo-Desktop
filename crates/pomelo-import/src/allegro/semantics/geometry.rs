//! Source paths, signed arcs, hatch rounding, and copper contours with bounded traversal.

use super::{
    super::{
        database::{BrdDatabase, LocatedRecord, ReferenceLocation},
        decoder::{DecodedRecord, fixed::FixedRecord},
        index::RecordKey,
    },
    units::millimetres_per_unit,
};
use crate::{ImportContext, ImportError};
use pomelo_core::{
    geometry::{COPPER_CHORD_TOLERANCE_MM, PathError, arc_sweep, flatten_path},
    model::{Arc, LayerId, NetId, ObjectId, Point, Segment},
};
use serde::Serialize;
use std::{collections::HashSet, mem::size_of};

#[derive(Debug, Clone)]
pub struct GeometryLimits {
    /// Conservative accounting for owned paths, points, and traversal sets, including growth.
    pub max_allocation_bytes: usize,
    pub max_edges: usize,
    pub max_paths: usize,
    pub max_points: usize,
}
impl Default for GeometryLimits {
    fn default() -> Self {
        Self {
            max_allocation_bytes: 512 * 1024 * 1024,
            max_edges: 1_000_000,
            max_paths: 65_536,
            max_points: 8_000_000,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Contours {
    pub paths: Vec<Vec<Segment>>,
    pub rings: Vec<Vec<Point>>,
}

pub struct GeometryDecoder<'a> {
    database: &'a BrdDatabase,
    scale: f64,
    limits: GeometryLimits,
}

impl<'a> GeometryDecoder<'a> {
    pub fn new(database: &'a BrdDatabase, limits: GeometryLimits) -> Result<Self, ImportError> {
        let scale = millimetres_per_unit(database.header().units, database.header().divisor)?;
        Ok(Self {
            database,
            scale,
            limits,
        })
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// Paths may close by linking to their first edge. Other cycles are invalid.
    /// Owner and header sentinel links terminate without decoding their target.
    pub fn read_path(
        &self,
        first: RecordKey,
        hatch: bool,
        owner: Option<RecordKey>,
        origin: ReferenceLocation,
        context: &ImportContext<'_>,
    ) -> Result<Vec<Segment>, ImportError> {
        self.path(
            first,
            hatch,
            owner,
            origin,
            &mut Budget::new(&self.limits),
            context,
        )
    }

    pub fn read_shape_paths(
        &self,
        id: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Vec<Vec<Segment>>, ImportError> {
        self.shape_paths(id, &mut Budget::new(&self.limits), context)
    }

    pub fn read_contours(
        &self,
        id: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Contours, ImportError> {
        let mut budget = Budget::new(&self.limits);
        let paths = self.shape_paths(id, &mut budget, context)?;
        let mut result = Contours {
            paths: Vec::new(),
            rings: Vec::new(),
        };
        let offset = self
            .database
            .index()
            .record(id)
            .map_or(0, |span| span.offset.0 as usize);
        let mut point_count = 0_usize;
        for path in paths {
            context.check_cancelled()?;
            let available_bytes = self
                .limits
                .max_allocation_bytes
                .saturating_sub(budget.bytes);
            let limit = self
                .limits
                .max_points
                .saturating_sub(point_count)
                .min(available_bytes / (2 * size_of::<Point>()));
            let ring = flatten_path(
                &path,
                COPPER_CHORD_TOLERANCE_MM,
                limit,
                context.cancellation,
            )
            .map_err(|error| match error {
                PathError::Cancelled => ImportError::Cancelled,
                PathError::Invalid(key) => ImportError::InvalidGeometry {
                    key: key.0,
                    offset: self
                        .database
                        .index()
                        .record(RecordKey(key.0))
                        .map_or(offset, |span| span.offset.0 as usize),
                    field: "COPPER_CONTOUR",
                },
                PathError::PointLimit { actual, limit } => ImportError::GeometryLimit {
                    offset,
                    actual: actual.saturating_mul(2 * size_of::<Point>()) as u64,
                    limit: limit.saturating_mul(2 * size_of::<Point>()) as u64,
                },
            })?;
            point_count = point_count.saturating_add(ring.len());
            budget.charge(ring.capacity().saturating_mul(size_of::<Point>()), offset)?;
            if ring.len() >= 3 {
                result.paths.push(path);
                result.rings.push(ring);
            }
        }
        context.check_cancelled()?;
        Ok(result)
    }

    fn shape_paths(
        &self,
        id: RecordKey,
        budget: &mut Budget<'_>,
        context: &ImportContext<'_>,
    ) -> Result<Vec<Vec<Segment>>, ImportError> {
        context.check_cancelled()?;
        if self
            .database
            .index()
            .record(id)
            .is_none_or(|span| span.record_type != 0x28)
        {
            return Ok(Vec::new());
        }
        let Some(record) = self.database.get(id, context)? else {
            return Ok(Vec::new());
        };
        let DecodedRecord::Fixed(FixedRecord::Shape(shape)) = record.fields else {
            return Ok(Vec::new());
        };
        let mut paths = Vec::new();
        budget.path(record.span.offset.0 as usize)?;
        let outer = self.path(
            RecordKey(shape.first_segment_ptr),
            false,
            Some(id),
            ReferenceLocation {
                offset: record.span.offset,
                field: "FirstSegmentPtr",
            },
            budget,
            context,
        )?;
        if !outer.is_empty() {
            paths.push(outer);
        }
        let mut key = RecordKey(shape.first_keepout_ptr);
        let mut origin = ReferenceLocation {
            offset: record.span.offset,
            field: "FirstKeepoutPtr",
        };
        let mut seen = HashSet::new();
        while key.0 != 0 && key != id && !self.database.header().sentinel_keys.contains(&key.0) {
            context.check_cancelled()?;
            if seen.contains(&key) {
                return Err(ImportError::ReferenceCycle {
                    key: key.0,
                    offset: origin.offset.0 as usize,
                    field: origin.field,
                });
            }
            let span = self
                .database
                .index()
                .record(key)
                .ok_or(ImportError::MissingReference {
                    key: key.0,
                    offset: origin.offset.0 as usize,
                    field: origin.field,
                })?;
            if span.record_type != 0x34 {
                break;
            }
            budget.path(origin.offset.0 as usize)?;
            let hole = self
                .database
                .require_record(key, &[0x34], origin, context)?;
            let DecodedRecord::Fixed(FixedRecord::Keepout(fields)) = hole.fields else {
                break;
            };
            seen.insert(key);
            let path = self.path(
                RecordKey(fields.first_segment_ptr),
                false,
                Some(key),
                ReferenceLocation {
                    offset: hole.span.offset,
                    field: "FirstSegmentPtr",
                },
                budget,
                context,
            )?;
            if !path.is_empty() {
                paths.push(path);
            }
            key = RecordKey(fields.next);
            origin = ReferenceLocation {
                offset: hole.span.offset,
                field: "Next",
            };
        }
        context.check_cancelled()?;
        Ok(paths)
    }

    fn path(
        &self,
        first: RecordKey,
        hatch: bool,
        owner: Option<RecordKey>,
        mut origin: ReferenceLocation,
        budget: &mut Budget<'_>,
        context: &ImportContext<'_>,
    ) -> Result<Vec<Segment>, ImportError> {
        context.check_cancelled()?;
        let mut key = first;
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        while key.0 != 0
            && Some(key) != owner
            && !self.database.header().sentinel_keys.contains(&key.0)
        {
            context.check_cancelled()?;
            if seen.contains(&key) {
                if key == first {
                    break;
                }
                return Err(ImportError::ReferenceCycle {
                    key: key.0,
                    offset: origin.offset.0 as usize,
                    field: origin.field,
                });
            }
            let span = self
                .database
                .index()
                .record(key)
                .ok_or(ImportError::MissingReference {
                    key: key.0,
                    offset: origin.offset.0 as usize,
                    field: origin.field,
                })?;
            // Stored lists also end at non-edge owner records; avoid allocating those payloads.
            if ![1, 0x15, 0x16, 0x17].contains(&span.record_type) {
                break;
            }
            budget.edge(origin.offset.0 as usize)?;
            let record =
                self.database
                    .require_record(key, &[1, 0x15, 0x16, 0x17], origin, context)?;
            let next = match &record.fields {
                DecodedRecord::Fixed(FixedRecord::Arc(record)) => record.next,
                DecodedRecord::Fixed(FixedRecord::Segment(record)) => record.next,
                _ => {
                    return Err(ImportError::InvalidRecord {
                        offset: span.offset.0 as usize,
                        field: "PATH_EDGE",
                        value: u64::from(key.0),
                    });
                }
            };
            result.push(decode_edge(&record, self.scale, hatch)?);
            seen.insert(key);
            key = RecordKey(next);
            origin = ReferenceLocation {
                offset: record.span.offset,
                field: "Next",
            };
        }
        context.check_cancelled()?;
        Ok(result)
    }
}

/// Source Radius is deliberately ignored: stored endpoints define the displayed circle.
pub fn decode_edge(
    record: &LocatedRecord,
    scale: f64,
    hatch: bool,
) -> Result<Segment, ImportError> {
    let invalid = |field| ImportError::InvalidGeometry {
        key: record.span.key.0,
        offset: record.span.offset.0 as usize,
        field,
    };
    if !scale.is_finite() || scale <= 0.0 {
        return Err(invalid("SCALE"));
    }
    let (start_x, start_y, end_x, end_y, width) = match &record.fields {
        DecodedRecord::Fixed(FixedRecord::Arc(edge)) => (
            edge.start_x,
            edge.start_y,
            edge.end_x,
            edge.end_y,
            edge.width,
        ),
        DecodedRecord::Fixed(FixedRecord::Segment(edge)) => (
            edge.start_x,
            edge.start_y,
            edge.end_x,
            edge.end_y,
            edge.width,
        ),
        _ => {
            return Err(ImportError::ReferenceType {
                key: record.span.key.0,
                actual: record.span.record_type,
                expected: vec![1, 0x15, 0x16, 0x17],
                offset: record.span.offset.0 as usize,
                field: "PATH_EDGE",
            });
        }
    };
    let a = Point::new(f64::from(start_x) * scale, f64::from(start_y) * scale);
    let b = Point::new(f64::from(end_x) * scale, f64::from(end_y) * scale);
    let arc = if let DecodedRecord::Fixed(FixedRecord::Arc(edge)) = &record.fields {
        let grid = |value: f64| {
            if hatch {
                value.round_ties_even()
            } else {
                value
            }
        };
        let center = Point::new(grid(edge.center_x) * scale, grid(edge.center_y) * scale);
        if !center.x.is_finite() || !center.y.is_finite() {
            return Err(invalid("ARC_CENTER"));
        }
        let radius = if hatch {
            (a.distance(center) + b.distance(center)) / 2.0
        } else {
            a.distance(center)
        };
        let start = (a.y - center.y).atan2(a.x - center.x);
        let end = (b.y - center.y).atan2(b.x - center.x);
        Some(Arc {
            center,
            radius,
            start,
            sweep: arc_sweep(start, end, edge.sub_type & 64 != 0),
        })
    } else {
        None
    };
    if ![a.x, a.y, b.x, b.y, f64::from(width) * scale]
        .into_iter()
        .all(f64::is_finite)
        || arc.is_some_and(|arc| !arc.radius.is_finite())
    {
        return Err(invalid("PATH_COORDINATES"));
    }
    Ok(Segment {
        bond_wire: None,
        id: ObjectId(record.span.key.0),
        track_id: ObjectId(0),
        layer: LayerId::UNASSIGNED,
        net: NetId(0),
        a,
        b,
        width: f64::from(width) * scale,
        arc,
    })
}

struct Budget<'a> {
    limits: &'a GeometryLimits,
    bytes: usize,
    edges: usize,
    paths: usize,
}
impl<'a> Budget<'a> {
    fn new(limits: &'a GeometryLimits) -> Self {
        Self {
            limits,
            bytes: 0,
            edges: 0,
            paths: 0,
        }
    }
    fn charge(&mut self, bytes: usize, offset: usize) -> Result<(), ImportError> {
        let actual = self.bytes.saturating_add(bytes);
        if actual > self.limits.max_allocation_bytes {
            return Err(ImportError::GeometryLimit {
                offset,
                actual: actual as u64,
                limit: self.limits.max_allocation_bytes as u64,
            });
        }
        self.bytes = actual;
        Ok(())
    }
    fn edge(&mut self, offset: usize) -> Result<(), ImportError> {
        self.edges = self.edges.saturating_add(1);
        if self.edges > self.limits.max_edges {
            return Err(ImportError::InvalidRecord {
                offset,
                field: "GEOMETRY_EDGE_COUNT",
                value: self.edges as u64,
            });
        }
        // Small vectors start with four elements; include that minimum before the first push.
        self.charge(64 + 4 * size_of::<Segment>(), offset)
    }
    fn path(&mut self, offset: usize) -> Result<(), ImportError> {
        self.paths = self.paths.saturating_add(1);
        if self.paths > self.limits.max_paths {
            return Err(ImportError::InvalidRecord {
                offset,
                field: "GEOMETRY_PATH_COUNT",
                value: self.paths as u64,
            });
        }
        self.charge(64 + 4 * size_of::<Vec<Segment>>(), offset)
    }
}
