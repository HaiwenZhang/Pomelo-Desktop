//! Computed copper, hatch strokes, rotated copper rectangles and stored outlines.

use super::{
    CacheBudget, CacheLimits,
    connectivity::NetworkMap,
    geometry::{GeometryDecoder, GeometryLimits},
};
use crate::{
    ImportContext, ImportError,
    allegro::{
        database::{BrdDatabase, ChainLimits, ChainRequest, ReferenceLocation},
        decoder::{DecodedRecord, fixed::FixedRecord},
        index::RecordKey,
    },
};
use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    i18n::{Message, MessageKey},
    model::{Diagnostic, LayerId, NetId, ObjectId, Point, Segment, Severity, Zone},
};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct CopperLimits {
    pub geometry: GeometryLimits,
    pub mesh: MeshLimits,
    pub objects: CacheLimits,
    pub chain: ChainLimits,
}
impl Default for CopperLimits {
    fn default() -> Self {
        Self {
            geometry: GeometryLimits::default(),
            mesh: MeshLimits::default(),
            objects: CacheLimits {
                max_bytes: 2 * 1024 * 1024 * 1024,
                max_entries: 8_000_000,
            },
            chain: ChainLimits::default(),
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct CopperObject {
    pub zone: Option<Zone>,
    pub segments: Vec<Segment>,
    pub outline: Vec<Segment>,
}

/// Build-owned accounting includes all returned objects, even after a query transfers ownership.
pub struct CopperDecoder<'a> {
    database: &'a BrdDatabase,
    networks: &'a NetworkMap,
    layers: u32,
    limits: CopperLimits,
    budget: CacheBudget,
    diagnostics: Vec<Diagnostic>,
}
impl<'a> CopperDecoder<'a> {
    pub fn new(
        database: &'a BrdDatabase,
        networks: &'a NetworkMap,
        layers: u32,
        limits: CopperLimits,
    ) -> Result<Self, ImportError> {
        if layers == 0 || layers > 256 {
            return Err(ImportError::InvalidRecord {
                offset: 0,
                field: "COPPER_LAYER_COUNT",
                value: layers as u64,
            });
        }
        Ok(Self {
            database,
            networks,
            layers,
            budget: CacheBudget::new(limits.objects.clone()),
            limits,
            diagnostics: Vec::new(),
        })
    }

    pub fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }

    pub(super) fn output_bytes(&self) -> usize {
        self.budget.bytes
    }
    pub(super) fn bound_next_output(&mut self, bytes: usize) {
        self.budget.limits.max_bytes = self
            .limits
            .objects
            .max_bytes
            .min(self.budget.bytes.saturating_add(bytes));
    }

    fn geometry(&self) -> Result<GeometryDecoder<'a>, ImportError> {
        let mut limits = self.limits.geometry.clone();
        limits.max_allocation_bytes = limits.max_allocation_bytes.min(self.budget.available());
        GeometryDecoder::new(self.database, limits)
    }

