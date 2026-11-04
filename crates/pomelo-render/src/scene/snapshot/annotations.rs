//! Export the actual canvas glyph packets, without depending on installed fonts.
//! MSDF coverage uses the native shader's linear sampling, median and edge softness.
//! Small screen-aligned PNG tiles preserve those pixels inside the SVG compositor.
use super::*;
use crate::text::{
    instances::PreparedTextInstances,
    msdf::{AtlasPage, GlyphInstance, LabelIndex, PreparedGlyphs},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use pomelo_core::{display::filter::FilterKind, model::ObjectId};

#[derive(Default)]
pub struct AnnotationSources<'a> {
    pub source_glyphs: Option<&'a PreparedGlyphs>,
    pub source_strokes: Option<&'a PreparedTextInstances>,
    pub label_index: Option<&'a LabelIndex>,
}

pub(super) struct Layout {
    labels: Option<PreparedGlyphs>,
    groups: BTreeMap<(LayerId, u32), Vec<usize>>,
}
impl Layout {
    pub fn new(
        request: &SnapshotRequest<'_>,
        p: &Projection,
        cancel: &CancellationToken,
    ) -> Result<Self, SnapshotError> {
        let mut camera = p.camera();
        camera.pixels_per_mm /= request.pixel_ratio;
        let labels = request
            .annotations
            .label_index
            .map(|index| {
                index
                    .layout(
                        camera,
                        p.width / request.pixel_ratio,
                        p.height / request.pixel_ratio,
                        request.display,
                        request.display.label_options,
                        cancel,
                    )
                    .map_err(|_| {
                        if cancel.is_cancelled() {
                            SnapshotError::Cancelled
                        } else {
                            SnapshotError::TooLarge
                        }
                    })
            })
            .transpose()?;
        let mut groups = BTreeMap::<_, Vec<_>>::new();
        if let Some(source) = &labels {
            for (i, g) in source.instances.iter().enumerate() {
                groups
                    .entry((LayerId(g.ids[2]), g.ids[1]))
                    .or_default()
                    .push(i);
            }
        }
        Ok(Self { labels, groups })
    }
    pub fn automatic(
        &self,
        writer: &mut Writer<'_>,
        request: &SnapshotRequest<'_>,
        p: &Projection,
        layer: LayerId,
        category: DisplayCategory,
        owner: Option<ObjectId>,
    ) -> Result<(), SnapshotError> {
        let Some(source) = &self.labels else {
            return Ok(());
        };
        let Some(indices) = self.groups.get(&(layer, category as u32)) else {
            return Ok(());
        };
        let brightness = if category == DisplayCategory::Placement {
            1.0
        } else {
            request.display.layer_brightness(layer)
        };
        for &i in indices {
            let g = &source.instances[i];
            if owner.is_some_and(|id| id.0 != g.ids[0]) {
                continue;
            }
            let mut paint = g.color;
            for c in &mut paint[..3] {
                *c *= brightness;
            }
            if g.rotation[3] as u32 & 1 == 0 {
                paint[3] *= request.display.global_opacity;
            }
            glyph(writer, p, source, g, paint)?;
        }
        Ok(())
    }
    pub fn source(
        writer: &mut Writer<'_>,
        request: &SnapshotRequest<'_>,
        p: &Projection,
        layer: LayerId,
    ) -> Result<(), SnapshotError> {
        let display = request.display;
        if !display.show_texts || !display.layer_visible(layer) {
            return Ok(());
        }
        let scale = p.scale / request.pixel_ratio;
        let paint = |owner: Option<ObjectId>| {
            let mut rgba = owner
                .and_then(|id| {
                    request
                        .object_colors
                        .and_then(|colors| colors.get(&SelectedObject::Drawing(id)))
                })
                .map(|&[r, g, b]| [r, g, b, 1.0])
                .or_else(|| display.appearance.material(layer, DisplayCategory::Text))
                .unwrap_or_else(|| {
                    request
                        .colors
                        .get(&layer)
                        .copied()
                        .unwrap_or(request.fallback_color)
                });
            for c in &mut rgba[..3] {
                *c *= display.layer_brightness(layer);
            }
            rgba[3] *= display.global_opacity;
            rgba
        };
        let patterned = writer.pattern_start(
            display
                .appearance
                .material_pattern(layer, DisplayCategory::Text),
        )?;
        if let Some(source) = request.annotations.source_glyphs {
            for g in source.instances.iter().filter(|g| g.ids[2] == layer.0) {
                if !display
                    .object_filter
                    .visible(FilterKind::Text, f64::from(g.low[3]), scale)
                {
                    continue;
                }
                let mut rgba = paint((g.low[2] == 1.0).then_some(ObjectId(g.ids[0])));
                for (channel, multiplier) in rgba.iter_mut().zip(g.color) {
                    *channel *= multiplier;
                }
                glyph(writer, p, source, g, rgba)?;
            }
        }
        if let Some(source) = request.annotations.source_strokes {
            for stroke in source.instances.iter().filter(|s| s.ids[1] == layer.0) {
                let height = f64::from(f32::from_bits(stroke.flags[2]));
                if !display
                    .object_filter
                    .visible(FilterKind::Text, height, scale)
                {
                    continue;
                }
                let rgba = paint((stroke.flags[1] == 1).then_some(ObjectId(stroke.flags[0])));
                let a = Point::new(
                    f64::from(stroke.a[0]) + f64::from(stroke.a[2]),
                    f64::from(stroke.a[1]) + f64::from(stroke.a[3]),
                );
                let b = Point::new(
                    f64::from(stroke.b[0]) + f64::from(stroke.b[2]),
                    f64::from(stroke.b[1]) + f64::from(stroke.b[3]),
                );
                let width = f64::from(f32::from_bits(stroke.ids[3])).max(writer.pixel);
                writer.count()?;
                let _ = write!(
                    writer.output,
                    "<path d=\"M{} {}L{} {}\" stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{width}\" fill=\"none\"/>",
                    a.x,
                    a.y,
                    b.x,
                    b.y,
                    color(rgba),
                    opacity(rgba)
                );
            }
        }
        writer.pattern_end(patterned);
        Ok(())
    }
}

