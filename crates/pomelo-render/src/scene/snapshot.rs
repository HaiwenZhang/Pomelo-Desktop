//! Vector snapshot of a board range as SVG, from the same immutable scene and display
//! state as the canvas. The application rasterizes it for PNG output.
//!
//! The snapshot contains visible copper (zones, tracks, pads, vias), drawings, drills,
//! the board outline, persistent highlight colors, optional review marks and a scale
//! bar. Prepared source text and camera-dependent labels share the canvas assets.
use crate::scene::colors::copper_color;
use pomelo_core::appearance::pattern::FillPattern;
use pomelo_core::{
    display::{BoardDisplay, DisplayCategory, LayerPrimitive},
    geometry::flatten_path,
    interaction::Camera,
    model::{BoardScene, Bounds, LayerId, Pad, Point, Segment},
    pad::{PadOutline, PadPlacement},
    selection::SelectedObject,
    task::CancellationToken,
    units::LengthUnit,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};
mod annotations;
pub use annotations::AnnotationSources;

/// Default element budget for one snapshot.
pub const MAX_ELEMENTS: usize = 2_000_000;
/// Largest output edge in pixels.
pub const MAX_EDGE: u32 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    #[error("SNAPSHOT_CANCELLED")]
    Cancelled,
    #[error("SNAPSHOT_EMPTY_RANGE")]
    EmptyRange,
    #[error("SNAPSHOT_TOO_LARGE")]
    TooLarge,
}

impl From<pomelo_core::task::Cancelled> for SnapshotError {
    fn from(_: pomelo_core::task::Cancelled) -> Self {
        Self::Cancelled
    }
}

/// Review overlays drawn above the board in a single mark color.
#[derive(Default)]
pub struct Marks<'a> {
    pub selection: Option<&'a BTreeSet<SelectedObject>>,
    pub measurements: Vec<Vec<Point>>,
    pub clearance: Option<(Point, Point)>,
}

pub struct SnapshotRequest<'a> {
    pub scene: &'a BoardScene,
    pub display: &'a BoardDisplay,
    /// Board-space range; it is centred and fitted into the output size.
    pub area: Bounds,
    pub width: u32,
    pub height: u32,
    /// Output pixels per logical canvas pixel. Whole-board exports normally use 1.
    pub pixel_ratio: f64,
    pub annotations: AnnotationSources<'a>,
    pub flipped: bool,
    /// Back-to-front layer order, as composed by the canvas.
    pub layer_order: &'a [LayerId],
    pub colors: &'a BTreeMap<LayerId, [f32; 4]>,
    pub fallback_color: [f32; 4],
    pub drill_color: [f32; 4],
    pub outline_color: [f32; 4],
    pub background: [f32; 4],
    pub mark_color: [f32; 4],
    pub object_colors: Option<&'a BTreeMap<SelectedObject, [f32; 3]>>,
    pub marks: Marks<'a>,
    /// Unit of the scale bar label; `None` omits the scale bar.
    pub scale_unit: Option<LengthUnit>,
    pub max_elements: usize,
}

struct Projection {
    scale: f64,
    sign: f64,
    center: Point,
    width: f64,
    height: f64,
}

impl Projection {
    fn new(area: Bounds, width: u32, height: u32, flipped: bool) -> Option<Self> {
        let (width, height) = (f64::from(width), f64::from(height));
        let span_x = (area.max.x - area.min.x).max(1e-6);
        let span_y = (area.max.y - area.min.y).max(1e-6);
        let scale = (width / span_x).min(height / span_y);
        (scale.is_finite() && scale > 0.0).then(|| Self {
            scale,
            sign: if flipped { -1.0 } else { 1.0 },
            center: area.center(),
            width,
            height,
        })
    }

    fn camera(&self) -> Camera {
        Camera {
            center: self.center,
            pixels_per_mm: self.scale,
            flipped: self.sign < 0.0,
        }
    }

    /// Board-space bounds of the output, with a small margin for strokes.
    fn visible(&self) -> Bounds {
        let half_x = self.width * 0.5 / self.scale + 1.0 / self.scale;
        let half_y = self.height * 0.5 / self.scale + 1.0 / self.scale;
        Bounds {
            min: Point::new(self.center.x - half_x, self.center.y - half_y),
            max: Point::new(self.center.x + half_x, self.center.y + half_y),
        }
    }

    fn transform(&self) -> String {
        let a = self.sign * self.scale;
        let d = -self.scale;
        let e = self.width * 0.5 - self.sign * self.center.x * self.scale;
        let f = self.height * 0.5 + self.center.y * self.scale;
        format!("matrix({a} 0 0 {d} {e} {f})")
    }
    fn inverse_transform(&self) -> String {
        let a = self.sign / self.scale;
        let d = -1.0 / self.scale;
        let e = self.center.x - self.sign * self.width * 0.5 / self.scale;
        let f = self.center.y + self.height * 0.5 / self.scale;
        format!("matrix({a} 0 0 {d} {e} {f})")
    }
}

