//! Web-compatible MSDF assets and glyph packets. No GPU or platform dependencies.
use crate::{split_position, tracks::PrepareError};
use pomelo_core::{
    i18n::MessageKey,
    model::{BoardText, Diagnostic, LayerId, ObjectId, TextAlignment},
    task::CancellationToken,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

mod resources {
    include!("../../../../assets/fonts/source-han-sans/pages.rs");
}
mod labels;
pub use labels::{LabelIndex, LabelOptions};

#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub uv: [f64; 4],
    pub plane: [f64; 4],
    pub advance: f64,
    pub page: u16,
}
#[derive(Debug)]
pub struct AtlasPage {
    pub page: u16,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Debug)]
pub struct MsdfFont {
    pub glyphs: BTreeMap<char, Glyph>,
    pub pages: Vec<AtlasPage>,
    pub cap_height: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    atlas: AtlasMetadata,
    metrics: Metrics,
    glyphs: Vec<GlyphMetadata>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AtlasMetadata {
    size: f64,
    distance_range: f64,
    width: u32,
    height: u32,
    y_origin: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metrics {
    cap_height: f64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GlyphMetadata {
    unicode: u32,
    advance: f64,
    plane_bounds: Option<Rectangle>,
    atlas_bounds: Option<Rectangle>,
}
#[derive(Deserialize)]
struct Rectangle {
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
}
impl Rectangle {
    fn values(&self) -> [f64; 4] {
        [self.left, self.bottom, self.right, self.top]
    }
}

impl MsdfFont {
    /// Load just the Unicode blocks used by this immutable board and its net names.
    /// Atlas PNGs retain the Web's RGB data without gamma conversion or regeneration.
    pub fn bundled<'a>(
        texts: impl IntoIterator<Item = &'a str>,
        cancellation: &CancellationToken,
    ) -> Result<Self, Diagnostic> {
        let invalid = || {
            Diagnostic::error(
                "RENDER_MSDF_RESOURCE_INVALID",
                MessageKey::RenderFontResourceInvalid,
            )
        };
        let core: Metadata =
            serde_json::from_slice(resources::PAGES[0].1).map_err(|_| invalid())?;
        let core_characters: BTreeSet<_> = core.glyphs.iter().map(|glyph| glyph.unicode).collect();
        let mut needed = BTreeSet::from([0u16]);
        let mut characters = 0;
        for text in texts {
            for ch in text.chars() {
                if cancellation.is_cancelled() {
                    return Err(PrepareError::Cancelled.diagnostic());
                }
                characters += 1;
                if characters > 2_000_000 {
                    return Err(Diagnostic::error(
                        "RENDER_MSDF_SCAN_LIMIT",
                        MessageKey::RenderFontLimit,
                    ));
                }
                if !(core_characters.contains(&(ch as u32)) || ['\n', '\r', '\t'].contains(&ch)) {
                    needed.insert(((ch as u32 >> 8) + 1) as u16);
                }
            }
        }
        let mut font = Self {
            glyphs: BTreeMap::new(),
            pages: Vec::new(),
            cap_height: 0.733,
        };
        let mut decoded_bytes = 0usize;
        for &(page, json, png) in resources::PAGES {
            if !needed.contains(&page) {
                continue;
            }
            if cancellation.is_cancelled() {
                return Err(PrepareError::Cancelled.diagnostic());
            }
            let data: Metadata = serde_json::from_slice(json).map_err(|_| invalid())?;
            if data.atlas.y_origin != "bottom"
                || data.atlas.size != 42.0
                || data.atlas.distance_range != 4.0
                || data.atlas.width == 0
                || data.atlas.height == 0
                || data.atlas.width > 2048
                || data.atlas.height > 2048
                || !data.metrics.cap_height.is_finite()
                || data.metrics.cap_height <= 0.0
            {
                return Err(invalid());
            }
            let bytes = data.atlas.width as usize * data.atlas.height as usize * 4;
            decoded_bytes = decoded_bytes.checked_add(bytes).ok_or_else(invalid)?;
            if decoded_bytes > 128 * 1024 * 1024 {
                return Err(Diagnostic::error(
                    "RENDER_MSDF_ATLAS_LIMIT",
                    MessageKey::RenderFontLimit,
                ));
            }
            let mut reader =
                image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png);
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(2048);
            limits.max_image_height = Some(2048);
            limits.max_alloc = Some(32 * 1024 * 1024);
            reader.limits(limits);
            let rgba = reader.decode().map_err(|_| invalid())?.to_rgba8();
            if rgba.dimensions() != (data.atlas.width, data.atlas.height) {
                return Err(invalid());
            }
            if page == 0 {
                font.cap_height = data.metrics.cap_height;
            }
            for glyph in data.glyphs {
                let ch = char::from_u32(glyph.unicode).ok_or_else(invalid)?;
                if !glyph.advance.is_finite()
                    || glyph.advance < 0.0
                    || (page != 0 && glyph.unicode >> 8 != u32::from(page - 1))
                {
                    return Err(invalid());
                }
                let plane = glyph.plane_bounds.map_or([0.0; 4], |r| r.values());
                let uv = glyph.atlas_bounds.map_or([0.0; 4], |r| {
                    [
                        r.left / f64::from(data.atlas.width),
                        1.0 - r.top / f64::from(data.atlas.height),
                        r.right / f64::from(data.atlas.width),
                        1.0 - r.bottom / f64::from(data.atlas.height),
                    ]
                });
                if !plane.into_iter().chain(uv).all(f64::is_finite)
                    || uv.into_iter().any(|v| !(0.0..=1.0).contains(&v))
                {
                    return Err(invalid());
                }
                // Some Latin glyphs exist in both the core and block 00; the block is authoritative.
                font.glyphs.insert(
                    ch,
                    Glyph {
                        plane,
                        uv,
                        advance: glyph.advance,
                        page,
                    },
                );
            }
            font.pages.push(AtlasPage {
                page,
                width: data.atlas.width,
                height: data.atlas.height,
                rgba: rgba.into_raw(),
            });
        }
        if !font.glyphs.contains_key(&'?') {
            return Err(invalid());
        }
        Ok(font)
    }
    pub fn glyph(&self, ch: char) -> &Glyph {
        self.glyphs.get(&ch).unwrap_or(&self.glyphs[&'?'])
    }
    pub fn advance(&self, text: &str) -> f64 {
        text.chars().map(|ch| self.glyph(ch).advance).sum()
    }
}

/// Six initialized vectors. Position high/residual supports deep zoom, while
/// page/opacity flags follow the Web's 16-float label packet verbatim.
#[derive(Debug, Clone, PartialEq)]
#[repr(C)]
pub struct GlyphInstance {
    pub xywh: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
    pub rotation: [f32; 4],
    pub low: [f32; 4],
    pub ids: [u32; 4],
}
const _: () = assert!(std::mem::size_of::<GlyphInstance>() == 96);
impl GlyphInstance {
    pub fn packet(values: [f64; 16], ids: [u32; 4]) -> Result<Self, Diagnostic> {
        if !values
            .into_iter()
            .all(|v| v.is_finite() && (v as f32).is_finite())
        {
            return Err(PrepareError::Invalid(ObjectId(ids[0])).diagnostic());
        }
        let [x, dx] = split_position(values[0]);
        let [y, dy] = split_position(values[1]);
        Ok(Self {
            xywh: [x, y, values[2] as f32, values[3] as f32],
            uv: <[f64; 4]>::try_from(&values[4..8])
                .unwrap()
                .map(|v| v as f32),
            color: <[f64; 4]>::try_from(&values[8..12])
                .unwrap()
                .map(|v| v as f32),
            rotation: <[f64; 4]>::try_from(&values[12..16])
                .unwrap()
                .map(|v| v as f32),
            low: [dx, dy, 0.0, 0.0],
            ids,
        })
    }
    pub fn page(&self) -> u16 {
        (self.rotation[3] as u32 / 2) as u16
    }
}

#[derive(Debug)]
pub struct PreparedGlyphs {
    pub font: Arc<MsdfFont>,
    pub instances: Vec<GlyphInstance>,
    pub batches: Vec<crate::tracks::TraceBatch>,
    pub objects: Vec<ObjectId>,
    pub pick_quads: Vec<pomelo_core::picking_index::TextPickQuad>,
    pub diagnostics: Vec<Diagnostic>,
}
impl PreparedGlyphs {
    pub fn bind_drawing_owners(&mut self, scene: &pomelo_core::model::BoardScene) {
        let owners: BTreeMap<_, _> = scene
            .drawings
            .iter()
            .flat_map(|d| d.text_ids.iter().map(move |id| (id.0, d.id.0)))
            .collect();
        for glyph in &mut self.instances {
            if let Some(&owner) = owners.get(&glyph.ids[0]) {
                glyph.ids[0] = owner;
                glyph.low[2] = 1.0;
            }
        }
    }