const MAX_ANNOTATION_PIXELS: usize = 128 * 1024 * 1024;
const MAX_SVG_BYTES: usize = 128 * 1024 * 1024;
const TILE: u32 = 256;

fn sample(page: &AtlasPage, u: f64, v: f64) -> f64 {
    let x = u * f64::from(page.width) - 0.5;
    let y = v * f64::from(page.height) - 0.5;
    let a = x.floor();
    let b = y.floor();
    let tx = x - a;
    let ty = y - b;
    let texel = |x: f64, y: f64, c: usize| {
        let x = x.clamp(0.0, f64::from(page.width - 1)) as usize;
        let y = y.clamp(0.0, f64::from(page.height - 1)) as usize;
        f64::from(page.rgba[(y * page.width as usize + x) * 4 + c]) / 255.0
    };
    let rgb: [f64; 3] = std::array::from_fn(|c| {
        let top = texel(a, b, c) * (1.0 - tx) + texel(a + 1.0, b, c) * tx;
        let bottom = texel(a, b + 1.0, c) * (1.0 - tx) + texel(a + 1.0, b + 1.0, c) * tx;
        top * (1.0 - ty) + bottom * ty
    });
    rgb[0].min(rgb[1]).max(rgb[0].max(rgb[1]).min(rgb[2]))
}

