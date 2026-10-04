//! Complete immutable scene assembly. The output budget is separate from source/index/cache limits.

use super::{
    connectivity::{NetworkLimits, NetworkMap},
    copper::{CopperDecoder, CopperLimits},
    drawing::{DrawingBuilder, DrawingLimits},
    layers::{drawing_layer, read_layers},
    placement::{PlacementDecoder, PlacementLimits},
    text::{TextBuilder, TextLimits},
};
use crate::{ImportContext, ImportError, allegro::database::BrdDatabase};
use pomelo_core::{
    i18n::{Message, MessageKey as Key},
    model::{
        BoardDrawing, BoardScene, BoardText, Bounds, Diagnostic, DrawingLayer, LayerId, Pad, Point,
        Segment, SpecialLayer, SpecialLayerKind, Zone,
    },
    pad::PadPlacement,
    task::{ImportProgress, ImportStage},
};
use std::{collections::BTreeMap, mem::size_of};

#[derive(Debug, Clone)]
pub struct SceneLimits {
    /// Aggregate conservative scene output accounting, including Vec growth and diagnostics.
    /// Source/index and definition/network/geometry scratch caches have their own bounded budgets.
    pub max_output_bytes: usize,
    pub max_objects: usize,
    pub networks: NetworkLimits,
    pub placement: PlacementLimits,
    pub copper: CopperLimits,
    pub text: TextLimits,
    pub drawing: DrawingLimits,
}
impl Default for SceneLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 4 * 1024 * 1024 * 1024,
            max_objects: 16_000_000,
            networks: NetworkLimits::default(),
            placement: PlacementLimits::default(),
            copper: CopperLimits::default(),
            text: TextLimits::default(),
            drawing: DrawingLimits::default(),
        }
    }
}

struct OutputBudget {
    bytes: usize,
    objects: usize,
    max_bytes: usize,
    max_objects: usize,
}
impl OutputBudget {
    fn available(&self) -> usize {
        self.max_bytes.saturating_sub(self.bytes)
    }
    fn charge(&mut self, bytes: usize, objects: usize, offset: usize) -> Result<(), ImportError> {
        let actual = self.bytes.saturating_add(bytes);
        if actual > self.max_bytes {
            return Err(ImportError::GeometryLimit {
                offset,
                actual: actual as u64,
                limit: self.max_bytes as u64,
            });
        }
        let count = self.objects.saturating_add(objects);
        if count > self.max_objects {
            return Err(ImportError::InvalidRecord {
                offset,
                field: "SCENE_OBJECT_COUNT",
                value: count as u64,
            });
        }
        self.bytes = actual;
        self.objects = count;
        Ok(())
    }
    fn diagnostics(
        &mut self,
        source: Vec<Diagnostic>,
        target: &mut Vec<Diagnostic>,
    ) -> Result<(), ImportError> {
        self.charge(source.len().saturating_mul(1024), source.len(), 0)?;
        target.extend(source);
        Ok(())
    }
}

pub struct SceneBuilder<'a> {
    database: &'a BrdDatabase,
    limits: SceneLimits,
}
impl<'a> SceneBuilder<'a> {
    pub fn new(database: &'a BrdDatabase, limits: SceneLimits) -> Self {
        Self { database, limits }
    }

    /// No partial scene is published after cancellation, a reference failure, or a resource limit.
    pub fn build(self, context: &ImportContext<'_>) -> Result<BoardScene, ImportError> {
        self.build_with_stage_observer(context, &|_, _| {})
    }