fn color(rgba: [f32; 4]) -> String {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        byte(rgba[0]),
        byte(rgba[1]),
        byte(rgba[2])
    )
}

fn opacity(rgba: [f32; 4]) -> f32 {
    rgba[3].clamp(0.0, 1.0)
}

fn overlaps(a: Bounds, b: Bounds) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}

struct Writer<'a> {
    output: String,
    patterns: BTreeSet<FillPattern>,
    elements: usize,
    limit: usize,
    cancel: &'a CancellationToken,
    /// Minimum stroke width: one output pixel, as on the canvas.
    pixel: f64,
    tolerance: f64,
    annotation_pixels: usize,
}

impl Writer<'_> {
    /// Exterior minus the union of holes, shared by material and annotation ink.
    fn coverage_start(&mut self, rings: &[Vec<Point>]) -> Result<bool, SnapshotError> {
        let Some(outer) = rings.first().filter(|r| r.len() >= 3) else {
            return Ok(false);
        };
        let Some(bounds) = Bounds::from_points(outer.iter().copied()) else {
            return Ok(false);
        };
        self.count()?;
        let id = self.elements;
        let _ = write!(
            self.output,
            "<defs><mask id=\"coverage-{id}\" maskUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\">",
            bounds.min.x - self.pixel,
            bounds.min.y - self.pixel,
            bounds.max.x - bounds.min.x + 2.0 * self.pixel,
            bounds.max.y - bounds.min.y + 2.0 * self.pixel
        );
        for (index, ring) in rings.iter().enumerate() {
            self.count()?;
            let _ = write!(
                self.output,
                "<path d=\"{}\" fill=\"{}\" fill-rule=\"evenodd\"/>",
                rings_path(std::slice::from_ref(ring)),
                if index == 0 { "white" } else { "black" }
            );
        }
        let _ = write!(
            self.output,
            "</mask></defs><g mask=\"url(#coverage-{id})\">"
        );
        Ok(true)
    }
    fn pattern_start(&mut self, pattern: Option<FillPattern>) -> Result<bool, SnapshotError> {
        let Some(pattern) = pattern.filter(|p| *p != FillPattern::Solid) else {
            return Ok(false);
        };
        self.count()?;
        self.patterns.insert(pattern);
        let _ = write!(
            self.output,
            "<g mask=\"url(#material-mask-{})\">",
            pattern as u8
        );
        Ok(true)
    }
    fn pattern_end(&mut self, patterned: bool) {
        if patterned {
            self.output.push_str("</g>");
        }
    }
    fn pattern_defs(&mut self, projection: &Projection) -> Result<(), SnapshotError> {
        if self.patterns.is_empty() {
            return Ok(());
        }
        let used = std::mem::take(&mut self.patterns);
        let b = projection.visible();
        let x = projection.center.x - projection.sign * projection.width * 0.5 / projection.scale;
        let y = projection.center.y + projection.height * 0.5 / projection.scale;
        self.output.push_str("<defs>");
        for pattern in used {
            let id = pattern as u8;
            self.count()?;
            let _ = write!(
                self.output,
                "<pattern id=\"material-tile-{id}\" width=\"16\" height=\"16\" patternUnits=\"userSpaceOnUse\" patternTransform=\"matrix({} 0 0 {} {x} {y})\">",
                projection.sign * self.pixel,
                -self.pixel
            );
            for row in 0..16 {
                for column in 0..16 {
                    if pattern.ink(column, row) {
                        self.count()?;
                        let _ = write!(
                            self.output,
                            "<rect x=\"{column}\" y=\"{row}\" width=\"1\" height=\"1\" fill=\"white\"/>"
                        );
                    }
                }
            }
            let _ = write!(
                self.output,
                "</pattern><mask id=\"material-mask-{id}\" maskUnits=\"userSpaceOnUse\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"url(#material-tile-{id})\"/></mask>",
                b.min.x,
                b.min.y,
                b.max.x - b.min.x,
                b.max.y - b.min.y,
                b.min.x,
                b.min.y,
                b.max.x - b.min.x,
                b.max.y - b.min.y
            );
        }
        self.output.push_str("</defs>");
        Ok(())
    }

    fn count(&mut self) -> Result<(), SnapshotError> {
        self.elements += 1;
        if self.elements > self.limit {
            return Err(SnapshotError::TooLarge);
        }
        if self.elements.is_multiple_of(4096) {
            self.cancel.check()?;
        }
        Ok(())
    }

    fn stroke(&mut self, segment: &Segment, paint: &str) -> Result<(), SnapshotError> {
        self.count()?;
        let width = segment.width.max(self.pixel);
        let _ = write!(
            self.output,
            "<path d=\"{}\" stroke=\"{paint}\" stroke-width=\"{width}\" fill=\"none\"/>",
            segment_path(segment)
        );
        Ok(())
    }

    fn pad(&mut self, pad: &Pad, owner: PadPlacement, paint: &str) -> Result<(), SnapshotError> {
        let Ok(Some(outline)) = pad.outline(owner, self.tolerance) else {
            return Ok(());
        };
        self.count()?;
        match outline {
            PadOutline::Circle { center, radius } => {
                let _ = write!(
                    self.output,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{paint}\"/>",
                    center.x,
                    center.y,
                    radius.max(self.pixel * 0.5)
                );
            }
            PadOutline::Capsule { a, b, radius } => {
                let _ = write!(
                    self.output,
                    "<path d=\"M{} {}L{} {}\" stroke=\"{paint}\" stroke-width=\"{}\" fill=\"none\"/>",
                    a.x,
                    a.y,
                    b.x,
                    b.y,
                    (radius * 2.0).max(self.pixel)
                );
            }
            PadOutline::Rings(rings) => {
                let _ = write!(
                    self.output,
                    "<path d=\"{}\" fill=\"{paint}\" fill-rule=\"evenodd\"/>",
                    rings_path(&rings)
                );
            }
        }
        Ok(())
    }
}

