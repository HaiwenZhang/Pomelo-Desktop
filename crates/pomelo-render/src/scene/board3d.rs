//! Immutable, GPU-ready conductor geometry. Preview heights are explicit assumptions.
mod holes;
mod paging;
mod spatial;
use super::solid::{PadSolidSource, PreparedSolid, SolidPrepareError, ViaWallSpan};
use pomelo_core::{
    display::{BoardDisplay, DisplayCategory},
    geometry3d::{
        Bounds3,
        solid::{SolidError, SolidLimits},
    },
    interaction::{
        camera3d::PickRay3d,
        selection::{MemberLayers, SelectedObject},
    },
    model::{BoardScene, DrillShape, LayerFunction, LayerId, NetId, Point, Segment},
    pad::PadPlacement,
    picking::PickFilter,
    task::CancellationToken,
};
use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
};

#[derive(Debug, Clone, Copy)]
pub struct PreviewStackup {
    pub board_mm: f64,
    pub copper_mm: f64,
    pub plating_mm: f64,
    pub chord_mm: f64,
}
impl Default for PreviewStackup {
    fn default() -> Self {
        Self {
            board_mm: 1.6,
            copper_mm: 0.035,
            plating_mm: 0.025,
            chord_mm: 0.005,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex3d {
    pub position: [f32; 4],
    pub normal: [f32; 4],
    pub owner: [u32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance3d {
    /// Translation relative to board origin, then cosine/sine of the placement angle.
    pub transform: [f32; 4],
    /// [source ID, capsule flag, f32 end-extension bits, reserved]. Zero ID uses vertex ownership.
    pub owner: [u32; 4],
}
const _: () = assert!(size_of::<Vertex3d>() == 48 && size_of::<Instance3d>() == 32);

pub const PAGE_VERTICES: usize = 65_536;
pub const PAGE_INDICES: usize = 393_216;
pub const PAGE_INSTANCES: usize = 4_096;

pub struct Mesh3d {
    pub vertices: Vec<Vertex3d>,
    pub indices: Vec<u32>,
}
pub struct Page3d {
    pub mesh: usize,
    pub instances: Vec<Instance3d>,
    pub sources: Vec<u32>,
    pub bounds: Bounds3,
}
pub struct SourcePart3d {
    pub mesh: usize,
    pub indices: Range<u32>,
}
pub struct Source3d {
    pub object: SelectedObject,
    pub layers: MemberLayers,
    pub net: NetId,
    pub bounds: Bounds3,
    pub parts: Range<usize>,
    pub transform: [f32; 4],
    /// Capsule endpoints move by ±extension; rounded ends keep their radius.
    pub extension: f32,
}
pub struct PreparedBoard3d {
    pub(crate) memory_reservation: Option<pomelo_core::memory::MemoryReservation>,
    pub meshes: Vec<Mesh3d>,
    pub pages: Vec<Page3d>,
    pub parts: Vec<SourcePart3d>,
    source_index: spatial::SpatialIndex3d,
    page_index: spatial::SpatialIndex3d,
    /// Materials and vertex/instance ownership use one-based indices into this array.
    pub sources: Vec<Source3d>,
    pub origin: Point,
    pub bounds: Bounds3,
    pub layers: Vec<LayerId>,
    pub warnings: Vec<SolidPrepareError>,
    pub warning_count: usize,
    pub preview: PreviewStackup,
}

impl PreparedBoard3d {
    pub fn preview(
        board: &BoardScene,
        preview: PreviewStackup,
        mut limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        let fail = SolidPrepareError::Scene;
        if ![
            preview.board_mm,
            preview.copper_mm,
            preview.plating_mm,
            preview.chord_mm,
        ]
        .into_iter()
        .all(|v| v.is_finite() && v > 0.0)
            || !board.bounds.is_valid()
        {
            return Err(fail(SolidError::Invalid("PREVIEW_STACKUP")));
        }
        let layers: Vec<_> = board
            .layers
            .iter()
            .filter(|l| l.id.is_copper() && l.function != LayerFunction::Dielectric)
            .map(|l| l.id)
            .collect();
        if layers.is_empty() || preview.board_mm < preview.copper_mm * layers.len() as f64 {
            return Err(fail(SolidError::Invalid("PREVIEW_LAYER_SPACING")));
        }
        let pitch = if layers.len() > 1 {
            (preview.board_mm - preview.copper_mm) / (layers.len() - 1) as f64
        } else {
            0.0
        };
        let heights: BTreeMap<_, _> = layers
            .iter()
            .enumerate()
            .map(|(i, &id)| {
                let top = preview.board_mm * 0.5 - pitch * i as f64;
                (id, [top - preview.copper_mm, top])
            })
            .collect();
        let count = holes::count(board);
        let index_bytes = spatial::SpatialIndex3d::estimate_bytes(count);
        let hole_limits = SolidLimits {
            max_allocation_bytes: limits.max_allocation_bytes.saturating_sub(index_bytes),
            ..limits
        };
        let holes = holes::build(board, hole_limits, cancel).map_err(fail)?;
        let hole_index =
            spatial::SpatialIndex3d::build(holes.len(), |id| holes::bounds(&holes[id]), cancel)?;
        limits.max_allocation_bytes = limits.max_allocation_bytes.saturating_sub(
            holes.capacity() * size_of::<holes::Hole>() + hole_index.allocation_bytes(),
        );
        let mut builder = Builder {
            origin: board.bounds.center(),
            groups: vec![],
            group_bytes: 0,
            instance_bytes: 0,
            static_groups: BTreeMap::new(),
            cache: BTreeMap::new(),
            sources: vec![],
            parts: vec![],
            tile_mm: ((board.bounds.max.x - board.bounds.min.x)
                .max(board.bounds.max.y - board.bounds.min.y)
                / 8.0)
                .clamp(4.0, 64.0),
            warnings: vec![],
            warning_count: 0,
            limits,
            cancel,
        };
        for segment in &board.segments {
            cancel.check().map_err(|e| fail(e.into()))?;
            if let Some(&z) = heights.get(&segment.layer) {
                let remaining = builder.remaining()?;
                let cuts = holes::cutouts(
                    &holes,
                    &hole_index,
                    segment.layer,
                    segment.bounds(),
                    preview.chord_mm,
                    remaining,
                    cancel,
                )
                .map_err(fail)?;
                if cuts.is_empty() {
                    builder.add_trace(segment, z, preview.chord_mm)?;
                } else {
                    let remaining = SolidLimits {
                        max_allocation_bytes: remaining
                            .max_allocation_bytes
                            .saturating_sub(super::solid::ring_bytes(&cuts)),
                        ..remaining
                    };
                    let prepared = PreparedSolid::prepare_trace_with_cutouts(
                        segment,
                        z,
                        preview.chord_mm,
                        &cuts,
                        remaining,
                        cancel,
                    );
                    builder.add_static(prepared, segment.layer)?;
                }
            }
        }
        for zone in &board.zones {
            cancel.check().map_err(|e| fail(e.into()))?;
            if let Some(&z) = heights.get(&zone.layer) {
                let remaining = builder.remaining()?;
                let bounds = zone.mesh.ring_bounds.first().copied();
                let cuts = holes::cutouts(
                    &holes,
                    &hole_index,
                    zone.layer,
                    bounds,
                    preview.chord_mm,
                    remaining,
                    cancel,
                )
                .map_err(fail)?;
                let remaining = SolidLimits {
                    max_allocation_bytes: remaining
                        .max_allocation_bytes
                        .saturating_sub(super::solid::ring_bytes(&cuts)),
                    ..remaining
                };
                let prepared = PreparedSolid::prepare_zone_with_cutouts(
                    zone,
                    z,
                    preview.chord_mm,
                    &cuts,
                    remaining,
                    cancel,
                );
                builder.add_static(prepared, zone.layer)?;
            }
        }
        for pin in &board.pins {
            let placement = PadPlacement {
                at: pin.at,
                angle: pin.angle,
                mirrored: pin.mirrored,
            };
            for pad in &pin.pads {
                if let Some(&z) = heights.get(&pad.layer) {
                    builder.add_pad(
                        PadSolidSource {
                            object: SelectedObject::Pin(pin.id),
                            net: pin.net,
                            pad,
                            placement,
                            drill: pin.drill_shape.pad().map(|_| pin.drill_shape),
                        },
                        z,
                        preview.chord_mm,
                    )?;
                }
            }
        }
        for via in &board.vias {
            let placement = PadPlacement {
                at: via.at,
                angle: via.angle,
                mirrored: via.mirrored,
            };
            let span = via
                .start_layer
                .zip(via.end_layer)
                .and_then(|(a, b)| Some((a, b, *heights.get(&a)?, *heights.get(&b)?)));
            for pad in via.pads.iter() {
                if let Some(&z) = heights.get(&pad.layer) {
                    let drill = span
                        .filter(|(_, _, a, b)| z[0] >= a[0].min(b[0]) && z[1] <= a[1].max(b[1]))
                        .map(|_| via.drill_shape)
                        .filter(|d| d.pad().is_some());
                    builder.add_pad(
                        PadSolidSource {
                            object: SelectedObject::Via(via.id),
                            net: via.net,
                            pad,
                            placement,
                            drill,
                        },
                        z,
                        preview.chord_mm,
                    )?;
                }
            }
            if via.drill_shape.plated && via.drill_shape.pad().is_some() {
                if let Some((a, b, za, zb)) = span.filter(|_| via.backdrill.is_none()) {
                    let z = [za[0].min(zb[0]), za[1].max(zb[1])];
                    let mut key = [0_u64; 16];
                    key[..7].copy_from_slice(&[
                        2,
                        via.drill_shape.width.to_bits(),
                        via.drill_shape.height.to_bits(),
                        preview.plating_mm.to_bits(),
                        z[0].to_bits(),
                        z[1].to_bits(),
                        u64::from(via.drill_shape.rectangular),
                    ]);
                    let meta = Identity {
                        object: SelectedObject::Via(via.id),
                        layers: MemberLayers::Span(a, b),
                        net: via.net,
                        extension: 0.0,
                        placement: PadPlacement {
                            mirrored: false,
                            ..placement
                        },
                    };
                    builder.add_shared(key, meta, |limits| {
                        PreparedSolid::prepare_via_wall(
                            via,
                            ViaWallSpan {
                                z,
                                layers: [a, b],
                                plating_thickness_mm: preview.plating_mm,
                            },
                            preview.chord_mm,
                            limits,
                            cancel,
                        )
                    })?;
                } else {
                    builder.warning(SolidPrepareError::Geometry {
                        object: SelectedObject::Via(via.id),
                        source: SolidError::Unsupported("PREVIEW_VIA_SPAN_OR_BACKDRILL"),
                    })?;
                }
            }
        }
        builder.finish(layers, preview)
    }

    pub fn vertex_count(&self) -> usize {
        self.meshes.iter().map(|m| m.vertices.len()).sum()
    }
    pub fn index_count(&self) -> usize {
        self.meshes.iter().map(|m| m.indices.len()).sum()
    }
    pub fn instance_count(&self) -> usize {
        self.pages.iter().map(|p| p.instances.len()).sum()
    }
    pub fn upload_bytes(&self) -> usize {
        self.vertex_count() * size_of::<Vertex3d>()
            + self.index_count() * 4
            + self.instance_count() * size_of::<Instance3d>()
    }
    pub fn allocation_bytes(&self) -> usize {
        self.meshes.capacity() * size_of::<Mesh3d>()
            + self
                .meshes
                .iter()
                .map(|m| m.vertices.capacity() * size_of::<Vertex3d>() + m.indices.capacity() * 4)
                .sum::<usize>()
            + self.pages.capacity() * size_of::<Page3d>()
            + self
                .pages
                .iter()
                .map(|p| {
                    p.instances.capacity() * size_of::<Instance3d>() + p.sources.capacity() * 4
                })
                .sum::<usize>()
            + self.sources.capacity() * size_of::<Source3d>()
            + self.parts.capacity() * size_of::<SourcePart3d>()
            + self.source_index.allocation_bytes()
            + self.page_index.allocation_bytes()
            + self.layers.capacity() * size_of::<LayerId>()
            + self.warnings.capacity() * size_of::<SolidPrepareError>()
    }

    pub fn materials(
        &self,
        board: &BoardScene,
        display: &BoardDisplay,
        transparent: bool,
        selected: Option<SelectedObject>,
        selected_net: Option<NetId>,
    ) -> Vec<[f32; 4]> {
        let colors: BTreeMap<_, _> = board
            .layers
            .iter()
            .map(|l| (l.id, parse_color(&l.color)))
            .collect();
        let mut result = Vec::with_capacity(self.sources.len() + 1);
        result.push([0.0; 4]);
        for source in &self.sources {
            let scope: Vec<_> = match source.layers {
                MemberLayers::Single(l) => vec![l],
                MemberLayers::Span(a, b) => {
                    let range = self
                        .layers
                        .iter()
                        .position(|l| *l == a)
                        .zip(self.layers.iter().position(|l| *l == b));
                    range.map_or_else(Vec::new, |(a, b)| self.layers[a.min(b)..=a.max(b)].to_vec())
                }
                MemberLayers::None => vec![],
            };
            let Some(layer) = scope
                .into_iter()
                .find(|l| display.object_visible(*l, source.object))
            else {
                result.push([0.0; 4]);
                continue;
            };
            let category = match source.object {
                SelectedObject::Segment(_) => DisplayCategory::Trace,
                SelectedObject::Pin(_) => DisplayCategory::Pin,
                SelectedObject::Via(_) => DisplayCategory::Via,
                _ => DisplayCategory::Zone,
            };
            let base = display
                .appearance
                .material(layer, category)
                .unwrap_or_else(|| colors.get(&layer).copied().unwrap_or([0.8, 0.5, 0.15, 1.0]));
            let mut color = display
                .appearance
                .net_material(source.net, category)
                .map(|c| c.rgba())
                .unwrap_or_else(|| {
                    super::colors::copper_color(display.color_mode, base, source.net)
                });
            color[3] = if transparent { 0.22 } else { 1.0 };
            color[3] *= if matches!(source.object, SelectedObject::Zone(_)) {
                display.copper_opacity
            } else {
                display.global_opacity
            };
            if color[3] > 0.0
                && (selected == Some(source.object) || selected_net == Some(source.net))
            {
                color = [1.0, 0.35, 0.05, 1.0];
            }
            result.push(color);
        }
        result
    }

    pub fn visible_pages(
        &self,
        camera: pomelo_core::interaction::camera3d::Camera3d,
        viewport: [f64; 2],
        z_scale: f64,
        output: &mut Vec<usize>,
    ) -> Result<(), SolidError> {
        output.clear();
        if !z_scale.is_finite() || z_scale <= 0.0 {
            return Err(SolidError::Invalid("DISPLAY_Z_SCALE"));
        }
        output
            .try_reserve(self.pages.len())
            .map_err(|_| SolidError::Allocation)?;
        self.page_index
            .visit_frustum(camera, viewport, z_scale, |id| {
                let mut b = self.pages[id].bounds;
                b.min[2] *= z_scale;
                b.max[2] *= z_scale;
                if camera.intersects(b, viewport) {
                    output.push(id);
                }
            });
        Ok(())
    }

    pub fn pick(
        &self,
        ray: PickRay3d,
        materials: &[[f32; 4]],
        filter: PickFilter,
    ) -> Option<SelectedObject> {
        let mut nearest = ray.length;
        let mut hit = None;
        let mut hit_index = usize::MAX;
        self.source_index
            .visit_ray(ray, &mut nearest, |i, nearest| {
                let source = &self.sources[i];
                if !filter.contains(source.object.into())
                    || materials.get(i + 1).is_none_or(|m| m[3] <= 0.0)
                    || !ray_box(ray, source.bounds, *nearest)
                {
                    return;
                }
                let [tx, ty, c, s] = source.transform.map(f64::from);
                let x = ray.origin[0] - self.origin.x - tx;
                let y = ray.origin[1] - self.origin.y - ty;
                let origin = [c * x + s * y, -s * x + c * y, ray.origin[2]];
                let direction = [
                    c * ray.direction[0] + s * ray.direction[1],
                    -s * ray.direction[0] + c * ray.direction[1],
                    ray.direction[2],
                ];
                for part in &self.parts[source.parts.clone()] {
                    let mesh = &self.meshes[part.mesh];
                    for triangle in mesh.indices
                        [part.indices.start as usize..part.indices.end as usize]
                        .as_chunks::<3>()
                        .0
                    {
                        let points = triangle.map(|i| {
                            let mut p: [f32; 3] = mesh.vertices[i as usize].position[..3]
                                .try_into()
                                .unwrap_or([0.0; 3]);
                            p[0] += p[0].signum() * source.extension;
                            p
                        });
                        if let Some(t) = ray_triangle(origin, direction, points)
                            && (t < *nearest || (t == *nearest && i < hit_index))
                        {
                            *nearest = t;
                            hit_index = i;
                            hit = Some(source.object);
                        }
                    }
                }
            });
        hit
    }
}

struct Identity {
    object: SelectedObject,
    layers: MemberLayers,
    net: NetId,
    placement: PadPlacement,
    extension: f32,
}
struct Group {
    vertices: Vec<Vertex3d>,
    indices: Vec<u32>,
    instances: Vec<Instance3d>,
    sources: Vec<u32>,
    bounds: Option<Bounds3>,
    shared: bool,
}
impl Group {
    fn allocation_bytes(&self) -> usize {
        self.vertices.capacity() * size_of::<Vertex3d>()
            + self.indices.capacity() * 4
            + self.instances.capacity() * size_of::<Instance3d>()
            + self.sources.capacity() * 4
    }
}
struct Builder<'a> {
    origin: Point,
    group_bytes: usize,
    instance_bytes: usize,
    groups: Vec<Group>,
    static_groups: BTreeMap<(LayerId, [i64; 2]), usize>,
    cache: BTreeMap<[u64; 16], std::sync::Arc<[usize]>>,
    sources: Vec<Source3d>,
    parts: Vec<SourcePart3d>,
    tile_mm: f64,
    warnings: Vec<SolidPrepareError>,
    warning_count: usize,
    limits: SolidLimits,
    cancel: &'a CancellationToken,
}
impl Builder<'_> {
    fn error(source: SolidError) -> SolidPrepareError {
        SolidPrepareError::Scene(source)
    }
    fn bytes(&self) -> usize {
        self.group_bytes
            + self.sources.capacity() * size_of::<Source3d>()
            + self.parts.capacity() * size_of::<SourcePart3d>()
            + self.cache.len() * 256
            + self.groups.capacity() * size_of::<Group>()
            + self.warnings.capacity() * size_of::<SolidPrepareError>()
    }
    fn remaining(&self) -> Result<SolidLimits, SolidPrepareError> {
        self.cancel.check().map_err(|e| Self::error(e.into()))?;
        let bytes = self.bytes().saturating_add(self.instance_bytes);
        if bytes > self.limits.max_allocation_bytes {
            return Err(Self::error(SolidError::Limit {
                resource: "bytes",
                actual: bytes,
                limit: self.limits.max_allocation_bytes,
            }));
        }
        Ok(SolidLimits {
            max_allocation_bytes: self.limits.max_allocation_bytes - bytes,
            ..self.limits
        })
    }
    fn warning(&mut self, error: SolidPrepareError) -> Result<(), SolidPrepareError> {
        if matches!(
            &error,
            SolidPrepareError::Geometry {
                source: SolidError::Cancelled | SolidError::Allocation | SolidError::Limit { .. },
                ..
            } | SolidPrepareError::Curve {
                source: pomelo_core::geometry::PathError::Cancelled
                    | pomelo_core::geometry::PathError::PointLimit { .. },
                ..
            }
        ) {
            return Err(error);
        }
        self.warning_count += 1;
        if self.warnings.len() < 128 {
            self.warnings.push(error);
        }
        Ok(())
    }
    fn group(&mut self) -> Result<usize, SolidPrepareError> {
        self.groups
            .try_reserve(1)
            .map_err(|_| Self::error(SolidError::Allocation))?;
        self.groups.push(Group {
            vertices: vec![],
            indices: vec![],
            instances: vec![],
            sources: vec![],
            bounds: None,
            shared: false,
        });
        Ok(self.groups.len() - 1)
    }
    fn source(
        &mut self,
        meta: Identity,
        bounds: Bounds3,
        transform: [f32; 4],
    ) -> Result<u32, SolidPrepareError> {
        if !bounds.is_valid() || !transform.into_iter().all(f32::is_finite) {
            return Err(Self::error(SolidError::Invalid("SOURCE_BOUNDS")));
        }
        let id = u32::try_from(self.sources.len().saturating_add(1))
            .map_err(|_| Self::error(SolidError::Invalid("MATERIAL_ID_RANGE")))?;
        self.sources
            .try_reserve(1)
            .map_err(|_| Self::error(SolidError::Allocation))?;
        self.sources.push(Source3d {
            object: meta.object,
            layers: meta.layers,
            net: meta.net,
            bounds,
            transform,
            extension: meta.extension,
            parts: self.parts.len()..self.parts.len(),
        });
        Ok(id)
    }
    fn add_trace(
        &mut self,
        segment: &Segment,
        z: [f64; 2],
        tolerance: f64,
    ) -> Result<(), SolidPrepareError> {
        let length = segment.a.distance(segment.b);
        if segment.arc.is_some()
            || segment.bond_wire.is_some()
            || !length.is_finite()
            || length == 0.0
            || !segment.width.is_finite()
            || segment.width <= 0.0
        {
            let solid =
                PreparedSolid::prepare_trace(segment, z, tolerance, self.remaining()?, self.cancel);
            return self.add_static(solid, segment.layer);
        }
        let extension = ((length - segment.width) * 0.5) as f32;
        if !extension.is_finite() {
            return Err(Self::error(SolidError::Invalid("INSTANCE_EXTENSION")));
        }
        let mut key = [0; 16];
        key[..5].copy_from_slice(&[
            3,
            segment.width.to_bits(),
            z[0].to_bits(),
            z[1].to_bits(),
            tolerance.to_bits(),
        ]);
        let meta = Identity {
            object: SelectedObject::Segment(segment.id),
            layers: MemberLayers::Single(segment.layer),
            net: segment.net,
            placement: PadPlacement {
                at: Point::new(
                    segment.a.x + (segment.b.x - segment.a.x) * 0.5,
                    segment.a.y + (segment.b.y - segment.a.y) * 0.5,
                ),
                angle: (segment.b.y - segment.a.y).atan2(segment.b.x - segment.a.x),
                mirrored: false,
            },
            extension,
        };
        let cancel = self.cancel;
        self.add_shared(key, meta, |limits| {
            let half = segment.width * 0.5;
            let prototype = Segment {
                a: Point::new(-half, 0.0),
                b: Point::new(half, 0.0),
                ..segment.clone()
            };
            PreparedSolid::prepare_trace(&prototype, z, tolerance, limits, cancel)
        })
    }

    fn add_pad(
        &mut self,
        source: PadSolidSource<'_>,
        z: [f64; 2],
        tolerance: f64,
    ) -> Result<(), SolidPrepareError> {
        let p = source.pad;
        let d = source.drill.unwrap_or(DrillShape {
            rectangular: false,
            width: 0.0,
            height: 0.0,
            plated: false,
        });
        let center = Point::new(-p.offset.x, -p.offset.y).rotate(-source.placement.angle);
        let key = [
            1,
            u64::from(p.kind.0),
            p.width.to_bits(),
            p.height.to_bits(),
            p.corner.to_bits(),
            p.inner_diameter.map_or(u64::MAX, f64::to_bits),
            p.custom
                .as_ref()
                .map_or(0, |c| std::sync::Arc::as_ptr(c) as u64),
            u64::from(p.custom.is_some() && source.placement.mirrored),
            d.width.to_bits(),
            d.height.to_bits(),
            if source.drill.is_some() {
                center.x.to_bits()
            } else {
                0
            },
            if source.drill.is_some() {
                center.y.to_bits()
            } else {
                0
            },
            z[0].to_bits(),
            z[1].to_bits(),
            u64::from(d.rectangular),
            0,
        ];
        let meta = Identity {
            object: source.object,
            layers: MemberLayers::Single(p.layer),
            net: source.net,
            extension: 0.0,
            placement: PadPlacement {
                at: Point::new(
                    source.placement.at.x + p.offset.x,
                    source.placement.at.y + p.offset.y,
                ),
                angle: source.placement.angle,
                mirrored: false,
            },
        };
        let cancel = self.cancel;
        self.add_shared(key, meta, |limits| {
            PreparedSolid::prepare_pad(source, z, tolerance, limits, cancel)
        })
    }
}

fn parse_color(value: &str) -> [f32; 4] {
    let Some(hex) = value.strip_prefix('#') else {
        return [0.8, 0.5, 0.15, 1.0];
    };
    if hex.len() != 6 {
        return [0.8, 0.5, 0.15, 1.0];
    }
    let Ok(rgb) = u32::from_str_radix(hex, 16) else {
        return [0.8, 0.5, 0.15, 1.0];
    };
    [
        ((rgb >> 16) & 255) as f32 / 255.0,
        ((rgb >> 8) & 255) as f32 / 255.0,
        (rgb & 255) as f32 / 255.0,
        1.0,
    ]
}
fn ray_box(ray: PickRay3d, b: Bounds3, maximum: f64) -> bool {
    let mut near = 0.0_f64;
    let mut far = maximum;
    for i in 0..3 {
        if ray.direction[i].abs() < 1e-15 {
            if ray.origin[i] < b.min[i] || ray.origin[i] > b.max[i] {
                return false;
            }
        } else {
            let a = (b.min[i] - ray.origin[i]) / ray.direction[i];
            let c = (b.max[i] - ray.origin[i]) / ray.direction[i];
            near = near.max(a.min(c));
            far = far.min(a.max(c));
            if near > far {
                return false;
            }
        }
    }
    true
}
fn ray_triangle(origin: [f64; 3], direction: [f64; 3], p: [[f32; 3]; 3]) -> Option<f64> {
    let p = p.map(|p| p.map(f64::from));
    let sub = |a: [f64; 3], b: [f64; 3]| std::array::from_fn::<_, 3, _>(|i| a[i] - b[i]);
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a.into_iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    let a = sub(p[1], p[0]);
    let b = sub(p[2], p[0]);
    let h = cross(direction, b);
    let det = dot(a, h);
    if det.abs() < 1e-15 {
        return None;
    }
    let s = sub(origin, p[0]);
    let u = dot(s, h) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = cross(s, a);
    let v = dot(direction, q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(b, q) / det;
    (t >= 0.0).then_some(t)
}
