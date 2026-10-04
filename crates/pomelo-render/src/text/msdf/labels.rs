//! Camera-dependent labels following Pomelo Web BoardLabelLayout.
use super::*;
use pomelo_core::{
    display::{BoardDisplay, DisplayCategory as Category, LayerPrimitive},
    interaction::Camera,
    model::{BoardScene, Bounds, Point},
};

pub use pomelo_core::display::LabelOptions;

#[derive(Clone, Copy)]
enum Source {
    Track(usize),
    Pin(usize),
    Via(usize),
    Zone(usize),
}
#[derive(Clone, Copy)]
struct Entry {
    source: Source,
    bounds: Bounds,
    min_scale: f64,
    sequence: usize,
}
struct Node {
    bounds: Bounds,
    min_scale: f64,
    range: std::ops::Range<usize>,
    children: Option<[usize; 2]>,
}
pub struct LabelIndex {
    scene: Arc<BoardScene>,
    pub font: Arc<MsdfFont>,
    entries: Vec<Entry>,
    nodes: Vec<Node>,
}
fn intersects(a: Bounds, b: Bounds) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}
fn expanded(b: Bounds, d: f64) -> Bounds {
    Bounds {
        min: Point::new(b.min.x - d, b.min.y - d),
        max: Point::new(b.max.x + d, b.max.y + d),
    }
}
fn zone_bounds(zone: &pomelo_core::model::Zone) -> Option<Bounds> {
    zone.paths
        .first()
        .and_then(|p| pomelo_core::geometry::path_bounds(p))
        .or_else(|| zone.mesh.ring_bounds.first().copied())
}
fn diameter(v: &pomelo_core::model::Via) -> f64 {
    v.backdrill.as_ref().map_or_else(
        || v.pads.iter().fold(v.drill, |d, p| d.max(p.width)),
        |b| b.definition.label_diameter,
    )
}

fn horizontal_pin_name_size(pad: &pomelo_core::model::Pad, angle: f64, advance: f64) -> f64 {
    let (sin, cos) = angle.sin_cos();
    let (sin, cos) = (sin.abs(), cos.abs());
    // Fit a horizontal label in the rotated pad's local width/height, rather
    // than using its larger world-space bounding box at oblique angles.
    let size = (pad.width / (advance * cos + sin)).min(pad.height / (advance * sin + cos)) * 0.85;
    size.min((pad.width * sin + pad.height * cos) * 0.65)
}

fn long_axis_pin_name_size(pad: &pomelo_core::model::Pad, advance: f64) -> f64 {
    (pad.width.max(pad.height) * 0.85 / advance).min(pad.width.min(pad.height) * 0.65)
}

fn long_axis_pin_name_angle(pad: &pomelo_core::model::Pad, pin_angle: f64) -> f64 {
    if pad.height > pad.width {
        pin_angle + std::f64::consts::FRAC_PI_2
    } else {
        // Equal axes preserve the source pin orientation.
        pin_angle
    }
}