fn segment_path(segment: &Segment) -> String {
    let (a, b) = (segment.a, segment.b);
    let Some(arc) = segment.arc else {
        return format!("M{} {}L{} {}", a.x, a.y, b.x, b.y);
    };
    let radius = arc.radius;
    let sweep = u8::from(arc.sweep > 0.0);
    if arc.sweep.abs() >= std::f64::consts::TAU - 1e-9 {
        // A full circle needs two half arcs; one arc between equal points draws nothing.
        let opposite = Point::new(arc.center.x * 2.0 - a.x, arc.center.y * 2.0 - a.y);
        return format!(
            "M{} {}A{radius} {radius} 0 1 {sweep} {} {}A{radius} {radius} 0 1 {sweep} {} {}",
            a.x, a.y, opposite.x, opposite.y, a.x, a.y
        );
    }
    let large = u8::from(arc.sweep.abs() > std::f64::consts::PI);
    format!(
        "M{} {}A{radius} {radius} 0 {large} {sweep} {} {}",
        a.x, a.y, b.x, b.y
    )
}

fn rings_path(rings: &[Vec<Point>]) -> String {
    let mut path = String::new();
    for ring in rings {
        for (index, point) in ring.iter().enumerate() {
            let _ = write!(
                path,
                "{}{} {}",
                if index == 0 { 'M' } else { 'L' },
                point.x,
                point.y
            );
        }
        path.push('Z');
    }
    path
}

fn zone_bounds(zone: &pomelo_core::model::Zone) -> Option<Bounds> {
    zone.paths
        .first()
        .and_then(|path| pomelo_core::geometry::path_bounds(path))
        .or_else(|| zone.mesh.ring_bounds.first().copied())
        .or_else(|| Bounds::from_points(zone.mesh.vertices.iter().copied()))
}

fn zone_rings(
    zone: &pomelo_core::model::Zone,
    request: &SnapshotRequest<'_>,
    tolerance: f64,
    scale: f64,
    cancel: &CancellationToken,
) -> Result<Vec<Vec<Point>>, SnapshotError> {
    let include = |index: usize, bounds: Option<Bounds>| {
        index == 0
            || bounds.is_none_or(|bounds| {
                request.display.object_filter.bounds_visible(
                    pomelo_core::display::filter::FilterKind::Void,
                    bounds,
                    scale,
                )
            })
    };
    let mut rings = Vec::new();
    if zone.paths.is_empty() {
        for (i, range) in zone.mesh.ring_offsets.windows(2).enumerate() {
            cancel.check()?;
            let Some(ring) = zone.mesh.vertices.get(range[0] as usize..range[1] as usize) else {
                return Err(SnapshotError::TooLarge);
            };
            if include(i, zone.mesh.ring_bounds.get(i).copied()) {
                rings.push(ring.to_vec());
            }
        }
    } else {
        for (i, path) in zone.paths.iter().enumerate() {
            if !include(i, pomelo_core::geometry::path_bounds(path)) {
                continue;
            }
            match flatten_path(path, tolerance, 1_000_000, cancel) {
                Ok(ring) if ring.len() >= 3 => rings.push(ring),
                Ok(_) => {}
                Err(pomelo_core::geometry::PathError::Cancelled) => {
                    return Err(SnapshotError::Cancelled);
                }
                Err(_) => return Err(SnapshotError::TooLarge),
            }
        }
    }
    Ok(rings)
}