fn glyph(
    writer: &mut Writer<'_>,
    p: &Projection,
    source: &PreparedGlyphs,
    g: &GlyphInstance,
    paint: [f32; 4],
) -> Result<(), SnapshotError> {
    writer.cancel.check()?;
    if paint[3] <= 0.0 {
        return Ok(());
    }
    let Some(page) = source.font.pages.iter().find(|page| page.page == g.page()) else {
        return Err(SnapshotError::TooLarge);
    };
    if page.width == 0
        || page.height == 0
        || page.rgba.len() != page.width as usize * page.height as usize * 4
    {
        return Err(SnapshotError::TooLarge);
    }
    let [cos, sin, mirror, _] = g.rotation.map(f64::from);
    let [x, y, w, h] = g.xywh.map(f64::from);
    let x = x + f64::from(g.low[0]);
    let y = y + f64::from(g.low[1]);
    let origin = [
        p.width * 0.5 + p.sign * (x - p.center.x) * p.scale,
        p.height * 0.5 - (y - p.center.y) * p.scale,
    ];
    let a = [p.sign * p.scale * cos * mirror * w, -p.scale * sin * w];
    let b = [-p.sign * p.scale * sin * mirror * h, -p.scale * cos * h];
    let det = a[0] * b[1] - a[1] * b[0];
    if !det.is_finite() || det.abs() < 1e-20 {
        return Ok(());
    }
    let corners = [
        [origin[0], origin[1]],
        [origin[0] + a[0], origin[1] + a[1]],
        [origin[0] + b[0], origin[1] + b[1]],
        [origin[0] + a[0] + b[0], origin[1] + a[1] + b[1]],
    ];
    let lo = |axis, edge: f64| {
        corners
            .iter()
            .map(|p| p[axis])
            .fold(f64::INFINITY, f64::min)
            .floor()
            .clamp(0.0, edge) as u32
    };
    let hi = |axis, edge: f64| {
        corners
            .iter()
            .map(|p| p[axis])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil()
            .clamp(0.0, edge) as u32
    };
    let (left, top, right, bottom) = (
        lo(0, p.width),
        lo(1, p.height),
        hi(0, p.width),
        hi(1, p.height),
    );
    if left >= right || top >= bottom {
        return Ok(());
    }
    writer.annotation_pixels = writer
        .annotation_pixels
        .saturating_add((right - left) as usize * (bottom - top) as usize);
    if writer.annotation_pixels > MAX_ANNOTATION_PIXELS {
        return Err(SnapshotError::TooLarge);
    }
    let [u0, v0, u1, v1] = g.uv.map(f64::from);
    let du = (u1 - u0) * f64::from(page.width);
    let dv = (v0 - v1) * f64::from(page.height);
    let dx = (du * b[1] / det).hypot(dv * (-a[1]) / det);
    let dy = (du * (-b[0]) / det).hypot(dv * a[0] / det);
    let softness = 0.5 / (4.0 / dx.max(dy).max(0.0001)).max(1.0);
    let _ = write!(writer.output, "<g transform=\"{}\">", p.inverse_transform());
    for tile_y in (top..bottom).step_by(TILE as usize) {
        for tile_x in (left..right).step_by(TILE as usize) {
            writer.cancel.check()?;
            let (width, height) = ((right - tile_x).min(TILE), (bottom - tile_y).min(TILE));
            let mut tile = image::RgbaImage::new(width, height);
            let mut ink = false;
            for (x, y, pixel) in tile.enumerate_pixels_mut() {
                let dx = f64::from(tile_x + x) + 0.5 - origin[0];
                let dy = f64::from(tile_y + y) + 0.5 - origin[1];
                let qx = (dx * b[1] - dy * b[0]) / det;
                let qy = (a[0] * dy - a[1] * dx) / det;
                if !(0.0..=1.0).contains(&qx) || !(0.0..=1.0).contains(&qy) {
                    continue;
                }
                let distance = sample(page, u0 + (u1 - u0) * qx, v1 + (v0 - v1) * qy);
                let t = ((distance - (0.5 - softness)) / (2.0 * softness)).clamp(0.0, 1.0);
                let alpha = f64::from(paint[3]) * t * t * (3.0 - 2.0 * t);
                let alpha = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
                if alpha == 0 {
                    continue;
                }
                ink = true;
                *pixel = image::Rgba(
                    [paint[0], paint[1], paint[2], 0.0]
                        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8),
                );
                pixel[3] = alpha;
            }
            if !ink {
                continue;
            }
            writer.count()?;
            let mut encoded = std::io::Cursor::new(Vec::new());
            tile.write_to(&mut encoded, image::ImageFormat::Png)
                .map_err(|_| SnapshotError::TooLarge)?;
            let encoded = STANDARD.encode(encoded.into_inner());
            if writer.output.len().saturating_add(encoded.len()) > MAX_SVG_BYTES {
                return Err(SnapshotError::TooLarge);
            }
            let _ = write!(
                writer.output,
                "<image x=\"{tile_x}\" y=\"{tile_y}\" width=\"{width}\" height=\"{height}\" href=\"data:image/png;base64,{encoded}\"/>"
            );
        }
    }
    writer.output.push_str("</g>");
    Ok(())
}