    /// Observe coarse scene-building wall times without per-record instrumentation.
    /// The observer runs synchronously and must avoid expensive work.
    pub fn build_with_stage_observer(
        self,
        context: &ImportContext<'_>,
        observer: &dyn Fn(&'static str, std::time::Duration),
    ) -> Result<BoardScene, ImportError> {
        let mut checkpoint = std::time::Instant::now();
        let mut stage = |name| {
            let now = std::time::Instant::now();
            observer(name, now.duration_since(checkpoint));
            checkpoint = std::time::Instant::now();
        };
        context.check_cancelled()?;
        let database = self.database;
        let mut budget = OutputBudget {
            bytes: 0,
            objects: 0,
            max_bytes: self.limits.max_output_bytes,
            max_objects: self.limits.max_objects,
        };
        let layers = read_layers(database, context)?;
        let layer_bytes = layers.iter().fold(
            layers
                .capacity()
                .saturating_mul(2 * size_of::<pomelo_core::model::Layer>()),
            |n, l| {
                n.saturating_add(l.name.capacity())
                    .saturating_add(l.color.capacity())
            },
        );
        budget.charge(layer_bytes, layers.len(), 0)?;
        stage("layers");
        let networks = NetworkMap::build(database, &self.limits.networks, context)?;
        for name in networks.nets.values() {
            context.check_cancelled()?;
            budget.charge(128usize.saturating_add(name.capacity()), 1, 0)?;
        }
        let layer_count = layers.len() as u32;
        stage("networks");
        let mut scene = BoardScene {
            layers,
            special_layers: Vec::new(),
            nets: BTreeMap::new(),
            segments: Vec::new(),
            pins: Vec::new(),
            components: Vec::new(),
            vias: Vec::new(),
            zones: Vec::new(),
            outline: Vec::new(),
            texts: Vec::new(),
            drawing_layers: Vec::new(),
            drawings: Vec::new(),
            bounds: Bounds {
                min: Point::default(),
                max: Point::default(),
            },
            diagnostics: Vec::new(),
        };
        let mut bounds = None;
        let mut placement =
            PlacementDecoder::new(database, &networks, layer_count, self.limits.placement)?;
        for record in database.records_of_type(5, context) {
            let record = record?;
            let offset = record.span.offset.0 as usize;
            placement.bound_next_output(budget.available());
            let before = placement.output_bytes();
            let segments = placement.track(record.span.key, context)?;
            budget.charge(
                placement.output_bytes().saturating_sub(before),
                segments.len(),
                offset,
            )?;
            for edge in &segments {
                context.check_cancelled()?;
                include_segment(&mut bounds, edge, offset)?;
                if edge.bond_wire.is_some() {
                    special(&mut scene, &mut budget, SpecialLayerKind::BondWire)?;
                }
            }
            scene.segments.extend(segments);
        }
        stage("tracks");
        for record in database.records_of_type(0x33, context) {
            let record = record?;
            let offset = record.span.offset.0 as usize;
            placement.bound_next_output(budget.available());
            let before = placement.output_bytes();
            let via = placement.via(record.span.key, context)?;
            budget.charge(
                placement.output_bytes().saturating_sub(before),
                usize::from(via.is_some()),
                offset,
            )?;
            if let Some(via) = via {
                include_pads(
                    &mut bounds,
                    &via.pads,
                    via.drill_shape.pad(),
                    PadPlacement {
                        at: via.at,
                        angle: via.angle,
                        mirrored: via.mirrored,
                    },
                    via.id.0,
                    offset,
                    context,
                )?;
                scene.vias.push(via);
            }
        }
        stage("vias");
        for record in database.records_of_type(0x2d, context) {
            let record = record?;
            let offset = record.span.offset.0 as usize;
            placement.bound_next_output(budget.available());
            let before = placement.output_bytes();
            let footprint = placement.footprint(record.span.key, context)?;
            budget.charge(
                placement.output_bytes().saturating_sub(before),
                footprint.pins.len().saturating_add(1),
                offset,
            )?;
            for pin in &footprint.pins {
                include_pads(
                    &mut bounds,
                    &pin.pads,
                    pin.drill_shape.pad(),
                    PadPlacement {
                        at: pin.at,
                        angle: pin.angle,
                        mirrored: pin.mirrored,
                    },
                    pin.id.0,
                    offset,
                    context,
                )?;
                if pin.die.is_some() {
                    special(&mut scene, &mut budget, SpecialLayerKind::DiePad)?;
                }
            }
            scene.components.push(footprint.component);
            scene.pins.extend(footprint.pins);
        }
        budget.diagnostics(placement.take_diagnostics(), &mut scene.diagnostics)?;
        drop(placement);
        stage("footprints");
        let mut copper = CopperDecoder::new(database, &networks, layer_count, self.limits.copper)?;
        for record in database.records_of_type(0x28, context) {
            let record = record?;
            let offset = record.span.offset.0 as usize;
            copper.bound_next_output(budget.available());
            let before = copper.output_bytes();
            let object = copper.shape(record.span.key, context)?;
            let count = object
                .segments
                .len()
                .saturating_add(object.outline.len())
                .saturating_add(usize::from(object.zone.is_some()));
            budget.charge(copper.output_bytes().saturating_sub(before), count, offset)?;
            for edge in &object.segments {
                context.check_cancelled()?;
                include_segment(&mut bounds, edge, offset)?;
            }
            // Board-fit includes exact outline arcs and stroke widths, like Web.
            for edge in &object.outline {
                context.check_cancelled()?;
                include_segment(&mut bounds, edge, offset)?;
            }
            if let Some(zone) = object.zone {
                include_zone(&mut bounds, &zone, offset, context)?;
                scene.zones.push(zone);
            }
            scene.segments.extend(object.segments);
            scene.outline.extend(object.outline);
        }
        for kind in [0x0e, 0x24] {
            for record in database.records_of_type(kind, context) {
                let record = record?;
                let offset = record.span.offset.0 as usize;
                copper.bound_next_output(budget.available());
                let before = copper.output_bytes();
                let zone = copper.rectangle(record.span.key, context)?;
                budget.charge(
                    copper.output_bytes().saturating_sub(before),
                    usize::from(zone.is_some()),
                    offset,
                )?;
                if let Some(zone) = zone {
                    include_zone(&mut bounds, &zone, offset, context)?;
                    scene.zones.push(zone);
                }
            }
        }
        for record in database.records_of_type(0x14, context) {
            let record = record?;
            copper.bound_next_output(budget.available());
            let before = copper.output_bytes();
            let outline = copper.graphic_outline(record.span.key, context)?;
            budget.charge(
                copper.output_bytes().saturating_sub(before),
                outline.len(),
                record.span.offset.0 as usize,
            )?;
            // Only accepted board/package outlines expand fit bounds; dimension
            // drawings and text annotations are deliberately excluded.
            for edge in &outline {
                context.check_cancelled()?;
                include_segment(&mut bounds, edge, record.span.offset.0 as usize)?;
            }
            scene.outline.extend(outline);
        }
        budget.diagnostics(copper.take_diagnostics(), &mut scene.diagnostics)?;
        drop(copper);
        stage("copper");
        scene.nets = networks.nets;
        scene.bounds = bounds
            .filter(|b: &Bounds| b.is_valid())
            .ok_or(ImportError::NoGeometry)?;
        let mut text_limits = self.limits.text;
        text_limits.objects.max_bytes = text_limits.objects.max_bytes.min(budget.available());
        let texts = TextBuilder::new(database, text_limits).build(context)?;
        let bytes = texts.texts.iter().fold(
            texts.texts.len().saturating_mul(2 * size_of::<BoardText>()),
            |n, t| n.saturating_add(t.text.capacity()),
        );
        let bytes = texts.drawing_layers.iter().fold(bytes, |n, l| {
            n.saturating_add(2048)
                .saturating_add(l.source_name.capacity())
        });
        budget.charge(
            bytes,
            texts.texts.len().saturating_add(texts.drawing_layers.len()),
            0,
        )?;
        budget.diagnostics(texts.diagnostics, &mut scene.diagnostics)?;
        scene.texts = texts.texts;
        scene.drawing_layers = texts.drawing_layers;
        stage("texts");
        let mut drawing_limits = self.limits.drawing;
        drawing_limits.objects.max_bytes = drawing_limits.objects.max_bytes.min(budget.available());
        let drawings =
            DrawingBuilder::new(database, drawing_limits).build(&scene.texts, context)?;
        for drawing in &drawings.drawings {
            context.check_cancelled()?;
            let bytes = (2 * size_of::<BoardDrawing>() + 128)
                .saturating_add(
                    drawing
                        .segments
                        .capacity()
                        .saturating_mul(size_of::<Segment>()),
                )
                .saturating_add(
                    drawing
                        .graphic_ids
                        .capacity()
                        .saturating_add(drawing.text_ids.capacity())
                        .saturating_mul(size_of::<pomelo_core::model::ObjectId>()),
                );
            budget.charge(bytes, drawing.segments.len().saturating_add(1), 0)?;
        }
        budget.diagnostics(drawings.diagnostics, &mut scene.diagnostics)?;
        scene.drawings = drawings.drawings;
        if !scene.drawings.is_empty()
            && !scene
                .drawing_layers
                .iter()
                .any(|l| l.id == LayerId::DIMENSION)
        {
            budget.charge(2048 + 2 * size_of::<DrawingLayer>(), 1, 0)?;
            scene
                .drawing_layers
                .push(drawing_layer(0xf901, String::new()));
            scene
                .drawing_layers
                .sort_by_key(|l| (!l.default_visible, l.id));
        }
        context.check_cancelled()?;
        stage("drawings");
        (context.progress)(ImportProgress {
            stage: ImportStage::BuildingGeometry,
            completed: budget.objects as u64,
            total: Some(budget.objects as u64),
        });
        Ok(scene)
    }
}

fn include_point(bounds: &mut Option<Bounds>, point: Point) {
    if let Some(bounds) = bounds {
        bounds.include(point);
    } else {
        *bounds = Some(Bounds {
            min: point,
            max: point,
        });
    }
}
fn include_box(bounds: &mut Option<Bounds>, value: Bounds) {
    include_point(bounds, value.min);
    include_point(bounds, value.max);
}
fn include_segment(
    bounds: &mut Option<Bounds>,
    edge: &Segment,
    offset: usize,
) -> Result<(), ImportError> {
    let value = edge.bounds().ok_or(ImportError::InvalidGeometry {
        key: edge.id.0,
        offset,
        field: "SCENE_SEGMENT_BOUNDS",
    })?;
    include_box(bounds, value);
    Ok(())
}
fn include_pads(
    bounds: &mut Option<Bounds>,
    pads: &[Pad],
    drill: Option<Pad>,
    owner: PadPlacement,
    key: u32,
    offset: usize,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    for pad in pads.iter().chain(drill.as_ref()) {
        context.check_cancelled()?;
        include_box(
            bounds,
            pad.bounds(owner).ok_or(ImportError::InvalidGeometry {
                key,
                offset,
                field: "SCENE_PAD_BOUNDS",
            })?,
        );
    }
    Ok(())
}
fn include_zone(
    bounds: &mut Option<Bounds>,
    zone: &Zone,
    offset: usize,
    context: &ImportContext<'_>,
) -> Result<(), ImportError> {
    if let Some(path) = zone.paths.first().filter(|p| !p.is_empty()) {
        for edge in path {
            context.check_cancelled()?;
            include_box(
                bounds,
                edge.centreline_bounds()
                    .ok_or(ImportError::InvalidGeometry {
                        key: zone.id.0,
                        offset,
                        field: "SCENE_ZONE_BOUNDS",
                    })?,
            );
        }
    } else if let Some(value) = zone.mesh.ring_bounds.first() {
        include_box(bounds, *value);
    }
    Ok(())
}
fn special(
    scene: &mut BoardScene,
    budget: &mut OutputBudget,
    kind: SpecialLayerKind,
) -> Result<(), ImportError> {
    if scene.special_layers.iter().any(|l| l.kind == kind) {
        return Ok(());
    }
    budget.charge(1024, 1, 0)?;
    let (id, color, label) = match kind {
        SpecialLayerKind::BondWire => (LayerId::BOND_WIRE_TOP, "#e4d95b", Key::BondWireLayerName),
        SpecialLayerKind::DiePad => (LayerId::BOND_TOP, "#d7cd58", Key::DiePadLayerName),
    };
    scene.special_layers.push(SpecialLayer {
        id,
        kind,
        color: color.into(),
        label: Message::new(label),
    });
    Ok(())
}