/// Render the request as a standalone SVG document.
pub fn svg(
    request: &SnapshotRequest<'_>,
    cancel: &CancellationToken,
) -> Result<String, SnapshotError> {
    cancel.check()?;
    if request.width == 0
        || request.height == 0
        || request.width > MAX_EDGE
        || request.height > MAX_EDGE
        || !request.pixel_ratio.is_finite()
        || request.pixel_ratio <= 0.0
    {
        return Err(SnapshotError::TooLarge);
    }
    if !request.area.is_valid() {
        return Err(SnapshotError::EmptyRange);
    }
    let projection = Projection::new(request.area, request.width, request.height, request.flipped)
        .ok_or(SnapshotError::EmptyRange)?;
    let view = projection.visible();
    let display = request.display;
    let scene = request.scene;
    let mut writer = Writer {
        output: String::new(),
        patterns: BTreeSet::new(),
        elements: 0,
        limit: request.max_elements,
        cancel,
        pixel: request.pixel_ratio / projection.scale,
        tolerance: 0.25 / projection.scale,
        annotation_pixels: 0,
    };
    let (width, height) = (request.width, request.height);
    let _ = write!(
        writer.output,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\">"
    );
    let _ = write!(
        writer.output,
        "<rect width=\"{width}\" height=\"{height}\" fill=\"{}\"/>",
        color(request.background)
    );
    let _ = write!(
        writer.output,
        "<g transform=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\">",
        projection.transform()
    );
    if display.grid.visible {
        let mut grid_camera = projection.camera();
        grid_camera.pixels_per_mm /= request.pixel_ratio;
        let paint = display
            .appearance
            .color(pomelo_core::appearance::ColorTarget::Grid)
            .map_or([0.2, 0.28, 0.23, 1.0], |color| color.rgba());
        for point in display.grid.points(
            grid_camera,
            Point::new(
                projection.width / request.pixel_ratio,
                projection.height / request.pixel_ratio,
            ),
        ) {
            writer.count()?;
            let _ = write!(
                writer.output,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>",
                point.x,
                point.y,
                0.75 * writer.pixel,
                color(paint)
            );
        }
    }
    let layer_color = |layer: LayerId, category: DisplayCategory| {
        display
            .appearance
            .material(layer, category)
            .unwrap_or_else(|| {
                request
                    .colors
                    .get(&layer)
                    .copied()
                    .unwrap_or(request.fallback_color)
            })
    };
    let net_color = |layer, net, category| {
        display
            .appearance
            .net_material(net, category)
            .map(pomelo_core::appearance::RgbColor::rgba)
            .unwrap_or_else(|| copper_color(display.color_mode, layer_color(layer, category), net))
    };
    let override_color = |object: SelectedObject| {
        request
            .object_colors
            .and_then(|colors| colors.get(&object))
            .map(|&[r, g, b]| [r, g, b, 1.0])
    };
    let shadow = |mut paint: [f32; 4], layer| {
        let brightness = display.layer_brightness(layer);
        for channel in &mut paint[..3] {
            *channel *= brightness;
        }
        paint
    };
    let global = display.global_opacity;
    let logical_scale = projection.scale / request.pixel_ratio;
    let annotations = annotations::Layout::new(request, &projection, cancel)?;
    let mut commands: Vec<_> = request
        .layer_order
        .iter()
        .copied()
        .flat_map(|layer| {
            [
                DisplayCategory::Drawing,
                DisplayCategory::Zone,
                DisplayCategory::ZoneOutline,
                DisplayCategory::Trace,
                DisplayCategory::Text,
                DisplayCategory::Placement,
                DisplayCategory::Pin,
                DisplayCategory::Via,
            ]
            .map(|category| (layer, category))
        })
        .collect();
    commands.push((LayerId::UNASSIGNED, DisplayCategory::Drill));
    commands.sort_by_key(|&(layer, category)| display.display_rank(layer, category));
    let outline = color(request.outline_color);
    for segment in &scene.outline {
        if segment
            .bounds()
            .is_some_and(|bounds| overlaps(bounds, view))
        {
            let mut stroke = segment.clone();
            stroke.width = stroke.width.max(writer.pixel);
            writer.stroke(&stroke, &outline)?;
        }
    }
    for (layer, category) in commands {
        cancel.check()?;
        if category != DisplayCategory::Drill && !display.category_visible(layer, category) {
            continue;
        }
        if category == DisplayCategory::Zone
            && display.show_copper
            && display.copper_opacity > 0.0
            && display.layer_visible(layer)
        {
            for zone in scene.zones.iter().filter(|zone| zone.layer == layer) {
                let object = SelectedObject::Zone(zone.id);
                let bounds = zone_bounds(zone);
                if !bounds.is_some_and(|bounds| {
                    overlaps(bounds, view)
                        && display.object_filter.bounds_visible(
                            pomelo_core::display::filter::FilterKind::Copper,
                            bounds,
                            logical_scale,
                        )
                }) {
                    continue;
                }
                let rings = zone_rings(zone, request, writer.tolerance, logical_scale, cancel)?;
                if !writer.coverage_start(&rings)? {
                    continue;
                }
                let paint = override_color(object)
                    .unwrap_or_else(|| net_color(layer, zone.net, DisplayCategory::Zone));
                let patterned = writer.pattern_start(
                    display
                        .appearance
                        .material_pattern(layer, DisplayCategory::Zone),
                )?;
                writer.count()?;
                let _ = write!(
                    writer.output,
                    "<path d=\"{}\" fill=\"{}\" fill-opacity=\"{}\" fill-rule=\"evenodd\"/>",
                    rings_path(&rings[..1]),
                    color(shadow(paint, layer)),
                    display.copper_opacity * opacity(paint)
                );
                writer.pattern_end(patterned);
                annotations.automatic(
                    &mut writer,
                    request,
                    &projection,
                    layer,
                    DisplayCategory::Zone,
                    Some(zone.id),
                )?;
                writer.output.push_str("</g>");
            }
        }
        if category == DisplayCategory::ZoneOutline && display.show_copper {
            for zone in scene.zones.iter().filter(|zone| zone.layer == layer) {
                if !zone_bounds(zone).is_some_and(|b| {
                    overlaps(b, view)
                        && display.object_filter.bounds_visible(
                            pomelo_core::display::filter::FilterKind::Copper,
                            b,
                            logical_scale,
                        )
                }) {
                    continue;
                }
                let paint = override_color(SelectedObject::Zone(zone.id))
                    .unwrap_or_else(|| net_color(layer, zone.net, DisplayCategory::ZoneOutline));
                let paint = color(shadow(paint, layer));
                let rings = zone_rings(zone, request, writer.tolerance, logical_scale, cancel)?;
                for ring in rings {
                    writer.count()?;
                    let _ = write!(
                        writer.output,
                        "<path d=\"{}\" stroke=\"{paint}\" stroke-width=\"{}\" fill=\"none\"/>",
                        rings_path(&[ring]),
                        writer.pixel
                    );
                }
            }
        }
        if category == DisplayCategory::Placement {
            for component in &scene.components {
                let Some(body) = component.body.as_ref().filter(|b| b.layer == layer) else {
                    continue;
                };
                for region in &body.regions {
                    if !region
                        .paths
                        .first()
                        .and_then(|path| pomelo_core::geometry::path_bounds(path))
                        .is_some_and(|b| overlaps(b, view))
                    {
                        continue;
                    }
                    let mut rings = Vec::new();
                    for path in &region.paths {
                        rings.push(
                            flatten_path(path, writer.tolerance, 1_000_000, cancel).map_err(
                                |_| {
                                    if cancel.is_cancelled() {
                                        SnapshotError::Cancelled
                                    } else {
                                        SnapshotError::TooLarge
                                    }
                                },
                            )?,
                        );
                    }
                    if writer.coverage_start(&rings)? {
                        annotations.automatic(
                            &mut writer,
                            request,
                            &projection,
                            layer,
                            DisplayCategory::Placement,
                            Some(component.id),
                        )?;
                        writer.output.push_str("</g>");
                    }
                    let _ = write!(writer.output, "<g opacity=\"{}\">", 0.3 * global);
                    for edge in region.paths.iter().flatten() {
                        writer.stroke(edge, "#d9dee3")?;
                    }
                    writer.output.push_str("</g>");
                }
            }
        }
        if category == DisplayCategory::Trace
            && display.primitive_visible(layer, LayerPrimitive::Traces)
        {
            let _ = write!(writer.output, "<g opacity=\"{global}\">");
            for segment in scene
                .segments
                .iter()
                .filter(|segment| segment.layer == layer)
            {
                if !segment
                    .bounds()
                    .is_some_and(|bounds| overlaps(bounds, view))
                {
                    continue;
                }
                let paint = override_color(SelectedObject::Segment(segment.id))
                    .unwrap_or_else(|| net_color(layer, segment.net, DisplayCategory::Trace));
                let patterned = writer.pattern_start(
                    display
                        .appearance
                        .material_pattern(layer, DisplayCategory::Trace),
                )?;
                writer.stroke(segment, &color(shadow(paint, layer)))?;
                writer.pattern_end(patterned);
            }
            writer.output.push_str("</g>");
        }
        if category == DisplayCategory::Drawing
            && display.show_drawings
            && display.layer_visible(layer)
        {
            let _ = write!(writer.output, "<g opacity=\"{global}\">");
            for drawing in scene
                .drawings
                .iter()
                .filter(|drawing| drawing.layer == layer)
            {
                let paint = override_color(SelectedObject::Drawing(drawing.id))
                    .unwrap_or_else(|| layer_color(layer, DisplayCategory::Drawing));
                let paint = color(shadow(paint, layer));
                let patterned = writer.pattern_start(
                    display
                        .appearance
                        .material_pattern(layer, DisplayCategory::Drawing),
                )?;
                for segment in &drawing.segments {
                    if segment
                        .bounds()
                        .is_some_and(|bounds| overlaps(bounds, view))
                    {
                        writer.stroke(segment, &paint)?;
                    }
                }
                writer.pattern_end(patterned);
            }
            writer.output.push_str("</g>");
        }
        let _ = write!(writer.output, "<g opacity=\"{global}\">");
        if category == DisplayCategory::Pin
            && display.primitive_visible(layer, LayerPrimitive::Pads)
        {
            for pin in &scene.pins {
                let owner = PadPlacement {
                    at: pin.at,
                    angle: pin.angle,
                    mirrored: pin.mirrored,
                };
                for pad in pin
                    .pads
                    .iter()
                    .filter(|pad| pad.layer == layer && !pad.backdrill)
                {
                    if !pad
                        .bounds(owner)
                        .is_some_and(|bounds| overlaps(bounds, view))
                    {
                        continue;
                    }
                    if !display.object_filter.pad_visible(
                        pomelo_core::display::filter::FilterKind::Pin,
                        pad,
                        owner,
                        logical_scale,
                    ) {
                        continue;
                    }
                    let paint = override_color(SelectedObject::Pin(pin.id))
                        .unwrap_or_else(|| net_color(layer, pin.net, DisplayCategory::Pin));
                    let patterned = writer.pattern_start(if display.filled {
                        display
                            .appearance
                            .material_pattern(layer, DisplayCategory::Pin)
                    } else {
                        None
                    })?;
                    writer.pad(pad, owner, &color(shadow(paint, layer)))?;
                    writer.pattern_end(patterned);
                }
            }
        }
        if category == DisplayCategory::Via
            && display.primitive_visible(layer, LayerPrimitive::Vias)
        {
            for via in &scene.vias {
                let owner = PadPlacement {
                    at: via.at,
                    angle: via.angle,
                    mirrored: via.mirrored,
                };
                for pad in via
                    .pads
                    .iter()
                    .filter(|pad| pad.layer == layer && !pad.backdrill)
                {
                    if !pad
                        .bounds(owner)
                        .is_some_and(|bounds| overlaps(bounds, view))
                    {
                        continue;
                    }
                    if !display.object_filter.pad_visible(
                        pomelo_core::display::filter::FilterKind::Via,
                        pad,
                        owner,
                        logical_scale,
                    ) {
                        continue;
                    }
                    let paint = override_color(SelectedObject::Via(via.id))
                        .unwrap_or_else(|| net_color(layer, via.net, DisplayCategory::Via));
                    let patterned = writer.pattern_start(if display.filled {
                        display
                            .appearance
                            .material_pattern(layer, DisplayCategory::Via)
                    } else {
                        None
                    })?;
                    writer.pad(pad, owner, &color(shadow(paint, layer)))?;
                    writer.pattern_end(patterned);
                }
            }
        }
        writer.output.push_str("</g>");
        if category == DisplayCategory::Drill && display.show_drills {
            let paint = color(
                display
                    .appearance
                    .material(LayerId::UNASSIGNED, DisplayCategory::Drill)
                    .unwrap_or(request.drill_color),
            );
            let _ = write!(writer.output, "<g opacity=\"{global}\">");
            let drills = scene
                .pins
                .iter()
                .map(|pin| (pin.at, pin.angle, pin.mirrored, &pin.drill_shape, None))
                .chain(scene.vias.iter().map(|via| {
                    let scope: Vec<_> = via.pads.iter().map(|pad| pad.layer).collect();
                    (
                        via.at,
                        via.angle,
                        via.mirrored,
                        &via.drill_shape,
                        Some(scope),
                    )
                }));
            for (at, angle, mirrored, shape, scope) in drills {
                if !display.drill_visible(scope.as_deref()) {
                    continue;
                }
                let Some(drill) = shape.pad() else {
                    continue;
                };
                let owner = PadPlacement {
                    at,
                    angle,
                    mirrored,
                };
                if drill
                    .bounds(owner)
                    .is_some_and(|bounds| overlaps(bounds, view))
                {
                    writer.pad(&drill, owner, &paint)?;
                }
            }
            writer.output.push_str("</g>");
        }
        if category == DisplayCategory::Text {
            annotations::Layout::source(&mut writer, request, &projection, layer)?;
        }
        if matches!(
            category,
            DisplayCategory::Trace | DisplayCategory::Pin | DisplayCategory::Drill
        ) {
            annotations.automatic(&mut writer, request, &projection, layer, category, None)?;
        }
    }
    writer.pattern_defs(&projection)?;
    write_marks(&mut writer, request, view)?;
    writer.output.push_str("</g>");
    if let Some(unit) = request.scale_unit {
        write_scale_bar(&mut writer.output, &projection, unit, request.mark_color);
    }
    writer.output.push_str("</svg>");
    cancel.check()?;
    Ok(writer.output)
}