    /// Only computed copper assigned by a source net chain is filled.
    pub fn shape(
        &mut self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<CopperObject, ImportError> {
        let database = self.database;
        let located = database.require_record(key, &[0x28], self.origin(key, "Shape"), context)?;
        let offset = located.span.offset.0 as usize;
        let DecodedRecord::Fixed(FixedRecord::Shape(shape)) = located.fields else {
            return Err(ImportError::InvalidRecord {
                offset,
                field: "COPPER_SHAPE_TYPE",
                value: 0x28,
            });
        };
        let class = shape.layer & 255;
        let layer = LayerId(u32::from(shape.layer >> 8));
        if class == 1 && [0xea, 0xfd].contains(&layer.0) {
            let outline = self.geometry()?.read_path(
                RecordKey(shape.first_segment_ptr),
                false,
                Some(key),
                self.origin(key, "FirstSegmentPtr"),
                context,
            )?;
            self.retain_path(&outline, offset)?;
            return Ok(CopperObject {
                outline,
                ..CopperObject::default()
            });
        }
        let Some(net) = self
            .networks
            .owner(key)
            .filter(|_| class == 6 && layer.0 < self.layers)
        else {
            return Ok(CopperObject::default());
        };
        if shape.unknown2.unwrap_or(0) & 255 == 2 {
            let mut output = CopperObject::default();
            self.hatch_path(
                RecordKey(shape.first_segment_ptr),
                key,
                layer,
                net,
                &mut output.segments,
                self.origin(key, "FirstSegmentPtr"),
                context,
            )?;
            for (first, kind, field) in [
                (shape.unknown4, 0x20, "Unknown4"),
                (shape.first_keepout_ptr, 0x34, "FirstKeepoutPtr"),
            ] {
                database.walk_chain(
                    &ChainRequest {
                        start: RecordKey(first),
                        terminator: key,
                        expected_types: &[kind],
                        origin: self.origin(key, field),
                        link_field: "Next",
                        limits: self.limits.chain.clone(),
                    },
                    context,
                    |record| Ok(record.fields.next_key().unwrap_or(RecordKey(0))),
                    |record| {
                        let first = match &record.fields {
                            DecodedRecord::Fixed(FixedRecord::UnknownRecord0x20(hatch)) => {
                                hatch.unknown_array1.first().copied().unwrap_or(0)
                            }
                            DecodedRecord::Fixed(FixedRecord::Keepout(hole)) => {
                                hole.first_segment_ptr
                            }
                            _ => {
                                return Err(ImportError::InvalidRecord {
                                    offset: record.span.offset.0 as usize,
                                    field: "COPPER_HATCH_TYPE",
                                    value: kind as u64,
                                });
                            }
                        };
                        self.hatch_path(
                            RecordKey(first),
                            key,
                            layer,
                            net,
                            &mut output.segments,
                            ReferenceLocation {
                                offset: record.span.offset,
                                field: "FirstSegmentPtr",
                            },
                            context,
                        )
                    },
                )?;
            }
            context.check_cancelled()?;
            return Ok(output);
        }
        let contours = self.geometry()?.read_contours(key, context)?;
        if contours.rings.is_empty() {
            self.budget.check(1024, offset)?;
            self.diagnostics.push(Diagnostic {
                code: "BRD_COPPER_BOUNDARY_EMPTY".into(),
                severity: Severity::Warning,
                message: Message::new(MessageKey::CopperBoundaryEmpty).arg("key", key.0),
                offset: Some(offset as u64),
                object: Some(ObjectId(key.0)),
                path: None,
                technical_details: None,
            });
            self.budget.commit(1024);
            return Ok(CopperObject::default());
        }
        let zone = self.zone(
            key,
            layer,
            net,
            contours.rings,
            contours.paths,
            offset,
            context,
        )?;
        Ok(CopperObject {
            zone: Some(zone),
            ..CopperObject::default()
        })
    }

    /// Rectangle rotations are about the first stored corner, not the rectangle centre.
    pub fn rectangle(
        &mut self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Option<Zone>, ImportError> {
        let located = self.database.require_record(
            key,
            &[0x0e, 0x24],
            self.origin(key, "CopperRectangle"),
            context,
        )?;
        let offset = located.span.offset.0 as usize;
        let (raw_layer, coords, rotation) = match located.fields {
            DecodedRecord::Fixed(FixedRecord::Rectangle(rect)) => {
                (rect.layer, rect.coords, rect.rotation)
            }
            DecodedRecord::Fixed(FixedRecord::FootprintRectangle(rect)) => {
                (rect.layer, rect.coords, rect.rotation)
            }
            _ => {
                return Err(ImportError::InvalidRecord {
                    offset,
                    field: "COPPER_RECTANGLE_TYPE",
                    value: located.span.record_type as u64,
                });
            }
        };
        let Some(net) = self.networks.owner(key).filter(|_| raw_layer & 255 == 6) else {
            return Ok(None);
        };
        let layer = LayerId(u32::from(raw_layer >> 8));
        if layer.0 >= self.layers {
            return Err(ImportError::CopperLayerUndefined {
                key: key.0,
                layer: layer.0,
                offset,
            });
        }
        let [x, y, u, v] = coords.as_slice() else {
            return Err(ImportError::InvalidGeometry {
                key: key.0,
                offset,
                field: "COPPER_RECTANGLE_COORDS",
            });
        };
        let angle = f64::from(rotation) * std::f64::consts::PI / 180000.0;
        let (s, c) = angle.sin_cos();
        let scale = self.geometry()?.scale();
        let dx = f64::from(*u) - f64::from(*x);
        let dy = f64::from(*v) - f64::from(*y);
        let corners: Vec<_> = [(0.0, 0.0), (dx, 0.0), (dx, dy), (0.0, dy)]
            .into_iter()
            .map(|(a, b)| {
                Point::new(
                    (f64::from(*x) + a * c - b * s) * scale,
                    (f64::from(*y) + a * s + b * c) * scale,
                )
            })
            .collect();
        let path = corners
            .iter()
            .copied()
            .enumerate()
            .map(|(index, a)| Segment {
                id: ObjectId(key.0),
                track_id: ObjectId(key.0),
                layer,
                net,
                a,
                b: corners[(index + 1) % corners.len()],
                width: 0.0,
                arc: None,
                bond_wire: None,
            })
            .collect();
        self.zone(key, layer, net, vec![corners], vec![path], offset, context)
            .map(Some)
    }

    pub fn graphic_outline(
        &mut self,
        key: RecordKey,
        context: &ImportContext<'_>,
    ) -> Result<Vec<Segment>, ImportError> {
        let record =
            self.database
                .require_record(key, &[0x14], self.origin(key, "Graphic"), context)?;
        let DecodedRecord::Fixed(FixedRecord::Graphic(graphic)) = record.fields else {
            return Ok(Vec::new());
        };
        if graphic.layer & 255 != 1 || ![0xea, 0xfd].contains(&(graphic.layer >> 8)) {
            return Ok(Vec::new());
        }
        let path = self.geometry()?.read_path(
            RecordKey(graphic.segment_ptr),
            false,
            Some(key),
            self.origin(key, "SegmentPtr"),
            context,
        )?;
        self.retain_path(&path, record.span.offset.0 as usize)?;
        Ok(path)
    }

    fn origin(&self, key: RecordKey, field: &'static str) -> ReferenceLocation {
        ReferenceLocation {
            offset: self
                .database
                .index()
                .record(key)
                .map_or(crate::allegro::index::FileOffset(0), |span| span.offset),
            field,
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Keep source identity, geometry ownership and import context explicit at the mesh boundary"
    )]
    fn zone(
        &mut self,
        key: RecordKey,
        layer: LayerId,
        net: NetId,
        rings: Vec<Vec<Point>>,
        paths: Vec<Vec<Segment>>,
        offset: usize,
        context: &ImportContext<'_>,
    ) -> Result<Zone, ImportError> {
        let paths_bytes = paths.capacity() * size_of::<Vec<Segment>>()
            + paths
                .iter()
                .map(|path| path.capacity() * size_of::<Segment>())
                .sum::<usize>();
        let input_bytes = paths_bytes
            .saturating_add(rings.capacity() * size_of::<Vec<Point>>())
            .saturating_add(
                rings
                    .iter()
                    .map(|ring| ring.capacity() * size_of::<Point>())
                    .sum::<usize>(),
            );
        self.budget.check(input_bytes, offset)?;
        let mut limits = self.limits.mesh.clone();
        limits.max_allocation_bytes = limits
            .max_allocation_bytes
            .min(self.budget.available().saturating_sub(input_bytes));
        let mesh =
            CopperMesh::build(&rings, &paths, &limits, context.cancellation).map_err(|source| {
                ImportError::CopperMesh {
                    key: key.0,
                    offset,
                    source,
                }
            })?;
        let charge = paths_bytes
            .saturating_add(mesh.allocation_bytes())
            .saturating_add(4 * size_of::<Zone>());
        self.budget.check(charge, offset)?;
        self.budget.commit(charge);
        Ok(Zone {
            id: ObjectId(key.0),
            layer,
            net,
            paths,
            mesh,
        })
    }

    fn retain_path(&mut self, path: &[Segment], offset: usize) -> Result<(), ImportError> {
        let charge = path
            .len()
            .saturating_mul(2 * size_of::<Segment>())
            .saturating_add(4 * size_of::<Segment>());
        self.budget.check(charge, offset)?;
        self.budget.commit(charge);
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "A hatch path carries its owning shape, assigned layer/net and referring source location"
    )]
    fn hatch_path(
        &mut self,
        first: RecordKey,
        owner: RecordKey,
        layer: LayerId,
        net: NetId,
        target: &mut Vec<Segment>,
        origin: ReferenceLocation,
        context: &ImportContext<'_>,
    ) -> Result<(), ImportError> {
        let mut path = self
            .geometry()?
            .read_path(first, true, Some(owner), origin, context)?;
        self.retain_path(&path, origin.offset.0 as usize)?;
        for edge in &mut path {
            edge.layer = layer;
            edge.net = net;
            edge.track_id = ObjectId(owner.0);
        }
        target.extend(path);
        Ok(())
    }
}