    pub fn build(
        texts: &[BoardText],
        font: Arc<MsdfFont>,
        max_bytes: usize,
        cancel: &CancellationToken,
    ) -> Result<Self, Diagnostic> {
        let mut result = Self {
            font,
            instances: Vec::new(),
            batches: Vec::new(),
            objects: Vec::new(),
            pick_quads: Vec::new(),
            diagnostics: Vec::new(),
        };
        let mut characters = 0usize;
        for text in texts {
            if cancel.is_cancelled() {
                return Err(PrepareError::Cancelled.diagnostic());
            }
            if text.width == 0.0 || text.height == 0.0 {
                continue;
            }
            if ![
                text.at.x,
                text.at.y,
                text.angle,
                text.width,
                text.height,
                text.spacing,
                text.line_spacing,
            ]
            .into_iter()
            .all(f64::is_finite)
                || text.width < 0.0
                || text.height < 0.0
            {
                return Err(PrepareError::Invalid(text.id).diagnostic());
            }
            let (sin, cos) = text.angle.sin_cos();
            let mirror = if text.mirrored { -1.0 } else { 1.0 };
            let scale_y = text.height / result.font.cap_height;
            let normalized = text
                .text
                .replace("\r\n", "\n")
                .replace('\r', "\n")
                .replace('\t', "    ");
            let mut missing = BTreeSet::new();
            for (row, line) in normalized.split('\n').enumerate() {
                let count = line.chars().count();
                let length = if count > 0 {
                    count as f64 * text.width + (count - 1) as f64 * text.spacing
                } else {
                    0.0
                };
                let left = match text.align {
                    TextAlignment::Right => -length,
                    TextAlignment::Center => -length / 2.0,
                    TextAlignment::Left => 0.0,
                };
                for (column, ch) in line.chars().enumerate() {
                    if cancel.is_cancelled() {
                        return Err(PrepareError::Cancelled.diagnostic());
                    }
                    characters += 1;
                    if characters > 2_000_000
                        || result.instances.len()
                            >= max_bytes
                                / (96
                                    + std::mem::size_of::<crate::tracks::TraceBatch>()
                                    + std::mem::size_of::<pomelo_core::picking_index::TextPickQuad>(
                                    ))
                    {
                        return Err(Diagnostic::error(
                            "RENDER_MSDF_GLYPH_LIMIT",
                            MessageKey::RenderFontLimit,
                        ));
                    }
                    if !result.font.glyphs.contains_key(&ch) {
                        missing.insert(ch);
                    }
                    let g = result.font.glyph(ch);
                    if g.plane[2] <= g.plane[0] || g.plane[3] <= g.plane[1] {
                        continue;
                    }
                    let scale_x = text.width / g.advance.max(0.001);
                    let x =
                        left + column as f64 * (text.width + text.spacing) + g.plane[0] * scale_x;
                    let y = g.plane[1] * scale_y - row as f64 * text.line_spacing;
                    let v = [
                        text.at.x + mirror * x * cos - y * sin,
                        text.at.y + mirror * x * sin + y * cos,
                        (g.plane[2] - g.plane[0]) * scale_x * mirror,
                        (g.plane[3] - g.plane[1]) * scale_y,
                        g.uv[0],
                        g.uv[1],
                        g.uv[2],
                        g.uv[3],
                        1.0,
                        1.0,
                        1.0,
                        1.0,
                        cos,
                        sin,
                        1.0,
                        f64::from(g.page) * 2.0,
                    ];
                    result
                        .instances
                        .try_reserve(1)
                        .map_err(|_| PrepareError::Allocation.diagnostic())?;
                    let corner = |dx: f64, dy: f64| {
                        pomelo_core::model::Point::new(
                            v[0] + dx * cos - dy * sin,
                            v[1] + dx * sin + dy * cos,
                        )
                    };
                    result
                        .pick_quads
                        .try_reserve(1)
                        .map_err(|_| PrepareError::Allocation.diagnostic())?;
                    result
                        .pick_quads
                        .push(pomelo_core::picking_index::TextPickQuad {
                            text: text.id,
                            corners: [
                                corner(0.0, 0.0),
                                corner(v[2], 0.0),
                                corner(v[2], v[3]),
                                corner(0.0, v[3]),
                            ],
                        });
                    result.instances.push(GlyphInstance::packet(
                        v,
                        [
                            text.id.0,
                            pomelo_core::display::DisplayCategory::Text as u32,
                            text.layer.0,
                            0,
                        ],
                    )?);
                }
            }
            result.objects.push(text.id);
            for ch in missing.into_iter().take(16) {
                let mut diagnostic = Diagnostic::error(
                    "RENDER_MSDF_MISSING_GLYPH",
                    MessageKey::RenderTextMissingGlyph,
                );
                diagnostic.severity = pomelo_core::model::Severity::Warning;
                diagnostic.message = diagnostic.message.arg("character", ch.to_string());
                diagnostic.object = Some(text.id);
                result.diagnostics.push(diagnostic);
            }
        }
        // Source order within a layer is retained; pages are switched during draw.
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