fn write_marks(
    writer: &mut Writer<'_>,
    request: &SnapshotRequest<'_>,
    view: Bounds,
) -> Result<(), SnapshotError> {
    let paint = color(request.mark_color);
    let scene = request.scene;
    if let Some(selection) = request.marks.selection.filter(|set| !set.is_empty()) {
        // Selection uses the canvas convention, a translucent white overlay, so it stays
        // distinct from measurement marks and from highlight-group colors.
        let paint = "#ffffff";
        let _ = write!(writer.output, "<g opacity=\"0.55\">");
        for segment in &scene.segments {
            if selection.contains(&SelectedObject::Segment(segment.id))
                && segment
                    .bounds()
                    .is_some_and(|bounds| overlaps(bounds, view))
            {
                writer.stroke(segment, paint)?;
            }
        }
        for pin in &scene.pins {
            if !selection.contains(&SelectedObject::Pin(pin.id)) {
                continue;
            }
            let owner = PadPlacement {
                at: pin.at,
                angle: pin.angle,
                mirrored: pin.mirrored,
            };
            for pad in pin.pads.iter().filter(|pad| {
                !pad.backdrill
                    && request
                        .display
                        .object_visible(pad.layer, SelectedObject::Pin(pin.id))
            }) {
                writer.pad(pad, owner, paint)?;
            }
        }
        for via in &scene.vias {
            if !selection.contains(&SelectedObject::Via(via.id)) {
                continue;
            }
            let owner = PadPlacement {
                at: via.at,
                angle: via.angle,
                mirrored: via.mirrored,
            };
            for pad in via.pads.iter().filter(|pad| {
                !pad.backdrill
                    && request
                        .display
                        .object_visible(pad.layer, SelectedObject::Via(via.id))
            }) {
                writer.pad(pad, owner, paint)?;
            }
        }
        for drawing in &scene.drawings {
            if selection.contains(&SelectedObject::Drawing(drawing.id)) {
                for segment in &drawing.segments {
                    writer.stroke(segment, paint)?;
                }
            }
        }
        writer.output.push_str("</g>");
    }
    let line = writer.pixel * 2.0;
    for points in &request.marks.measurements {
        writer.count()?;
        let path: String = points
            .iter()
            .enumerate()
            .map(|(index, point)| {
                format!(
                    "{}{} {}",
                    if index == 0 { 'M' } else { 'L' },
                    point.x,
                    point.y
                )
            })
            .collect();
        let _ = write!(
            writer.output,
            "<path d=\"{path}\" stroke=\"{paint}\" stroke-width=\"{line}\" fill=\"none\"/>"
        );
        for point in points {
            let _ = write!(
                writer.output,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{paint}\"/>",
                point.x,
                point.y,
                writer.pixel * 3.0
            );
        }
    }
    if let Some((a, b)) = request.marks.clearance {
        writer.count()?;
        let dash = writer.pixel * 6.0;
        let _ = write!(
            writer.output,
            "<path d=\"M{} {}L{} {}\" stroke=\"{paint}\" stroke-width=\"{line}\" \
             stroke-dasharray=\"{dash} {dash}\" fill=\"none\"/>",
            a.x, a.y, b.x, b.y
        );
        for point in [a, b] {
            let _ = write!(
                writer.output,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{paint}\"/>",
                point.x,
                point.y,
                writer.pixel * 3.0
            );
        }
    }
    Ok(())
}

