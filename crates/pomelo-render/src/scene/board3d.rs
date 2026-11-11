//! Immutable, GPU-ready conductor geometry. Preview heights are explicit assumptions.
mod edges;
mod holes;
mod paging;
mod parallel;
pub mod raster;
mod round;
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
    /// Also build display-only dielectric slabs between conductor layers.
    pub dielectric: bool,
}
impl Default for PreviewStackup {
    fn default() -> Self {
        Self {
            board_mm: 1.6,
            copper_mm: 0.035,
            plating_mm: 0.025,
            chord_mm: 0.005,
            dielectric: false,
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
    /// [source ID, capsule flag, f32 end-extension bits, circular-side shading flag]. Zero ID uses vertex ownership.
    pub owner: [u32; 4],
}
const _: () = assert!(size_of::<Vertex3d>() == 48 && size_of::<Instance3d>() == 32);

pub const PAGE_VERTICES: usize = 65_536;
pub const PAGE_INDICES: usize = 393_216;
pub const PAGE_INSTANCES: usize = 4_096;

pub struct Mesh3d {
    pub vertices: Vec<Vertex3d>,
    pub indices: Vec<u32>,
    /// Optional screen-error-bounded index ranges, sharing the full-resolution vertices.
    pub round_lods: Vec<RoundLod3d>,
    /// Line-list feature edges stored after every triangle range; empty when absent.
    pub edges: Range<u32>,
}
/// Alternative triangle range with a conservative model-space chord error.
pub struct RoundLod3d {
    pub indices: Range<u32>,
    pub error_mm: f32,
}
impl Mesh3d {
    /// Triangle prefix retained for full-resolution picking.
    pub fn fine_index_count(&self) -> usize {
        self.round_lods
            .first()
            .map_or(self.triangle_index_count(), |lod| {
                lod.indices.start as usize
            })
    }
    /// Append feature edges of the full-resolution triangles.
    pub(crate) fn add_edges(&mut self, generators: bool) {
        let fine = self.fine_index_count();
        let edges = edges::feature_edges(&self.vertices, &self.indices[..fine], generators);
        let start = self.indices.len() as u32;
        self.indices.extend(edges);
        self.edges = start..self.indices.len() as u32;
    }
    /// Index count of the triangle ranges, before the edge lines.
    pub fn triangle_index_count(&self) -> usize {
        if self.edges.is_empty() {
            self.indices.len()
        } else {
            self.edges.start as usize
        }
    }
    pub(crate) fn valid_lods(&self) -> bool {
        if !self.edges.is_empty()
            && (self.edges.end as usize != self.indices.len()
                || !(self.edges.end - self.edges.start).is_multiple_of(2))
        {
            return false;
        }
        let triangles = self.triangle_index_count();
        let fine = self.fine_index_count();
        if fine > triangles || !fine.is_multiple_of(3) || (!self.round_lods.is_empty() && fine == 0)
        {
            return false;
        }
        let mut end = fine as u32;
        for lod in &self.round_lods {
            if lod.indices.start != end
                || lod.indices.end <= end
                || lod.indices.end as usize > triangles
                || !(lod.indices.end - end).is_multiple_of(3)
                || !lod.error_mm.is_finite()
                || lod.error_mm < 0.0
            {
                return false;
            }
            end = lod.indices.end;
        }
        end as usize == triangles
    }
    /// Select an uploaded range whose silhouette error is at most half a pixel.
    pub fn draw_indices(&self, pixels_per_mm: f32) -> Range<u32> {
        if pixels_per_mm.is_finite() && pixels_per_mm > 0.0 {
            for lod in &self.round_lods {
                if lod.error_mm * pixels_per_mm <= 0.5 {
                    return lod.indices.clone();
                }
            }
        }
        0..self.fine_index_count() as u32
    }
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
    /// A display-only dielectric slab: never picked or selected.
    pub dielectric: bool,
}
/// Selection and mode inputs of [`PreparedBoard3d::materials`].
pub struct MaterialOptions<'a> {
    pub mode: pomelo_core::display3d::RenderMode3d,
    pub selected: Option<SelectedObject>,
    pub selected_net: Option<NetId>,
    /// Members of the selected group, drawn in the primary selection color.
    pub related: &'a std::collections::BTreeSet<SelectedObject>,
    /// Added selection-set members, drawn in the secondary selection color.
    pub secondary: &'a std::collections::BTreeSet<SelectedObject>,
    pub hovered: Option<SelectedObject>,
    /// Opacity kept by unselected objects while anything is selected.
    pub unselected_opacity: f32,
    /// Show selected objects with faces even where outlines are requested.
    pub selected_solid: bool,
    /// Draw the dielectric slabs between conductor layers.
    pub show_dielectric: bool,
    /// Highlight group colors, applied before selection colors.
    pub group_colors: Option<&'a std::collections::BTreeMap<SelectedObject, [f32; 3]>>,
}
impl MaterialOptions<'static> {
    /// Default mode with nothing selected.
    pub fn plain() -> Self {
        const NONE: &std::collections::BTreeSet<SelectedObject> =
            &std::collections::BTreeSet::new();
        Self {
            mode: pomelo_core::display3d::RenderMode3d::Default,
            selected: None,
            selected_net: None,
            related: NONE,
            secondary: NONE,
            hovered: None,
            unselected_opacity: 1.0,
            selected_solid: true,
            show_dielectric: false,
            group_colors: None,
        }
    }
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
        limits: SolidLimits,
        cancel: &CancellationToken,
    ) -> Result<Self, SolidPrepareError> {
        Self::preview_with_workers(board, preview, limits, cancel, parallel::worker_limit())
    }

    fn preview_partition(
        board: &BoardScene,
        preview: PreviewStackup,
        mut limits: SolidLimits,
        cancel: &CancellationToken,
        only: Option<LayerId>,
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
            if let Some(&z) = heights.get(&segment.layer)
                && only.is_none_or(|layer| layer == segment.layer)
            {
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
            if let Some(&z) = heights.get(&zone.layer)
                && only.is_none_or(|layer| layer == zone.layer)
            {
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
            cancel.check().map_err(|e| fail(e.into()))?;
            let placement = PadPlacement {
                at: pin.at,
                angle: pin.angle,
                mirrored: pin.mirrored,
            };
            for pad in &pin.pads {
                if let Some(&z) = heights.get(&pad.layer)
                    && only.is_none_or(|layer| layer == pad.layer)
                {
                    builder.add_pad(
                        PadSolidSource {
                            object: SelectedObject::Pin(pin.id),
                            net: pin.net,
                            pad,
                            placement,
                            drill: pin.drill_shape.pad().map(|_| pin.drill_shape.clone()),
                        },
                        z,
                        preview.chord_mm,
                    )?;
                }
            }
        }
        for via in &board.vias {
            cancel.check().map_err(|e| fail(e.into()))?;
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
                if let Some(&z) = heights.get(&pad.layer)
                    && only.is_none_or(|layer| layer == pad.layer)
                {
                    builder.add_pad(
                        PadSolidSource {
                            object: SelectedObject::Via(via.id),
                            net: via.net,
                            pad,
                            placement,
                            drill: None,
                        },
                        z,
                        preview.chord_mm,
                    )?;
                }
            }
            if via.drill_shape.plated
                && via.drill_shape.pad().is_some()
                && only.is_none_or(|layer| layer == span.map_or(layers[0], |s| s.0))
            {
                if let Some((a, b, za, zb)) = span {
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
                    key[7] = via
                        .drill_shape
                        .custom
                        .as_ref()
                        .map_or(0, |c| std::sync::Arc::as_ptr(c) as u64);
                    key[8] = u64::from(via.drill_shape.custom.is_some() && via.mirrored);
                    let meta = Identity {
                        object: SelectedObject::Via(via.id),
                        layers: MemberLayers::Span(a, b),
                        net: via.net,
                        extension: 0.0,
                        smooth_round: via.drill_shape.custom.is_none()
                            && !via.drill_shape.rectangular
                            && via.drill_shape.width == via.drill_shape.height,
                        placement: PadPlacement {
                            mirrored: false,
                            ..placement
                        },
                    };
                    builder.add_shared(key, meta, |limits| {
                        PreparedSolid::prepare_via_body(
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
                        source: SolidError::Unsupported("PREVIEW_VIA_SPAN"),
                    })?;
                }
            }
        }
        if preview.dielectric && only.is_none_or(|layer| layer == layers[0]) {
            for (gap, pair) in layers.windows(2).enumerate() {
                cancel.check().map_err(|e| fail(e.into()))?;
                let (Some(upper), Some(lower)) = (heights.get(&pair[0]), heights.get(&pair[1]))
                else {
                    continue;
                };
                let Some(ring) = outline_ring(board, preview.chord_mm) else {
                    break;
                };
                let remaining = builder.remaining()?;
                let Ok(mesh) = pomelo_core::copper::CopperMesh::build(
                    &[ring],
                    &[],
                    &pomelo_core::copper::MeshLimits::default(),
                    cancel,
                ) else {
                    break;
                };
                let zone = pomelo_core::model::Zone {
                    id: pomelo_core::model::ObjectId(u32::MAX - gap as u32),
                    layer: pair[0],
                    net: NetId(0),
                    kind: Default::default(),
                    paths: vec![],
                    mesh,
                };
                let cuts = holes::cutouts(
                    &holes,
                    &hole_index,
                    pair[0],
                    zone.mesh.ring_bounds.first().copied(),
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
                let first = builder.sources.len();
                builder.add_static(
                    PreparedSolid::prepare_zone_with_cutouts(
                        &zone,
                        [lower[1], upper[0]],
                        preview.chord_mm,
                        &cuts,
                        remaining,
                        cancel,
                    ),
                    pair[0],
                )?;
                for source in &mut builder.sources[first..] {
                    source.dielectric = true;
                }
            }
        }
        builder.finish(layers, preview, only.is_some())
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
                .map(|m| {
                    m.vertices.capacity() * size_of::<Vertex3d>()
                        + m.indices.capacity() * 4
                        + m.round_lods.capacity() * size_of::<RoundLod3d>()
                })
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

    /// Face materials for slot 0 and every source, followed by edge materials in
    /// the same order. A zero alpha draws nothing.
    pub fn materials(
        &self,
        board: &BoardScene,
        display: &BoardDisplay,
        options: &MaterialOptions<'_>,
    ) -> Vec<[f32; 4]> {
        use pomelo_core::display::{CellMode, LayerPrimitive};
        use pomelo_core::display3d::RenderMode3d;
        const PRIMARY: [f32; 3] = [1.0, 0.35, 0.05];
        const SECONDARY: [f32; 3] = [1.0, 0.82, 0.4];
        const HOVER: [f32; 3] = [0.63, 1.0, 0.85];
        let colors: BTreeMap<_, _> = board
            .layers
            .iter()
            .map(|l| (l.id, parse_color(&l.color)))
            .collect();
        let count = self.sources.len() + 1;
        let mut faces = Vec::with_capacity(count * 2);
        faces.push([0.0; 4]);
        let mut edges = Vec::with_capacity(count);
        edges.push([0.0; 4]);
        let any_selection = options.selected.is_some()
            || options.selected_net.is_some()
            || !options.related.is_empty()
            || !options.secondary.is_empty();
        for source in &self.sources {
            if source.dielectric {
                const DIELECTRIC: [f32; 3] = [0.36, 0.52, 0.4];
                let shown = options.show_dielectric;
                let wire = options.mode == RenderMode3d::Wireframe;
                let [r, g, b] = DIELECTRIC;
                faces.push([r, g, b, if shown && !wire { 0.18 } else { 0.0 }]);
                edges.push([r * 0.8, g * 0.8, b * 0.8, f32::from(shown && wire)]);
                continue;
            }
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
                .filter(|_| display.net_visible(source.net))
            else {
                faces.push([0.0; 4]);
                edges.push([0.0; 4]);
                continue;
            };
            let (category, kind) = match source.object {
                SelectedObject::Segment(_) => (DisplayCategory::Trace, LayerPrimitive::Traces),
                SelectedObject::Pin(_) => (DisplayCategory::Pin, LayerPrimitive::Pads),
                SelectedObject::Via(_) => (DisplayCategory::Via, LayerPrimitive::Vias),
                _ => (DisplayCategory::Zone, LayerPrimitive::Copper),
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
            if let Some(&[r, g, b]) = options
                .group_colors
                .and_then(|colors| colors.get(&source.object))
            {
                color[..3].copy_from_slice(&[r, g, b]);
            }
            let alpha = if matches!(source.object, SelectedObject::Zone(_)) {
                display.copper_opacity
            } else {
                display.global_opacity
            } * display.layer_alpha(layer);
            let outline = options.mode == RenderMode3d::Wireframe
                || display.cell_mode(layer, kind) == CellMode::Outline;
            let mut face = if outline {
                0.0
            } else if options.mode == RenderMode3d::Solid {
                1.0
            } else {
                alpha
            };
            let mut edge: f32 = if outline { 1.0 } else { 0.0 };
            let primary = options.selected == Some(source.object)
                || options.selected_net == Some(source.net)
                || options.related.contains(&source.object);
            let secondary = options.secondary.contains(&source.object);
            if primary || secondary {
                color[..3].copy_from_slice(if primary { &PRIMARY } else { &SECONDARY });
                if options.selected_solid {
                    face = 1.0;
                }
                face = face.max(0.6);
            } else if options.hovered == Some(source.object) {
                color[..3].copy_from_slice(&HOVER);
                face = face.max(0.5);
                edge = edge.max(f32::from(outline));
            } else if any_selection {
                face *= options.unselected_opacity;
                // Lines stay opaque; darken them in proportion instead.
                let keep = 0.25 + 0.75 * options.unselected_opacity;
                let rgb = [color[0] * keep, color[1] * keep, color[2] * keep];
                faces.push([color[0], color[1], color[2], face.clamp(0.0, 1.0)]);
                edges.push([rgb[0], rgb[1], rgb[2], edge]);
                continue;
            }
            faces.push([color[0], color[1], color[2], face.clamp(0.0, 1.0)]);
            edges.push([color[0], color[1], color[2], edge]);
        }
        faces.extend(edges);
        faces
    }

    pub fn visible_pages(
        &self,
        camera: pomelo_core::interaction::camera3d::Camera3d,
        viewport: [f64; 2],
        z_map: ZMap,
        output: &mut Vec<usize>,
    ) -> Result<(), SolidError> {
        output.clear();
        if !z_map.scale.is_finite() || z_map.scale <= 0.0 {
            return Err(SolidError::Invalid("DISPLAY_Z_SCALE"));
        }
        output
            .try_reserve(self.pages.len())
            .map_err(|_| SolidError::Allocation)?;
        self.page_index
            .visit_frustum(camera, viewport, z_map, |id| {
                if camera.intersects(z_map.bounds(self.pages[id].bounds), viewport) {
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
        // Bounds use f64 source geometry, while picking intersects f32 GPU vertices.
        // Allow their rounding envelope so a previous coplanar hit cannot prune
        // equivalent candidates before the stable object-identity tie break.
        let padding = (0..3)
            .map(|axis| self.bounds.max[axis] - self.bounds.min[axis])
            .fold(1.0_f64, f64::max)
            * f64::from(f32::EPSILON)
            * 4.0;
        self.source_index
            .visit_ray(ray, &mut nearest, padding, |i, nearest| {
                let source = &self.sources[i];
                if source.dielectric
                    || !filter.contains(source.object.into())
                    || (materials.get(i + 1).is_none_or(|m| m[3] <= 0.0)
                        && materials
                            .get(self.sources.len() + 2 + i)
                            .is_none_or(|m| m[3] <= 0.0))
                    || !ray_box(ray, source.bounds, *nearest, padding)
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
                            && (t < *nearest
                                || (t == *nearest && hit.is_none_or(|old| source.object < old)))
                        {
                            *nearest = t;
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
    smooth_round: bool,
}
struct Group {
    vertices: Vec<Vertex3d>,
    indices: Vec<u32>,
    instances: Vec<Instance3d>,
    sources: Vec<u32>,
    bounds: Option<Bounds3>,
    shared: bool,
    round_lods: Vec<RoundLod3d>,
}
impl Group {
    fn allocation_bytes(&self) -> usize {
        self.vertices.capacity() * size_of::<Vertex3d>()
            + self.indices.capacity() * 4
            + self.instances.capacity() * size_of::<Instance3d>()
            + self.sources.capacity() * 4
            + self.round_lods.capacity() * size_of::<RoundLod3d>()
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
            round_lods: vec![],
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
            dielectric: false,
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
            smooth_round: false,
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
        let empty_drill = DrillShape {
            custom: None,
            rectangular: false,
            width: 0.0,
            height: 0.0,
            plated: false,
        };
        let d = source.drill.as_ref().unwrap_or(&empty_drill);
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
            u64::from((p.custom.is_some() || d.custom.is_some()) && source.placement.mirrored),
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
            d.custom
                .as_ref()
                .map_or(0, |c| std::sync::Arc::as_ptr(c) as u64),
        ];
        let meta = Identity {
            object: source.object,
            layers: MemberLayers::Single(p.layer),
            net: source.net,
            smooth_round: p.custom.is_none()
                && matches!(p.kind.0, 2 | 25)
                && source.drill.is_none(),
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
fn ray_box(ray: PickRay3d, b: Bounds3, maximum: f64, padding: f64) -> bool {
    let mut near = 0.0_f64;
    let mut far = maximum + padding;
    for i in 0..3 {
        if ray.direction[i].abs() < 1e-15 {
            if ray.origin[i] < b.min[i] - padding || ray.origin[i] > b.max[i] + padding {
                return false;
            }
        } else {
            let a = (b.min[i] - padding - ray.origin[i]) / ray.direction[i];
            let c = (b.max[i] + padding - ray.origin[i]) / ray.direction[i];
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

/// Board outline as one closed polyline, following segment connectivity and
/// flattening arcs. Falls back to the board bounds when the outline is open.
fn outline_ring(board: &BoardScene, chord: f64) -> Option<Vec<Point>> {
    let bounds = board.bounds;
    let rectangle = || {
        bounds.is_valid().then(|| {
            vec![
                Point::new(bounds.min.x, bounds.min.y),
                Point::new(bounds.max.x, bounds.min.y),
                Point::new(bounds.max.x, bounds.max.y),
                Point::new(bounds.min.x, bounds.max.y),
            ]
        })
    };
    let segments = &board.outline;
    if segments.len() < 3 {
        return rectangle();
    }
    let close = |a: Point, b: Point| a.distance(b) <= 1e-3;
    let flatten = |segment: &Segment, reverse: bool| -> Vec<Point> {
        let mut points = match segment.arc {
            Some(arc) => {
                let steps = ((arc.sweep.abs() * arc.radius / chord.max(0.01))
                    .sqrt()
                    .ceil() as usize)
                    .clamp(4, 64);
                (0..=steps)
                    .map(|i| {
                        let t = arc.start + arc.sweep * i as f64 / steps as f64;
                        Point::new(
                            arc.center.x + arc.radius * t.cos(),
                            arc.center.y + arc.radius * t.sin(),
                        )
                    })
                    .collect()
            }
            None => vec![segment.a, segment.b],
        };
        // Arc samples run from its start angle; orient them like the segment.
        if points.first().is_some_and(|p| !close(*p, segment.a)) {
            points.reverse();
        }
        if reverse {
            points.reverse();
        }
        points
    };
    let mut used = vec![false; segments.len()];
    used[0] = true;
    let mut ring = flatten(&segments[0], false);
    let start = segments[0].a;
    for _ in 1..segments.len() {
        let end = *ring.last()?;
        let next = segments.iter().enumerate().find_map(|(i, s)| {
            (!used[i]).then_some(())?;
            if close(s.a, end) {
                Some((i, false))
            } else if close(s.b, end) {
                Some((i, true))
            } else {
                None
            }
        });
        let Some((i, reverse)) = next else {
            return rectangle();
        };
        used[i] = true;
        ring.extend(flatten(&segments[i], reverse).into_iter().skip(1));
    }
    if !ring.last().is_some_and(|p| close(*p, start)) {
        return rectangle();
    }
    ring.pop();
    ring.dedup_by(|a, b| close(*a, *b));
    (ring.len() >= 3).then_some(ring)
}

/// Model-to-display height map shared by the shader, culling, fitting and picking.
/// With `dielectric` the copper layers keep their thickness and only the gaps
/// between them grow; otherwise every height is multiplied by `scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZMap {
    pub scale: f64,
    /// Uniform preview stack: pitch, copper thickness, top surface, layer count.
    pub stack: Option<[f64; 4]>,
}

impl ZMap {
    pub fn linear(scale: f64) -> Self {
        Self { scale, stack: None }
    }
    pub fn apply(self, z: f64) -> f64 {
        let Some([pitch, copper, top, count]) = self.stack else {
            return z * self.scale;
        };
        let depth = (top - z).clamp(0.0, pitch * (count - 1.0) + copper);
        let layers = (depth / pitch).floor().min(count - 1.0);
        let within = depth - layers * pitch;
        let metal = layers * copper + within.min(copper);
        let gap = depth - metal;
        let outside = (top - z) - depth;
        top * self.scale - (metal + gap * self.scale + outside * self.scale)
    }
    /// Inverse of [`Self::apply`]; the map is strictly increasing.
    pub fn inverse(self, display: f64) -> f64 {
        if self.stack.is_none() {
            return display / self.scale;
        }
        let (mut low, mut high) = (-1e6_f64, 1e6_f64);
        for _ in 0..80 {
            let mid = 0.5 * (low + high);
            if self.apply(mid) < display {
                low = mid;
            } else {
                high = mid;
            }
        }
        0.5 * (low + high)
    }
    pub fn bounds(self, bounds: Bounds3) -> Bounds3 {
        Bounds3 {
            min: [bounds.min[0], bounds.min[1], self.apply(bounds.min[2])],
            max: [bounds.max[0], bounds.max[1], self.apply(bounds.max[2])],
        }
    }
    /// Display heights where the slope of the map changes, low to high.
    fn breaks(self) -> Vec<f64> {
        let Some([pitch, copper, top, count]) = self.stack else {
            return vec![];
        };
        let mut z: Vec<_> = (0..count as usize)
            .flat_map(|i| {
                let layer_top = top - pitch * i as f64;
                [layer_top, layer_top - copper]
            })
            .map(|z| self.apply(z))
            .collect();
        z.sort_by(f64::total_cmp);
        z
    }
    /// Split a display-space ray into model-space rays, nearest first. Each piece
    /// spans t in 0..1 along its unnormalized direction.
    pub fn model_rays(self, ray: PickRay3d) -> Vec<PickRay3d> {
        let point =
            |t: f64| std::array::from_fn::<_, 3, _>(|i| ray.origin[i] + ray.direction[i] * t);
        let mut ts = vec![0.0, ray.length];
        if ray.direction[2].abs() > f64::EPSILON {
            for z in self.breaks() {
                let t = (z - ray.origin[2]) / ray.direction[2];
                if t > 0.0 && t < ray.length {
                    ts.push(t);
                }
            }
        }
        ts.sort_by(f64::total_cmp);
        ts.windows(2)
            .filter(|w| w[1] > w[0])
            .map(|w| {
                let [mut a, mut b] = [point(w[0]), point(w[1])];
                a[2] = self.inverse(a[2]);
                b[2] = self.inverse(b[2]);
                PickRay3d {
                    origin: a,
                    direction: std::array::from_fn(|i| b[i] - a[i]),
                    length: 1.0,
                }
            })
            .collect()
    }
    /// Shader parameters: (mode flag, stack) for the solid uniforms.
    pub fn uniforms(self) -> (f32, [f32; 4]) {
        match self.stack {
            Some(stack) => (1.0, stack.map(|v| v as f32)),
            None => (0.0, [0.0; 4]),
        }
    }
}

impl PreparedBoard3d {
    /// Height map for a display factor; dielectric-only needs at least two layers.
    pub fn z_map(&self, scale: f64, dielectric_only: bool) -> ZMap {
        let count = self.layers.len();
        if !dielectric_only || count < 2 {
            return ZMap::linear(scale);
        }
        let p = self.preview;
        let pitch = (p.board_mm - p.copper_mm) / (count - 1) as f64;
        ZMap {
            scale,
            stack: (pitch > p.copper_mm).then_some([
                pitch,
                p.copper_mm,
                p.board_mm * 0.5,
                count as f64,
            ]),
        }
    }
}

#[cfg(test)]
mod z_map_tests {
    use super::*;

    fn map() -> ZMap {
        ZMap {
            scale: 10.0,
            stack: Some([0.5, 0.05, 0.775, 4.0]),
        }
    }

    #[test]
    fn dielectric_only_keeps_copper_thickness_and_stays_monotonic() {
        let m = map();
        let top = 0.775;
        assert!((m.apply(top) - top * 10.0).abs() < 1e-9);
        // A copper layer keeps its 0.05 mm thickness.
        assert!(((m.apply(top) - m.apply(top - 0.05)) - 0.05).abs() < 1e-9);
        // A gap grows by the factor.
        assert!(((m.apply(top - 0.05) - m.apply(top - 0.5)) - 4.5).abs() < 1e-9);
        let mut previous = f64::NEG_INFINITY;
        for i in 0..400 {
            let z = -1.0 + i as f64 * 0.005;
            let d = m.apply(z);
            assert!(d > previous);
            assert!((m.inverse(d) - z).abs() < 1e-6);
            previous = d;
        }
    }

    #[test]
    fn split_rays_join_end_to_end_in_model_space() {
        let m = map();
        let ray = PickRay3d {
            origin: [0.0, 0.0, 20.0],
            direction: [0.1, 0.0, -1.0],
            length: 40.0,
        };
        let pieces = m.model_rays(ray);
        assert!(pieces.len() > 2);
        for w in pieces.windows(2) {
            let end = std::array::from_fn::<_, 3, _>(|i| w[0].origin[i] + w[0].direction[i]);
            for (end, start) in end.iter().zip(w[1].origin) {
                assert!((end - start).abs() < 1e-6);
            }
        }
        assert_eq!(ZMap::linear(5.0).model_rays(ray).len(), 1);
    }
}