impl LabelIndex {
    pub fn build(
        scene: Arc<BoardScene>,
        font: Arc<MsdfFont>,
        cancel: &CancellationToken,
    ) -> Result<Self, Diagnostic> {
        let mut result = Self {
            scene,
            font,
            entries: Vec::new(),
            nodes: Vec::new(),
        };
        let mut append = |source, bounds, min_scale| -> Result<(), Diagnostic> {
            if cancel.is_cancelled() {
                return Err(PrepareError::Cancelled.diagnostic());
            }
            if result.entries.len() >= 2_000_000 {
                return Err(Diagnostic::error(
                    "RENDER_LABEL_INDEX_LIMIT",
                    MessageKey::RenderFontLimit,
                ));
            }
            result
                .entries
                .try_reserve(1)
                .map_err(|_| PrepareError::Allocation.diagnostic())?;
            result.entries.push(Entry {
                source,
                bounds,
                min_scale,
                sequence: result.entries.len(),
            });
            Ok(())
        };
        for (i, s) in result.scene.segments.iter().enumerate() {
            if s.arc.is_some() || s.width <= 0.0 {
                continue;
            }
            let Some(name) = result.scene.nets.get(&s.net).filter(|n| !n.is_empty()) else {
                continue;
            };
            let length = s.a.distance(s.b);
            let size = (s.width * 0.68).min(length * 0.85 / result.font.advance(name).max(1.0));
            if size > 0.0
                && let Some(bounds) = s.bounds()
            {
                append(
                    Source::Track(i),
                    expanded(bounds, result.font.advance(name) * size + size),
                    (12.0 / s.width).max(8.0 / size),
                )?;
            }
        }
        for (i, p) in result.scene.pins.iter().enumerate() {
            let Some(name) = result.scene.nets.get(&p.net).filter(|n| !n.is_empty()) else {
                continue;
            };
            let mut size = 0.0f64;
            let mut padding = 1.0f64;
            for pad in &p.pads {
                let advance = result.font.advance(name);
                size = size
                    .max(long_axis_pin_name_size(pad, advance))
                    .max(horizontal_pin_name_size(pad, p.angle, advance));
                padding = padding
                    .max(pad.width.max(pad.height) + pad.offset.x.abs() + pad.offset.y.abs());
            }
            if size > 0.0 {
                append(
                    Source::Pin(i),
                    Bounds {
                        min: Point::new(p.at.x - padding, p.at.y - padding),
                        max: Point::new(p.at.x + padding, p.at.y + padding),
                    },
                    8.0 / size,
                )?;
            }
        }
        for (i, v) in result.scene.vias.iter().enumerate() {
            let d = diameter(v);
            if d > 0.0 {
                append(
                    Source::Via(i),
                    Bounds {
                        min: Point::new(v.at.x - d, v.at.y - d),
                        max: Point::new(v.at.x + d, v.at.y + d),
                    },
                    22.0 / d,
                )?;
            }
        }
        for (i, z) in result.scene.zones.iter().enumerate() {
            if let Some(b) = zone_bounds(z) {
                append(Source::Zone(i), b, 0.0)?;
            }
        }
        fn build(
            entries: &mut [Entry],
            base: usize,
            nodes: &mut Vec<Node>,
            cancel: &CancellationToken,
        ) -> Result<usize, Diagnostic> {
            if cancel.is_cancelled() {
                return Err(PrepareError::Cancelled.diagnostic());
            }
            let mut bounds = entries[0].bounds;
            let mut min_scale = entries[0].min_scale;
            for e in &entries[1..] {
                bounds.include(e.bounds.min);
                bounds.include(e.bounds.max);
                min_scale = min_scale.min(e.min_scale);
            }
            let index = nodes.len();
            nodes
                .try_reserve(1)
                .map_err(|_| PrepareError::Allocation.diagnostic())?;
            nodes.push(Node {
                bounds,
                min_scale,
                range: base..base + entries.len(),
                children: None,
            });
            if entries.len() > 16 {
                let mid = entries.len() / 2;
                let axis = bounds.max.x - bounds.min.x >= bounds.max.y - bounds.min.y;
                entries.select_nth_unstable_by(mid, |a, b| {
                    let center = |e: &Entry| {
                        if axis {
                            e.bounds.min.x + e.bounds.max.x
                        } else {
                            e.bounds.min.y + e.bounds.max.y
                        }
                    };
                    center(a).total_cmp(&center(b))
                });
                let (left, right) = entries.split_at_mut(mid);
                let children = [
                    build(left, base, nodes, cancel)?,
                    build(right, base + mid, nodes, cancel)?,
                ];
                nodes[index].children = Some(children);
            }
            Ok(index)
        }
        if !result.entries.is_empty() {
            build(&mut result.entries, 0, &mut result.nodes, cancel)?;
        }
        Ok(result)
    }
    fn query(&self, view: Bounds, scale: f64) -> Vec<Entry> {
        let mut result = Vec::new();
        let mut pending = vec![0];
        while let Some(i) = pending.pop() {
            let Some(n) = self.nodes.get(i) else {
                continue;
            };
            if n.min_scale > scale || !intersects(n.bounds, view) {
                continue;
            }
            if let Some([a, b]) = n.children {
                pending.push(b);
                pending.push(a);
            } else {
                result.extend(
                    self.entries[n.range.clone()]
                        .iter()
                        .copied()
                        .filter(|e| e.min_scale <= scale && intersects(e.bounds, view)),
                );
            }
        }
        result.sort_unstable_by_key(|e| e.sequence);
        result
    }
    pub fn layout(
        &self,
        camera: Camera,
        width: f64,
        height: f64,
        display: &BoardDisplay,
        options: LabelOptions,
        cancel: &CancellationToken,
    ) -> Result<PreparedGlyphs, Diagnostic> {
        let mut result = PreparedGlyphs {
            font: Arc::clone(&self.font),
            instances: Vec::new(),
            batches: Vec::new(),
            objects: Vec::new(),
            pick_quads: Vec::new(),
            diagnostics: Vec::new(),
        };
        if width <= 0.0 || height <= 0.0 || !camera.is_renderable() {
            return Ok(result);
        }
        let scale = camera.pixels_per_mm;
        let sign = if camera.flipped { -1.0 } else { 1.0 };
        let view = Bounds {
            min: Point::new(
                camera.center.x - width / 2.0 / scale,
                camera.center.y - height / 2.0 / scale,
            ),
            max: Point::new(
                camera.center.x + width / 2.0 / scale,
                camera.center.y + height / 2.0 / scale,
            ),
        };
        let visible = |p: Point, pad: f64| {
            p.x >= view.min.x - pad
                && p.x <= view.max.x + pad
                && p.y >= view.min.y - pad
                && p.y <= view.max.y + pad
        };
        let mut append = |name: &str,
                          center: Point,
                          size: f64,
                          angle: f64,
                          color: [f64; 4],
                          category: Category,
                          layer: LayerId,
                          owner: ObjectId,
                          net: u32,
                          vertical: f64,
                          independent: bool|
         -> Result<(), Diagnostic> {
            let (sin, cos) = angle.sin_cos();
            let mut min_y = f64::INFINITY;
            let mut max_y = f64::NEG_INFINITY;
            for ch in name.chars().filter(|&c| c != ' ') {
                let g = self.font.glyph(ch);
                if g.plane[2] > g.plane[0] && g.plane[3] > g.plane[1] {
                    min_y = min_y.min(g.plane[1]);
                    max_y = max_y.max(g.plane[3]);
                }
            }
            let cy = if min_y.is_finite() {
                (min_y + max_y) / 2.0
            } else {
                0.0
            };
            let mut pen = -self.font.advance(name) * size / 2.0;
            for ch in name.chars() {
                if cancel.is_cancelled() {
                    return Err(PrepareError::Cancelled.diagnostic());
                }
                let g = self.font.glyph(ch);
                if ch != ' ' {
                    if result.instances.len() >= 65_536 {
                        return Err(Diagnostic::error(
                            "RENDER_LABEL_GLYPH_LIMIT",
                            MessageKey::RenderFontLimit,
                        ));
                    }
                    let x = pen + g.plane[0] * size;
                    let y = (g.plane[1] - cy) * size * vertical;
                    result.instances.push(GlyphInstance::packet(
                        [
                            center.x + (x * cos - y * sin) * sign,
                            center.y + x * sin + y * cos,
                            (g.plane[2] - g.plane[0]) * size,
                            (g.plane[3] - g.plane[1]) * size * vertical,
                            g.uv[0],
                            g.uv[1],
                            g.uv[2],
                            g.uv[3],
                            color[0],
                            color[1],
                            color[2],
                            color[3],
                            cos,
                            sin,
                            sign,
                            f64::from(g.page) * 2.0 + f64::from(u8::from(independent)),
                        ],
                        [owner.0, category as u32, layer.0, net],
                    )?);
                }
                pen += g.advance * size;
            }
            Ok(())
        };
        let readable = |angle: f64| {
            if angle > std::f64::consts::FRAC_PI_2 {
                angle - std::f64::consts::PI
            } else if angle < -std::f64::consts::FRAC_PI_2 {
                angle + std::f64::consts::PI
            } else {
                angle
            }
        };
        // The query prunes invisible and sub-pixel labels before traversing source objects.
        for entry in self.query(expanded(view, 2.0 / scale), scale) {
            match entry.source {
                Source::Track(i) if options.track_names => {
                    let s = &self.scene.segments[i];
                    if !display.primitive_visible(s.layer, LayerPrimitive::Traces) {
                        continue;
                    }
                    let name = &self.scene.nets[&s.net];
                    let length = s.a.distance(s.b);
                    let advance = self.font.advance(name);
                    let size = (s.width * 0.68).min(length * 0.85 / advance.max(1.0));
                    if s.width * scale < 12.0 || size * scale < 8.0 {
                        continue;
                    }
                    let spacing = (advance * size + size * 3.0).max(180.0 / scale);
                    let count = (length / spacing).floor().max(1.0);
                    let padding = advance * size + size;
                    let Some((first, last)) = interval(s.a, s.b, expanded(view, padding)) else {
                        continue;
                    };
                    let first = (first * count - 0.5).ceil().max(0.0);
                    let last = (last * count - 0.5).floor().min(count - 1.0);
                    let angle = readable((s.b.y - s.a.y).atan2((s.b.x - s.a.x) * sign));
                    for index in first as i64..=last as i64 {
                        let t = (index as f64 + 0.5) / count;
                        let at =
                            Point::new(s.a.x + (s.b.x - s.a.x) * t, s.a.y + (s.b.y - s.a.y) * t);
                        if visible(at, advance * size) {
                            append(
                                name,
                                at,
                                size,
                                angle,
                                [0.95, 0.97, 1.0, 0.9],
                                Category::Trace,
                                s.layer,
                                s.id,
                                s.net.0,
                                1.0,
                                false,
                            )?;
                        }
                    }
                }
                Source::Pin(i) if options.pin_names => {
                    let p = &self.scene.pins[i];
                    if !visible(p.at, 1.0) {
                        continue;
                    }
                    let name = &self.scene.nets[&p.net];
                    let advance = self.font.advance(name);
                    for pad in &p.pads {
                        let kind = if p.die.is_some() && pad.layer.0 >= 0x20000 {
                            LayerPrimitive::Traces
                        } else {
                            LayerPrimitive::Pads
                        };
                        if !display.primitive_visible(pad.layer, kind) {
                            continue;
                        }
                        let size = if display.horizontal_pin_names {
                            horizontal_pin_name_size(pad, p.angle, advance)
                        } else {
                            long_axis_pin_name_size(pad, advance)
                        };
                        if size * scale < 8.0 {
                            continue;
                        }
                        append(
                            name,
                            Point::new(p.at.x + pad.offset.x, p.at.y + pad.offset.y),
                            size,
                            if display.horizontal_pin_names {
                                0.0
                            } else {
                                let angle = long_axis_pin_name_angle(pad, p.angle);
                                readable(angle.sin().atan2(angle.cos() * sign))
                            },
                            [1.0, 1.0, 1.0, 0.9],
                            if kind == LayerPrimitive::Traces {
                                Category::Trace
                            } else {
                                Category::Pin
                            },
                            pad.layer,
                            p.id,
                            p.net.0,
                            1.0,
                            false,
                        )?;
                    }
                }
                Source::Via(i) if options.thru_labels || options.bb_labels || options.via_names => {
                    let v = &self.scene.vias[i];
                    let d = diameter(v);
                    if !visible(v.at, d)
                        || d * scale < 22.0
                        || !display.drill_scope_visible(Some(
                            &v.pads.iter().map(|p| p.layer).collect::<Vec<_>>(),
                        ))
                    {
                        continue;
                    }
                    let through = self.scene.layers.len() > 1
                        && v.start_layer == Some(LayerId(0))
                        && v.end_layer == Some(LayerId(self.scene.layers.len() as u32 - 1));
                    let span = if let Some(b) = &v.backdrill {
                        if options.bb_labels {
                            b.definition
                                .spans
                                .iter()
                                .map(|s| {
                                    format!(
                                        "B{}-{}-{}",
                                        s.start_layer.0 + 1,
                                        s.stop_layer.0 + 1,
                                        s.protected_layer.0 + 1
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(",")
                        } else {
                            String::new()
                        }
                    } else if let (Some(a), Some(b)) = (v.start_layer, v.end_layer)
                        && v.drill > 0.0
                        && b.0 > a.0
                        && (b.0 as usize) < self.scene.layers.len()
                        && if through {
                            options.thru_labels
                        } else {
                            options.bb_labels
                        }
                    {
                        format!("{}:{}", a.0 + 1, b.0 + 1)
                    } else {
                        String::new()
                    };
                    let name = if options.via_names {
                        self.scene.nets.get(&v.net).map_or("", String::as_str)
                    } else {
                        ""
                    };
                    let fraction = if v.backdrill.is_some() {
                        if d > v.drill { 0.85 } else { 0.95 }
                    } else {
                        0.75
                    };
                    let size = if name.is_empty() {
                        0.0
                    } else {
                        (d * fraction / self.font.advance(name)).min(d * 0.4)
                    };
                    let show = !name.is_empty()
                        && size * scale >= if v.backdrill.is_some() { 4.0 } else { 8.0 };
                    let offset = if !span.is_empty() && show {
                        d * 0.25
                    } else {
                        0.0
                    };
                    if !span.is_empty() {
                        append(
                            &span,
                            Point::new(v.at.x, v.at.y + offset),
                            d * if through { 0.85 } else { 0.68 } / self.font.advance(&span),
                            0.0,
                            if through {
                                [1.0; 4]
                            } else {
                                [0.0, 1.0, 1.0, 1.0]
                            },
                            Category::Drill,
                            LayerId::UNASSIGNED,
                            v.id,
                            v.net.0,
                            1.0,
                            true,
                        )?;
                    }
                    if show {
                        append(
                            name,
                            Point::new(v.at.x, v.at.y - offset),
                            size,
                            0.0,
                            [1.0; 4],
                            Category::Drill,
                            LayerId::UNASSIGNED,
                            v.id,
                            v.net.0,
                            1.0,
                            false,
                        )?;
                    }
                }
                Source::Zone(i)
                    if options.zone_names
                        && display.show_copper
                        && display.copper_opacity > 0.0 =>
                {
                    let z = &self.scene.zones[i];
                    if !display.layer_visible(z.layer) {
                        continue;
                    }
                    let Some(name) = self.scene.nets.get(&z.net).filter(|n| !n.is_empty()) else {
                        continue;
                    };
                    let b = entry.bounds;
                    let x0 = view.min.x.max(b.min.x);
                    let x1 = view.max.x.min(b.max.x);
                    let y0 = view.min.y.max(b.min.y);
                    let y1 = view.max.y.min(b.max.y);
                    let pixels = ((x1 - x0) * scale * 0.2 / self.font.advance(name).max(1.0))
                        .min(height * 0.2);
                    if pixels < 10.0 {
                        continue;
                    }
                    let size = pixels / scale;
                    if size > (y1 - y0) / 4.0 {
                        continue;
                    }
                    for t in [0.25, 0.5, 0.75] {
                        append(
                            name,
                            Point::new(x0 + (x1 - x0) * t, y1 - (y1 - y0) * t),
                            size,
                            0.0,
                            [0.88, 0.91, 0.94, 0.62 * f64::from(display.copper_opacity)],
                            Category::Zone,
                            z.layer,
                            z.id,
                            z.net.0,
                            0.9,
                            true,
                        )?;
                    }
                }
                _ => {}
            }
        }
        result.instances.sort_by_key(|i| i.ids[2]);
        for (index, i) in result.instances.iter().enumerate() {
            if let Some(batch) = result.batches.last_mut().filter(|b| b.layer.0 == i.ids[2]) {
                batch.count += 1;
            } else {
                result.batches.push(crate::tracks::TraceBatch {
                    layer: LayerId(i.ids[2]),
                    start: index as u32,
                    count: 1,
                    outline: false,
                });
            }
        }
        Ok(result)
    }
}
fn interval(a: Point, b: Point, bounds: Bounds) -> Option<(f64, f64)> {
    let (mut first, mut last) = (0.0f64, 1.0f64);
    for (origin, delta, min, max) in [
        (a.x, b.x - a.x, bounds.min.x, bounds.max.x),
        (a.y, b.y - a.y, bounds.min.y, bounds.max.y),
    ] {
        if delta == 0.0 {
            if origin < min || origin > max {
                return None;
            }
            continue;
        }
        let t0 = (min - origin) / delta;
        let t1 = (max - origin) / delta;
        first = first.max(t0.min(t1));
        last = last.min(t0.max(t1));
        if first > last {
            return None;
        }
    }
    Some((first, last))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::model::{DrillShape, NetId, Pad, PadKind, Pin};

    fn index(angle: f64) -> LabelIndex {
        index_with_pad(angle, 0.5, 3.0)
    }

    fn index_with_pad(angle: f64, width: f64, height: f64) -> LabelIndex {
        let glyph = Glyph {
            uv: [0.0, 0.0, 1.0, 1.0],
            plane: [0.0, 0.0, 1.0, 1.0],
            advance: 1.0,
            page: 0,
        };
        let font = Arc::new(MsdfFont {
            glyphs: ['G', 'N', 'D', '?']
                .into_iter()
                .map(|ch| (ch, glyph))
                .collect(),
            pages: Vec::new(),
            cap_height: 1.0,
        });
        let scene = Arc::new(BoardScene {
            layers: Vec::new(),
            special_layers: Vec::new(),
            nets: [(NetId(1), "GND".into())].into(),
            segments: Vec::new(),
            pins: vec![Pin {
                id: ObjectId(11),
                owner_id: ObjectId(7),
                net: NetId(1),
                name: "1".into(),
                reference: "U1".into(),
                at: Point::new(2.0, 3.0),
                angle,
                mirrored: false,
                drill: 0.0,
                drill_shape: DrillShape {
                    width: 0.0,
                    height: 0.0,
                    plated: false,
                },
                pads: vec![Pad {
                    layer: LayerId(0),
                    width,
                    height,
                    offset: Point::new(0.5, -0.25),
                    kind: PadKind(3),
                    corner: 0.0,
                    inner_diameter: None,
                    custom: None,
                    backdrill: false,
                    backdrill_base: false,
                }],
                stackup_region: None,
                die: None,
            }],
            components: Vec::new(),
            vias: Vec::new(),
            zones: Vec::new(),
            outline: Vec::new(),
            texts: Vec::new(),
            drawing_layers: Vec::new(),
            drawings: Vec::new(),
            bounds: Bounds {
                min: Point::new(-10.0, -10.0),
                max: Point::new(10.0, 10.0),
            },
            diagnostics: Vec::new(),
        });
        LabelIndex::build(scene, font, &CancellationToken::default()).unwrap()
    }

    #[test]
    fn pin_label_toggle_retains_geometry_and_restores_the_same_packets() {
        let index = index(std::f64::consts::FRAC_PI_2);
        let mut display = BoardDisplay {
            horizontal_pin_names: true,
            ..BoardDisplay::default()
        };
        let camera = Camera {
            center: Point::new(2.0, 3.0),
            pixels_per_mm: 1000.0,
            flipped: false,
        };
        let layout = |display: &BoardDisplay| {
            index
                .layout(
                    camera,
                    1000.0,
                    1000.0,
                    display,
                    display.label_options,
                    &CancellationToken::default(),
                )
                .unwrap()
        };
        let visible = layout(&display);
        assert_eq!(visible.instances.len(), 3);
        display.label_options.pin_names = false;
        assert!(layout(&display).instances.is_empty());
        assert_eq!(index.scene.pins.len(), 1);
        display.label_options.pin_names = true;
        let restored = layout(&display);
        assert_eq!(visible.instances.len(), restored.instances.len());
        for (before, after) in visible.instances.iter().zip(&restored.instances) {
            assert_eq!(before.xywh, after.xywh);
            assert_eq!(before.uv, after.uv);
            assert_eq!(before.color, after.color);
            assert_eq!(before.rotation, after.rotation);
            assert_eq!(before.low, after.low);
            assert_eq!(before.ids, after.ids);
        }
    }
    #[test]
    fn horizontal_pin_names_are_upright_for_rotated_and_flipped_pads() {
        for angle in [
            0.0,
            0.5,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            -0.5,
        ] {
            let index = index(angle);
            for flipped in [false, true] {
                let display = BoardDisplay {
                    horizontal_pin_names: true,
                    ..BoardDisplay::default()
                };
                let labels = index
                    .layout(
                        Camera {
                            center: Point::new(2.0, 3.0),
                            pixels_per_mm: 1000.0,
                            flipped,
                        },
                        1000.0,
                        1000.0,
                        &display,
                        LabelOptions::default(),
                        &CancellationToken::default(),
                    )
                    .unwrap();
                assert_eq!(labels.instances.len(), 3, "angle={angle} flipped={flipped}");
                for glyph in labels.instances {
                    assert_eq!(
                        &glyph.rotation[..3],
                        &[1.0, 0.0, if flipped { -1.0 } else { 1.0 }]
                    );
                    assert_eq!(glyph.ids, [11, Category::Pin as u32, 0, 1]);
                }
            }
        }
    }

    #[test]
    fn horizontal_pin_names_are_not_pruned_by_rotated_label_size() {
        // A square at45deg admits a slightly larger horizontal box. Retain a
        // genuine override-only LOD witness after automatic long-axis fitting.
        let index = index_with_pad(std::f64::consts::FRAC_PI_4, 0.5, 0.5);
        let mut display = BoardDisplay::default();
        let camera = Camera {
            center: Point::new(2.0, 3.0),
            pixels_per_mm: 56.0,
            flipped: false,
        };
        let rotated = index
            .layout(
                camera,
                500.0,
                500.0,
                &display,
                LabelOptions::default(),
                &CancellationToken::default(),
            )
            .unwrap();
        assert!(rotated.instances.is_empty());
        display.horizontal_pin_names = true;
        let horizontal = index
            .layout(
                camera,
                500.0,
                500.0,
                &display,
                LabelOptions::default(),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(horizontal.instances.len(), 3);
    }

    #[test]
    fn horizontal_pin_name_box_fits_oblique_pad_local_dimensions() {
        let index = index(0.0);
        let pad = &index.scene.pins[0].pads[0];
        for angle in [0.0, 0.3, 0.8, std::f64::consts::FRAC_PI_2, -1.2] {
            let size = horizontal_pin_name_size(pad, angle, 3.0);
            let (sin, cos) = angle.sin_cos();
            let local_width = size * (3.0 * cos.abs() + sin.abs());
            let local_height = size * (3.0 * sin.abs() + cos.abs());
            assert!(
                local_width <= pad.width * 0.85 + 1e-12
                    && local_height <= pad.height * 0.85 + 1e-12,
                "angle={angle}: {local_width}x{local_height}"
            );
        }
    }

    #[test]
    fn automatic_pin_names_follow_each_pads_long_axis_and_remain_readable_when_flipped() {
        for angle in [
            0.0,
            0.35,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI - 0.35,
            -0.7,
        ] {
            let LabelIndex { scene, font, .. } = index(angle);
            let mut scene = Arc::try_unwrap(scene).ok().unwrap();
            let mut horizontal = scene.pins[0].pads[0].clone();
            horizontal.layer = LayerId(1);
            horizontal.width = 3.0;
            horizontal.height = 0.5;
            scene.pins[0].pads.push(horizontal);
            let index =
                LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap();
            for flipped in [false, true] {
                let display = BoardDisplay::default();
                let labels = index
                    .layout(
                        Camera {
                            center: Point::new(2.0, 3.0),
                            pixels_per_mm: 100.0,
                            flipped,
                        },
                        500.0,
                        500.0,
                        &display,
                        LabelOptions::default(),
                        &CancellationToken::default(),
                    )
                    .unwrap();
                assert_eq!(labels.instances.len(), 6);
                for glyph in labels.instances {
                    let axis = angle
                        + if glyph.ids[2] == 0 {
                            std::f64::consts::FRAC_PI_2
                        } else {
                            0.0
                        };
                    let world_x = f64::from(glyph.rotation[0] * glyph.rotation[2]);
                    let world_y = f64::from(glyph.rotation[1]);
                    assert!(
                        (world_x * axis.sin() - world_y * axis.cos()).abs() < 1e-6,
                        "layer={} angle={angle} flipped={flipped}: baseline must follow local long axis",
                        glyph.ids[2]
                    );
                    assert!(
                        glyph.rotation[0] >= -1e-6,
                        "camera-readable baseline must face screen-right"
                    );
                    assert!(
                        (glyph.xywh[2] - 0.325).abs() < 1e-6,
                        "size must fit advance along3mm and cap height across0.5mm"
                    );
                    assert_eq!(glyph.ids[0], 11);
                    assert_eq!(glyph.ids[1], Category::Pin as u32);
                }
            }
        }
    }

    #[test]
    fn automatic_long_axis_names_survive_index_lod_pruning_at_the_actual_fitted_size() {
        for (width, height) in [(0.5, 3.0), (3.0, 0.5)] {
            let index = index_with_pad(0.0, width, height);
            let display = BoardDisplay::default();
            for (scale, expected) in [(24.0, 0), (25.0, 3)] {
                let labels = index
                    .layout(
                        Camera {
                            center: Point::new(2.0, 3.0),
                            pixels_per_mm: scale,
                            flipped: false,
                        },
                        500.0,
                        500.0,
                        &display,
                        LabelOptions::default(),
                        &CancellationToken::default(),
                    )
                    .unwrap();
                assert_eq!(
                    labels.instances.len(),
                    expected,
                    "{width}x{height}mm at{scale}px/mm"
                );
            }
            assert!(
                index
                    .entries
                    .iter()
                    .find(|entry| matches!(entry.source, Source::Pin(0)))
                    .unwrap()
                    .min_scale
                    < 25.0
            );
        }
    }

    #[test]
    fn automatic_equal_axis_pads_keep_source_pin_orientation() {
        let angle = 0.7;
        let index = index_with_pad(angle, 1.0, 1.0);
        let display = BoardDisplay::default();
        let labels = index
            .layout(
                Camera {
                    center: Point::new(2.0, 3.0),
                    pixels_per_mm: 100.0,
                    flipped: false,
                },
                500.0,
                500.0,
                &display,
                LabelOptions::default(),
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(labels.instances.len(), 3);
        for glyph in labels.instances {
            assert!((f64::from(glyph.rotation[0]) - angle.cos()).abs() < 1e-6);
            assert!((f64::from(glyph.rotation[1]) - angle.sin()).abs() < 1e-6);
        }
    }
}