/// Scale-bar label text: the tick length with an international unit symbol.
pub fn scale_label(millimeters: f64, unit: LengthUnit) -> String {
    let value = unit.from_millimeters(millimeters);
    let number = format!("{value:.6}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned();
    format!(
        "{number} {}",
        match unit {
            LengthUnit::Millimeters => "mm",
            LengthUnit::Mils => "mil",
        }
    )
}

fn write_scale_bar(output: &mut String, projection: &Projection, unit: LengthUnit, mark: [f32; 4]) {
    let maximum = (projection.width * 0.25).clamp(40.0, 400.0);
    let Some(scale) = projection.camera().scale_bar_in_unit(maximum, unit) else {
        return;
    };
    let unit_px = (projection.width.min(projection.height) / 400.0).clamp(1.0, 8.0);
    let margin = 16.0 * unit_px;
    let right = projection.width - margin;
    let bottom = projection.height - margin;
    let left = right - scale.pixels;
    let tick = 6.0 * unit_px;
    let paint = color(mark);
    let _ = write!(
        output,
        "<path d=\"M{left} {}L{left} {bottom}L{right} {bottom}L{right} {}\" stroke=\"{paint}\" \
         stroke-width=\"{}\" fill=\"none\"/>",
        bottom - tick,
        bottom - tick,
        1.5 * unit_px
    );
    let label = scale_label(scale.millimeters, unit);
    let glyph = 2.0 * unit_px;
    let text_width = label.chars().count() as f64 * GLYPH_ADVANCE * glyph;
    let path = stroke_text(
        &label,
        right - text_width,
        bottom - tick - 4.0 * unit_px,
        glyph,
    );
    let _ = write!(
        output,
        "<path d=\"{path}\" stroke=\"{paint}\" stroke-width=\"{}\" fill=\"none\" \
         stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
        0.9 * unit_px
    );
}

const GLYPH_ADVANCE: f64 = 6.0;

/// A minimal stroke font for scale labels (digits, '.', '-', 'm', 'i', 'l'), so the
/// label renders identically in SVG viewers and the PNG rasterizer without fonts.
/// Glyphs are polylines on a 4 × 6 grid with the baseline at y = 0.
fn glyph(c: char) -> &'static [&'static [(f64, f64)]] {
    match c {
        '0' => &[&[(0.0, 0.0), (4.0, 0.0), (4.0, 6.0), (0.0, 6.0), (0.0, 0.0)]],
        '1' => &[&[(1.0, 5.0), (2.0, 6.0), (2.0, 0.0)]],
        '2' => &[&[
            (0.0, 6.0),
            (4.0, 6.0),
            (4.0, 3.0),
            (0.0, 3.0),
            (0.0, 0.0),
            (4.0, 0.0),
        ]],
        '3' => &[
            &[(0.0, 6.0), (4.0, 6.0), (4.0, 0.0), (0.0, 0.0)],
            &[(0.0, 3.0), (4.0, 3.0)],
        ],
        '4' => &[
            &[(0.0, 6.0), (0.0, 3.0), (4.0, 3.0)],
            &[(4.0, 6.0), (4.0, 0.0)],
        ],
        '5' => &[&[
            (4.0, 6.0),
            (0.0, 6.0),
            (0.0, 3.0),
            (4.0, 3.0),
            (4.0, 0.0),
            (0.0, 0.0),
        ]],
        '6' => &[&[
            (4.0, 6.0),
            (0.0, 6.0),
            (0.0, 0.0),
            (4.0, 0.0),
            (4.0, 3.0),
            (0.0, 3.0),
        ]],
        '7' => &[&[(0.0, 6.0), (4.0, 6.0), (4.0, 0.0)]],
        '8' => &[
            &[(0.0, 0.0), (4.0, 0.0), (4.0, 6.0), (0.0, 6.0), (0.0, 0.0)],
            &[(0.0, 3.0), (4.0, 3.0)],
        ],
        '9' => &[&[
            (0.0, 0.0),
            (4.0, 0.0),
            (4.0, 6.0),
            (0.0, 6.0),
            (0.0, 3.0),
            (4.0, 3.0),
        ]],
        '.' => &[&[(2.0, 0.0), (2.0, 0.4)]],
        '-' => &[&[(0.5, 3.0), (3.5, 3.0)]],
        'm' => &[
            &[(0.0, 0.0), (0.0, 4.0), (4.0, 4.0), (4.0, 0.0)],
            &[(2.0, 4.0), (2.0, 0.0)],
        ],
        'i' => &[&[(2.0, 0.0), (2.0, 4.0)], &[(2.0, 5.6), (2.0, 6.0)]],
        'l' => &[&[(2.0, 0.0), (2.0, 6.0)]],
        'e' => &[&[
            (4.0, 0.0),
            (0.0, 0.0),
            (0.0, 4.0),
            (4.0, 4.0),
            (4.0, 2.0),
            (0.0, 2.0),
        ]],
        _ => &[],
    }
}

/// SVG path data for `text` with its baseline at (`x`, `baseline`) in pixel space.
fn stroke_text(text: &str, x: f64, baseline: f64, size: f64) -> String {
    let mut path = String::new();
    for (index, c) in text.chars().enumerate() {
        let origin = x + index as f64 * GLYPH_ADVANCE * size;
        for stroke in glyph(c) {
            for (point_index, (gx, gy)) in stroke.iter().enumerate() {
                let _ = write!(
                    path,
                    "{}{} {}",
                    if point_index == 0 { 'M' } else { 'L' },
                    origin + gx * size,
                    baseline - gy * size
                );
            }
        }
    }
    path
}

#[cfg(test)]
#[path = "../../tests/unit/scene/snapshot.rs"]
mod tests;
