//! Stroke geometry decoding, independent of font resources and GPU backends.

use pomelo_core::model::{BoardText, TextAlignment};
use pomelo_core::task::CancellationToken;
use pomelo_core::{
    i18n::MessageKey,
    model::{Diagnostic, ObjectId},
};

mod bundled_resources {
    include!("../../../../assets/fonts/stroke/pages.rs");
}

/// Provider owns validated, normalized glyphs; this layer never substitutes text.
pub trait StrokeGlyphs {
    fn glyph(&self, character: char) -> Option<&[GlyphStroke]>;
}

#[derive(Debug, Clone, Copy)]
pub struct FontLimits {
    pub glyphs: usize,
    pub encoded_bytes: usize,
    pub points_per_glyph: usize,
    pub strokes: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FontLoadError {
    ResourceInvalid,
    Cancelled,
    Limit,
    Duplicate(char),
    Glyph {
        character: char,
        error: GlyphDecodeError,
    },
    AllocationFailed,
}

impl FontLoadError {
    pub fn diagnostic(&self) -> Diagnostic {
        let (code, key, character) = match self {
            Self::ResourceInvalid => (
                "RENDER_FONT_RESOURCE_INVALID",
                MessageKey::RenderFontResourceInvalid,
                None,
            ),
            Self::Cancelled => ("RENDER_FONT_CANCELLED", MessageKey::Cancelled, None),
            Self::Limit
            | Self::Glyph {
                error: GlyphDecodeError::PointLimit { .. },
                ..
            } => ("RENDER_FONT_LIMIT", MessageKey::RenderFontLimit, None),
            Self::AllocationFailed
            | Self::Glyph {
                error: GlyphDecodeError::AllocationFailed,
                ..
            } => (
                "RENDER_FONT_ALLOCATION",
                MessageKey::RenderPrepareAllocation,
                None,
            ),
            Self::Duplicate(character) => (
                "RENDER_FONT_DUPLICATE",
                MessageKey::RenderFontDuplicate,
                Some(*character),
            ),
            Self::Glyph {
                character,
                error: GlyphDecodeError::InvalidEncoding,
            } => (
                "RENDER_FONT_INVALID",
                MessageKey::RenderFontInvalid,
                Some(*character),
            ),
        };
        let mut diagnostic = Diagnostic::error(code, key);
        if let Some(character) = character {
            diagnostic.message = diagnostic.message.arg("character", character.to_string());
        }
        diagnostic
    }
}

/// Owns decoded glyphs only. Resource provenance and serialization are handled
/// by the asset loader; constructing a font never embeds a default resource.
#[derive(Debug)]
pub struct StrokeFont {
    glyphs: std::collections::HashMap<char, Vec<GlyphStroke>>,
    encoded_bytes: usize,
}

impl StrokeFont {
    /// Prepare only the bundled Unicode pages required by source board text.
    pub fn bundled_for_texts(
        texts: &[BoardText],
        max_scan_characters: usize,
        max_json_bytes: usize,
        limits: FontLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, Diagnostic> {
        Self::bundled_for_texts_policy(
            texts,
            max_scan_characters,
            max_json_bytes,
            limits,
            cancellation,
            false,
        )
    }

    /// Load available pages; absent glyphs are reported by recovering layout.
    pub fn bundled_for_recovering_texts(
        texts: &[BoardText],
        max_scan_characters: usize,
        max_json_bytes: usize,
        limits: FontLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, Diagnostic> {
        Self::bundled_for_texts_policy(
            texts,
            max_scan_characters,
            max_json_bytes,
            limits,
            cancellation,
            true,
        )
    }

    fn bundled_for_texts_policy(
        texts: &[BoardText],
        max_scan_characters: usize,
        max_json_bytes: usize,
        limits: FontLimits,
        cancellation: &CancellationToken,
        allow_missing: bool,
    ) -> Result<Self, Diagnostic> {
        if cancellation.is_cancelled() {
            return Err(FontLoadError::Cancelled.diagnostic());
        }
        if include_bytes!("../../../../assets/fonts/stroke/core.json").len() > max_json_bytes {
            return Err(FontLoadError::Limit.diagnostic());
        }
        let core = Self::bundled_core(limits, cancellation).map_err(|error| error.diagnostic())?;
        let available: Vec<_> = bundled_resources::PAGES
            .iter()
            .map(|(block, _)| *block)
            .collect();
        let needed = core.required_pages_policy(
            texts,
            &available,
            max_scan_characters,
            cancellation,
            allow_missing,
        )?;
        if needed.is_empty() {
            return Ok(core);
        }
        drop(core);
        let pages =
            std::iter::once(include_bytes!("../../../../assets/fonts/stroke/core.json").as_slice())
                .chain(
                    bundled_resources::PAGES
                        .iter()
                        .filter(|(block, _)| needed.binary_search(block).is_ok())
                        .map(|(_, data)| *data),
                );
        Self::load_json_pages(pages, max_json_bytes, limits, cancellation)
            .map_err(|error| error.diagnostic())
    }
    /// Bundled core resource keeps its own notices under assets/fonts/stroke.
    pub fn bundled_core(
        limits: FontLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, FontLoadError> {
        Self::load_json(
            include_bytes!("../../../../assets/fonts/stroke/core.json"),
            65_536,
            limits,
            cancellation,
        )
    }
    /// Discover unloaded Unicode pages from stored board text. Available pages
    /// are supplied by a frozen asset manifest, never inferred from file paths.
    pub fn required_pages(
        &self,
        texts: &[BoardText],
        available: &[u32],
        max_characters: usize,
        cancellation: &CancellationToken,
    ) -> Result<Vec<u32>, Diagnostic> {
        self.required_pages_policy(texts, available, max_characters, cancellation, false)
    }

    fn required_pages_policy(
        &self,
        texts: &[BoardText],
        available: &[u32],
        max_characters: usize,
        cancellation: &CancellationToken,
        allow_missing: bool,
    ) -> Result<Vec<u32>, Diagnostic> {
        let mut pages = Vec::new();
        let mut scanned = 0_usize;
        for text in texts {
            if cancellation.is_cancelled() {
                return Err(TextBuildError::Cancelled.diagnostic(text.id));
            }
            validate_text_geometry(text)
                .map_err(|error| TextBuildError::Geometry(error).diagnostic(text.id))?;
            if text_is_zero_size(text) {
                continue;
            }
            for character in text.text.chars() {
                if cancellation.is_cancelled() {
                    return Err(TextBuildError::Cancelled.diagnostic(text.id));
                }
                if scanned >= max_characters {
                    return Err(TextBuildError::CharacterLimit.diagnostic(text.id));
                }
                scanned += 1;
                let character = match character {
                    '\r' | '\n' => continue,
                    '\t' => ' ',
                    other => other,
                };
                if self.glyphs.contains_key(&character) {
                    continue;
                }
                let page = u32::from(character) >> 8;
                if !available.contains(&page) {
                    if allow_missing {
                        continue;
                    }
                    return Err(TextBuildError::MissingGlyph(character).diagnostic(text.id));
                }
                if let Err(index) = pages.binary_search(&page) {
                    pages
                        .try_reserve(1)
                        .map_err(|_| TextBuildError::AllocationFailed.diagnostic(text.id))?;
                    pages.insert(index, page);
                }
            }
        }
        if cancellation.is_cancelled() {
            let mut diagnostic = Diagnostic::error("RENDER_TEXT_CANCELLED", MessageKey::Cancelled);
            diagnostic.object = texts.last().map(|text| text.id);
            return Err(diagnostic);
        }
        Ok(pages)
    }
    /// Merge resource pages with one font-wide budget. No partial font escapes
    /// on failure, and duplicate characters across pages are rejected.
    pub fn load_json_pages<'a>(
        pages: impl IntoIterator<Item = &'a [u8]>,
        max_json_bytes: usize,
        limits: FontLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, FontLoadError> {
        let mut combined = Self {
            glyphs: std::collections::HashMap::new(),
            encoded_bytes: 0,
        };
        let mut json_bytes = 0_usize;
        let mut strokes = 0_usize;
        for data in pages {
            if cancellation.is_cancelled() {
                return Err(FontLoadError::Cancelled);
            }
            json_bytes = json_bytes
                .checked_add(data.len())
                .ok_or(FontLoadError::Limit)?;
            if json_bytes > max_json_bytes {
                return Err(FontLoadError::Limit);
            }
            let page = Self::load_json(
                data,
                data.len(),
                FontLimits {
                    glyphs: limits.glyphs.saturating_sub(combined.glyphs.len()),
                    encoded_bytes: limits.encoded_bytes.saturating_sub(combined.encoded_bytes),
                    points_per_glyph: limits.points_per_glyph,
                    strokes: limits.strokes.saturating_sub(strokes),
                },
                cancellation,
            )?;
            combined
                .glyphs
                .try_reserve(page.glyphs.len())
                .map_err(|_| FontLoadError::AllocationFailed)?;
            combined.encoded_bytes = combined
                .encoded_bytes
                .checked_add(page.encoded_bytes)
                .ok_or(FontLoadError::Limit)?;
            for (character, glyph) in page.glyphs {
                if cancellation.is_cancelled() {
                    return Err(FontLoadError::Cancelled);
                }
                if combined.glyphs.contains_key(&character) {
                    return Err(FontLoadError::Duplicate(character));
                }
                strokes = strokes
                    .checked_add(glyph.len())
                    .ok_or(FontLoadError::Limit)?;
                combined.glyphs.insert(character, glyph);
            }
        }
        if cancellation.is_cancelled() {
            return Err(FontLoadError::Cancelled);
        }
        Ok(combined)
    }
    /// Decode a bounded JSON glyph map. Use a sequence internally so duplicate
    /// JSON keys survive parsing and are rejected by the font loader.
    pub fn load_json(
        data: &[u8],
        max_json_bytes: usize,
        limits: FontLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, FontLoadError> {
        if cancellation.is_cancelled() {
            return Err(FontLoadError::Cancelled);
        }
        if data.len() > max_json_bytes {
            return Err(FontLoadError::Limit);
        }
        struct GlyphMapVisitor<'a> {
            cancellation: &'a CancellationToken,
            limits: FontLimits,
            failure: &'a mut Option<FontLoadError>,
        }
        impl<'de> serde::de::Visitor<'de> for GlyphMapVisitor<'_> {
            type Value = Vec<(char, String)>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("FONT_GLYPH_MAP")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                let mut bytes = 0_usize;
                loop {
                    if self.cancellation.is_cancelled() {
                        *self.failure = Some(FontLoadError::Cancelled);
                        return Err(serde::de::Error::custom("FONT_CANCELLED"));
                    }
                    let Some(character) = map.next_key::<char>()? else {
                        break;
                    };
                    if entries.len() >= self.limits.glyphs {
                        *self.failure = Some(FontLoadError::Limit);
                        return Err(serde::de::Error::custom("FONT_LIMIT"));
                    }
                    let encoded = map.next_value::<String>()?;
                    if self.cancellation.is_cancelled() {
                        *self.failure = Some(FontLoadError::Cancelled);
                        return Err(serde::de::Error::custom("FONT_CANCELLED"));
                    }
                    let Some(total) = bytes
                        .checked_add(encoded.len())
                        .filter(|total| *total <= self.limits.encoded_bytes)
                    else {
                        *self.failure = Some(FontLoadError::Limit);
                        return Err(serde::de::Error::custom("FONT_LIMIT"));
                    };
                    bytes = total;
                    if entries.try_reserve(1).is_err() {
                        *self.failure = Some(FontLoadError::AllocationFailed);
                        return Err(serde::de::Error::custom("FONT_ALLOCATION"));
                    }
                    entries.push((character, encoded));
                }
                Ok(entries)
            }
        }
        let mut deserializer = serde_json::Deserializer::from_slice(data);
        let mut failure = None;
        let entries = serde::de::Deserializer::deserialize_map(
            &mut deserializer,
            GlyphMapVisitor {
                cancellation,
                limits,
                failure: &mut failure,
            },
        )
        .map_err(|_| failure.unwrap_or(FontLoadError::ResourceInvalid))?;
        deserializer
            .end()
            .map_err(|_| FontLoadError::ResourceInvalid)?;
        Self::load(
            entries
                .iter()
                .map(|(character, encoded)| (*character, encoded.as_str())),
            limits,
            cancellation,
        )
    }
    pub fn load<'a>(
        encoded: impl IntoIterator<Item = (char, &'a str)>,
        limits: FontLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, FontLoadError> {
        let mut glyphs = std::collections::HashMap::new();
        let mut bytes = 0_usize;
        let mut strokes = 0_usize;
        for (character, data) in encoded {
            if cancellation.is_cancelled() {
                return Err(FontLoadError::Cancelled);
            }
            if glyphs.contains_key(&character) {
                return Err(FontLoadError::Duplicate(character));
            }
            bytes = bytes.checked_add(data.len()).ok_or(FontLoadError::Limit)?;
            if glyphs.len() >= limits.glyphs || bytes > limits.encoded_bytes {
                return Err(FontLoadError::Limit);
            }
            let glyph = decode_glyph(data, limits.points_per_glyph)
                .map_err(|error| FontLoadError::Glyph { character, error })?;
            strokes = strokes
                .checked_add(glyph.len())
                .ok_or(FontLoadError::Limit)?;
            if strokes > limits.strokes {
                return Err(FontLoadError::Limit);
            }
            glyphs
                .try_reserve(1)
                .map_err(|_| FontLoadError::AllocationFailed)?;
            glyphs.insert(character, glyph);
        }
        if cancellation.is_cancelled() {
            return Err(FontLoadError::Cancelled);
        }
        Ok(Self {
            glyphs,
            encoded_bytes: bytes,
        })
    }
}

impl StrokeGlyphs for StrokeFont {
    fn glyph(&self, character: char) -> Option<&[GlyphStroke]> {
        self.glyphs.get(&character).map(Vec::as_slice)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum TextBuildError {
    Cancelled,
    CharacterLimit,
    StrokeLimit,
    AllocationFailed,
    MissingGlyph(char),
    Geometry(TextTransformError),
}

impl TextBuildError {
    pub fn diagnostic(&self, object: ObjectId) -> Diagnostic {
        let (code, key) = match self {
            Self::Cancelled => ("RENDER_TEXT_CANCELLED", MessageKey::Cancelled),
            Self::CharacterLimit => (
                "RENDER_TEXT_CHARACTER_LIMIT",
                MessageKey::RenderTextCharacterLimit,
            ),
            Self::StrokeLimit => (
                "RENDER_TEXT_STROKE_LIMIT",
                MessageKey::RenderTextStrokeLimit,
            ),
            Self::AllocationFailed => (
                "RENDER_TEXT_ALLOCATION",
                MessageKey::RenderPrepareAllocation,
            ),
            Self::MissingGlyph(_) => (
                "RENDER_TEXT_MISSING_GLYPH",
                MessageKey::RenderTextMissingGlyph,
            ),
            Self::Geometry(_) => (
                "RENDER_TEXT_INVALID_GEOMETRY",
                MessageKey::RenderTextInvalidGeometry,
            ),
        };
        let mut diagnostic = Diagnostic::error(code, key);
        diagnostic.object = Some(object);
        if let Self::MissingGlyph(character) = self {
            diagnostic.message = diagnostic.message.arg("character", character.to_string());
        }
        diagnostic
    }
}

#[derive(Debug, PartialEq)]
pub struct TextStroke {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub width: f64,
}

/// Build one source text with explicit character and output budgets. A failure
/// discards partial geometry; callers attach source identity and localized diagnostics.
pub fn build_text(
    text: &BoardText,
    font: &impl StrokeGlyphs,
    max_characters: usize,
    max_strokes: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<TextStroke>, TextBuildError> {
    build_text_counted(text, font, max_characters, max_strokes, cancellation)
        .map(|(strokes, _)| strokes)
}

fn build_text_counted(
    text: &BoardText,
    font: &impl StrokeGlyphs,
    max_characters: usize,
    max_strokes: usize,
    cancellation: &CancellationToken,
) -> Result<(Vec<TextStroke>, usize), TextBuildError> {
    if cancellation.is_cancelled() {
        return Err(TextBuildError::Cancelled);
    }
    validate_text_geometry(text).map_err(TextBuildError::Geometry)?;
    if text_is_zero_size(text) {
        return Ok((Vec::new(), 0));
    }
    let characters = normalized_characters(text, max_characters, cancellation)?;
    let strokes = build_normalized_text(text, font, &characters, max_strokes, cancellation)?;
    Ok((strokes, characters.len()))
}

/// Private: callers validate geometry and bound normalized input before layout.
fn build_normalized_text(
    text: &BoardText,
    font: &impl StrokeGlyphs,
    characters: &[char],
    max_strokes: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<TextStroke>, TextBuildError> {
    let mut output = Vec::new();
    for (row, line) in characters.split(|character| *character == '\n').enumerate() {
        for (column, character) in line.iter().copied().enumerate() {
            if cancellation.is_cancelled() {
                return Err(TextBuildError::Cancelled);
            }
            let transform = GlyphTransform::new(text, row, column, line.len())
                .map_err(TextBuildError::Geometry)?;
            let glyph = font.glyph(character);
            if cancellation.is_cancelled() {
                return Err(TextBuildError::Cancelled);
            }
            let glyph = glyph.ok_or(TextBuildError::MissingGlyph(character))?;
            if glyph.len() > max_strokes.saturating_sub(output.len()) {
                return Err(TextBuildError::StrokeLimit);
            }
            output
                .try_reserve(glyph.len())
                .map_err(|_| TextBuildError::AllocationFailed)?;
            for stroke in glyph {
                if cancellation.is_cancelled() {
                    return Err(TextBuildError::Cancelled);
                }
                output.push(TextStroke {
                    a: transform
                        .point(stroke.a)
                        .map_err(TextBuildError::Geometry)?,
                    b: transform
                        .point(stroke.b)
                        .map_err(TextBuildError::Geometry)?,
                    width: text.stroke_width,
                });
            }
        }
    }
    if cancellation.is_cancelled() {
        return Err(TextBuildError::Cancelled);
    }
    Ok(output)
}

#[derive(Debug)]
pub struct TextBatch {
    pub object: ObjectId,
    pub layer: pomelo_core::model::LayerId,
    pub strokes: std::ops::Range<usize>,
}

#[derive(Debug)]
pub struct PreparedTexts {
    pub strokes: Vec<TextStroke>,
    pub batches: Vec<TextBatch>,
    pub characters: usize,
}

/// Aggregate result of object-at-a-time text preparation.
#[derive(Debug)]
pub struct TextPreparationSummary {
    pub characters: usize,
    pub strokes: usize,
    pub objects: usize,
    pub diagnostics: Vec<Diagnostic>,
}

/// Counts font strokes without allocating board-wide transformed geometry.
/// This is a sizing probe, not geometry or GPU validation.
pub fn count_text_strokes(
    texts: &[BoardText],
    font: &impl StrokeGlyphs,
    max_characters: usize,
    cancellation: &CancellationToken,
) -> Result<(usize, usize), Diagnostic> {
    let mut characters = 0usize;
    let mut strokes = 0usize;
    let mut missing_objects = 0usize;
    for text in texts {
        let normalized = normalized_characters(
            text,
            max_characters.saturating_sub(characters),
            cancellation,
        )
        .map_err(|error| error.diagnostic(text.id))?;
        characters += normalized.len();
        let mut object_strokes = 0usize;
        let mut missing = false;
        for character in normalized
            .into_iter()
            .filter(|character| *character != '\n')
        {
            if cancellation.is_cancelled() {
                return Err(TextBuildError::Cancelled.diagnostic(text.id));
            }
            match font.glyph(character) {
                Some(glyph) => {
                    object_strokes = object_strokes
                        .checked_add(glyph.len())
                        .ok_or_else(|| TextBuildError::StrokeLimit.diagnostic(text.id))?;
                }
                None => missing = true,
            }
        }
        if missing {
            missing_objects += 1;
        } else {
            strokes = strokes
                .checked_add(object_strokes)
                .ok_or_else(|| TextBuildError::StrokeLimit.diagnostic(text.id))?;
        }
    }
    if cancellation.is_cancelled() {
        return Err(Diagnostic::error(
            "RENDER_TEXT_CANCELLED",
            MessageKey::Cancelled,
        ));
    }
    Ok((strokes, missing_objects))
}

/// Aggregate budgets include blank and newline characters and empty text objects.
/// Source order and typed source identities are preserved without renumbering.
impl PreparedTexts {
    /// Keep supported objects when a source object contains an unavailable glyph.
    /// All other errors, including aggregate budgets and cancellation, remain fatal.
    pub fn build_recovering_missing_glyphs(
        texts: &[BoardText],
        font: &impl StrokeGlyphs,
        max_objects: usize,
        max_characters: usize,
        max_strokes: usize,
        cancellation: &CancellationToken,
    ) -> Result<(Self, Vec<Diagnostic>), Diagnostic> {
        let mut prepared = Self {
            strokes: Vec::new(),
            batches: Vec::new(),
            characters: 0,
        };
        let summary = Self::visit_recovering_missing_glyphs(
            texts,
            font,
            max_objects,
            max_characters,
            max_strokes,
            cancellation,
            |text, strokes| {
                prepared
                    .strokes
                    .try_reserve(strokes.len())
                    .map_err(|_| TextBuildError::AllocationFailed.diagnostic(text.id))?;
                prepared
                    .batches
                    .try_reserve(1)
                    .map_err(|_| TextBuildError::AllocationFailed.diagnostic(text.id))?;
                let start = prepared.strokes.len();
                prepared
                    .strokes
                    .extend(strokes.iter().map(|stroke| TextStroke {
                        a: stroke.a,
                        b: stroke.b,
                        width: stroke.width,
                    }));
                prepared.batches.push(TextBatch {
                    object: text.id,
                    layer: text.layer,
                    strokes: start..prepared.strokes.len(),
                });
                Ok(())
            },
        )?;
        prepared.characters = summary.characters;
        Ok((prepared, summary.diagnostics))
    }

    /// Visit supported objects in source order while retaining only one object's
    /// transformed strokes. A visitor error is fatal; callers must discard any
    /// partially accumulated result. Missing glyphs skip the whole object.
    pub fn visit_recovering_missing_glyphs(
        texts: &[BoardText],
        font: &impl StrokeGlyphs,
        max_objects: usize,
        max_characters: usize,
        max_strokes: usize,
        cancellation: &CancellationToken,
        mut visit: impl FnMut(&BoardText, &[TextStroke]) -> Result<(), Diagnostic>,
    ) -> Result<TextPreparationSummary, Diagnostic> {
        if cancellation.is_cancelled() {
            let mut diagnostic = Diagnostic::error("RENDER_TEXT_CANCELLED", MessageKey::Cancelled);
            diagnostic.object = texts.first().map(|text| text.id);
            return Err(diagnostic);
        }
        let mut characters_count = 0;
        let mut strokes_count = 0;
        let mut objects_count = 0;
        let mut diagnostics = Vec::new();
        for (index, text) in texts.iter().enumerate() {
            if cancellation.is_cancelled() {
                return Err(TextBuildError::Cancelled.diagnostic(text.id));
            }
            if index >= max_objects {
                let mut error =
                    Diagnostic::error("RENDER_TEXT_OBJECT_LIMIT", MessageKey::GeometryLimit);
                error.message = error
                    .message
                    .arg("actual", index.saturating_add(1))
                    .arg("limit", max_objects);
                error.object = Some(text.id);
                return Err(error);
            }
            let characters = normalized_characters(
                text,
                max_characters.saturating_sub(characters_count),
                cancellation,
            )
            .map_err(|error| error.diagnostic(text.id))?;
            characters_count += characters.len();
            match build_normalized_text(
                text,
                font,
                &characters,
                max_strokes.saturating_sub(strokes_count),
                cancellation,
            ) {
                Ok(strokes) => {
                    visit(text, &strokes)?;
                    strokes_count += strokes.len();
                    objects_count += 1;
                }
                Err(error @ TextBuildError::MissingGlyph(_)) => {
                    diagnostics
                        .try_reserve(1)
                        .map_err(|_| TextBuildError::AllocationFailed.diagnostic(text.id))?;
                    let mut diagnostic = error.diagnostic(text.id);
                    diagnostic.severity = pomelo_core::model::Severity::Warning;
                    diagnostics.push(diagnostic);
                }
                Err(TextBuildError::StrokeLimit) => {
                    let mut error = TextBuildError::StrokeLimit.diagnostic(text.id);
                    error.message =
                        pomelo_core::i18n::Message::new(MessageKey::RenderTextStrokeBudget)
                            .arg("prepared", strokes_count)
                            .arg("limit", max_strokes)
                            .arg("objects", objects_count);
                    return Err(error);
                }
                Err(error) => return Err(error.diagnostic(text.id)),
            }
        }
        if cancellation.is_cancelled() {
            let mut diagnostic = Diagnostic::error("RENDER_TEXT_CANCELLED", MessageKey::Cancelled);
            diagnostic.object = texts.last().map(|text| text.id);
            return Err(diagnostic);
        }
        Ok(TextPreparationSummary {
            characters: characters_count,
            strokes: strokes_count,
            objects: objects_count,
            diagnostics,
        })
    }

    pub fn build(
        texts: &[BoardText],
        font: &impl StrokeGlyphs,
        max_objects: usize,
        max_characters: usize,
        max_strokes: usize,
        cancellation: &CancellationToken,
    ) -> Result<Self, Diagnostic> {
        let mut prepared = Self {
            strokes: Vec::new(),
            batches: Vec::new(),
            characters: 0,
        };
        for text in texts {
            if cancellation.is_cancelled() {
                return Err(TextBuildError::Cancelled.diagnostic(text.id));
            }
            if prepared.batches.len() >= max_objects {
                let mut diagnostic =
                    Diagnostic::error("RENDER_TEXT_OBJECT_LIMIT", MessageKey::GeometryLimit);
                diagnostic.message = diagnostic
                    .message
                    .arg("actual", prepared.batches.len().saturating_add(1))
                    .arg("limit", max_objects);
                diagnostic.object = Some(text.id);
                return Err(diagnostic);
            }
            let (mut strokes, characters) = build_text_counted(
                text,
                font,
                max_characters.saturating_sub(prepared.characters),
                max_strokes.saturating_sub(prepared.strokes.len()),
                cancellation,
            )
            .map_err(|error| error.diagnostic(text.id))?;
            prepared
                .strokes
                .try_reserve(strokes.len())
                .map_err(|_| TextBuildError::AllocationFailed.diagnostic(text.id))?;
            prepared
                .batches
                .try_reserve(1)
                .map_err(|_| TextBuildError::AllocationFailed.diagnostic(text.id))?;
            let start = prepared.strokes.len();
            prepared.strokes.append(&mut strokes);
            prepared.characters += characters;
            prepared.batches.push(TextBatch {
                object: text.id,
                layer: text.layer,
                strokes: start..prepared.strokes.len(),
            });
        }
        if cancellation.is_cancelled() {
            let mut diagnostic = Diagnostic::error("RENDER_TEXT_CANCELLED", MessageKey::Cancelled);
            diagnostic.object = texts.last().map(|text| text.id);
            return Err(diagnostic);
        }
        Ok(prepared)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum TextTransformError {
    InvalidGeometry,
    InvalidColumn,
}

fn validate_text_geometry(text: &BoardText) -> Result<(), TextTransformError> {
    if ![
        text.at.x,
        text.at.y,
        text.angle,
        text.width,
        text.height,
        text.spacing,
        text.line_spacing,
        text.stroke_width,
    ]
    .into_iter()
    .all(f64::is_finite)
        || text.width < 0.0
        || text.height < 0.0
        || text.stroke_width < 0.0
    {
        Err(TextTransformError::InvalidGeometry)
    } else {
        Ok(())
    }
}

fn text_is_zero_size(text: &BoardText) -> bool {
    [
        text.width,
        text.height,
        text.spacing,
        text.line_spacing,
        text.stroke_width,
    ]
    .into_iter()
    .all(|value| value == 0.0)
}

/// Per-glyph transform into millimetre board coordinates. Row and column are
/// Unicode character positions after newline normalization and tab expansion.
#[derive(Debug)]
pub struct GlyphTransform {
    origin: [f64; 2],
    offset: [f64; 2],
    size: [f64; 2],
    rotation: [f64; 2],
    mirror: f64,
}

impl GlyphTransform {
    pub fn new(
        text: &BoardText,
        row: usize,
        column: usize,
        line_characters: usize,
    ) -> Result<Self, TextTransformError> {
        if column >= line_characters {
            return Err(TextTransformError::InvalidColumn);
        }
        validate_text_geometry(text)?;
        let length = line_characters as f64 * text.width
            + line_characters.saturating_sub(1) as f64 * text.spacing;
        let left = match text.align {
            TextAlignment::Left => 0.0,
            TextAlignment::Center => -length / 2.0,
            TextAlignment::Right => -length,
        };
        let offset = [
            left + column as f64 * (text.width + text.spacing),
            -(row as f64) * text.line_spacing,
        ];
        if !length.is_finite() || !offset.into_iter().all(f64::is_finite) {
            return Err(TextTransformError::InvalidGeometry);
        }
        let (sin, cos) = text.angle.sin_cos();
        Ok(Self {
            origin: [text.at.x, text.at.y],
            offset,
            size: [text.width, text.height],
            rotation: [cos, sin],
            mirror: if text.mirrored { -1.0 } else { 1.0 },
        })
    }

    /// Mirror in local text coordinates before rotating around the text origin.
    pub fn point(&self, point: [f64; 2]) -> Result<[f64; 2], TextTransformError> {
        let x = self.mirror * (self.offset[0] + point[0] * self.size[0]);
        let y = self.offset[1] + point[1] * self.size[1];
        let [cos, sin] = self.rotation;
        let result = [
            self.origin[0] + x * cos - y * sin,
            self.origin[1] + x * sin + y * cos,
        ];
        if result.into_iter().all(f64::is_finite) {
            Ok(result)
        } else {
            Err(TextTransformError::InvalidGeometry)
        }
    }
}

/// Normalized glyph stroke. Board text layout applies sizing and transforms later.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphStroke {
    pub a: [f64; 2],
    pub b: [f64; 2],
}

#[derive(Debug, PartialEq, Eq)]
pub enum GlyphDecodeError {
    InvalidEncoding,
    PointLimit { actual: usize, limit: usize },
    AllocationFailed,
}

/// Decode pair-encoded stroke data. The first pair contains side bearings;
/// ` R` separates paths. Normalization follows the board stroke font convention.
/// No font data or missing-character replacement is embedded here.
pub fn decode_glyph(
    encoded: &str,
    max_points: usize,
) -> Result<Vec<GlyphStroke>, GlyphDecodeError> {
    let bytes = encoded.as_bytes();
    if bytes.len() < 2
        || !bytes.len().is_multiple_of(2)
        || !bytes.iter().all(|b| (32..=126).contains(b))
    {
        return Err(GlyphDecodeError::InvalidEncoding);
    }
    let pairs = bytes[2..].as_chunks::<2>().0.iter();
    let mut points = 0_usize;
    let mut minimum = u8::MAX;
    let mut maximum = u8::MIN;
    for pair in pairs.clone().filter(|pair| *pair != b" R") {
        points += 1;
        if points > max_points {
            return Err(GlyphDecodeError::PointLimit {
                actual: points,
                limit: max_points,
            });
        }
        minimum = minimum.min(pair[0]);
        maximum = maximum.max(pair[0]);
    }
    if points == 0 {
        return Ok(Vec::new());
    }
    let center = (f64::from(minimum) + f64::from(maximum)) / 2.0;
    let span = f64::from(maximum - minimum).max(14.0);
    let normalize = |pair: &[u8]| {
        [
            0.5 + (f64::from(pair[0]) - center) / span,
            (91.0 - f64::from(pair[1])) / 21.0,
        ]
    };
    let mut strokes = Vec::new();
    strokes
        .try_reserve_exact(points.saturating_sub(1))
        .map_err(|_| GlyphDecodeError::AllocationFailed)?;
    let mut previous = None;
    for pair in pairs {
        if pair == b" R" {
            previous = None;
        } else {
            let point = normalize(pair);
            if let Some(a) = previous {
                strokes.push(GlyphStroke { a, b: point });
            }
            previous = Some(point);
        }
    }
    Ok(strokes)
}

fn normalized_characters(
    text: &BoardText,
    max_characters: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<char>, TextBuildError> {
    if cancellation.is_cancelled() {
        return Err(TextBuildError::Cancelled);
    }
    validate_text_geometry(text).map_err(TextBuildError::Geometry)?;
    if text_is_zero_size(text) {
        return Ok(Vec::new());
    }
    let mut characters = Vec::new();
    let mut input = text.text.chars().peekable();
    while let Some(character) = input.next() {
        if cancellation.is_cancelled() {
            return Err(TextBuildError::Cancelled);
        }
        let (character, repeats) = match character {
            '\r' => {
                if input.peek() == Some(&'\n') {
                    input.next();
                }
                ('\n', 1)
            }
            '\t' => (' ', 4),
            other => (other, 1),
        };
        if repeats > max_characters.saturating_sub(characters.len()) {
            return Err(TextBuildError::CharacterLimit);
        }
        characters
            .try_reserve(repeats)
            .map_err(|_| TextBuildError::AllocationFailed)?;
        characters.extend(std::iter::repeat_n(character, repeats));
    }

    Ok(characters)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct SyntheticFont;

    #[test]
    fn compact_text_batches_preserve_per_layer_source_order_and_full_ranges() {
        let token = CancellationToken::default();
        let mut sources = vec![text(), text(), text()];
        for (index, source) in sources.iter_mut().enumerate() {
            source.id = ObjectId(index as u32 + 10);
            source.layer = pomelo_core::model::LayerId(if index == 1 { 1 } else { 2 });
        }
        let (legacy, _, _) = crate::tracks::PreparedTracks::build_source_texts(
            &sources,
            &SyntheticFont,
            3,
            100,
            Default::default(),
            &token,
        )
        .unwrap();
        let compact = crate::text_instances::PreparedTextInstances::build(
            &sources,
            &SyntheticFont,
            3,
            100,
            4096,
            &token,
        )
        .unwrap();
        assert_eq!(compact.batches.len(), 2);
        let mut cursor = 0;
        for batch in &compact.batches {
            assert_eq!(batch.start as usize, cursor);
            cursor += batch.count as usize;
            assert!(
                compact.instances[batch.start as usize..cursor]
                    .iter()
                    .all(|instance| instance.ids[1] == batch.layer.0)
            );
        }
        assert_eq!(cursor, compact.instances.len());
        for (packed, full) in compact.instances.iter().zip(&legacy.instances) {
            assert_eq!(packed.a, full.a);
            assert_eq!(packed.b, full.b);
            assert_eq!(
                packed.ids,
                [full.ids[0], full.ids[2], full.flags[1], full.flags[2]]
            );
        }
    }

    #[test]
    fn compact_text_keeps_endpoint_precision_identity_order_and_budget() {
        let token = CancellationToken::default();
        let mut source = text();
        source.at.x = 100_000.000_123;
        let texts = [source];
        let expected = PreparedTexts::build(&texts, &SyntheticFont, 1, 100, 100, &token).unwrap();
        let actual = crate::text_instances::PreparedTextInstances::build(
            &texts,
            &SyntheticFont,
            1,
            100,
            1024,
            &token,
        )
        .unwrap();
        assert!(!actual.instances.is_empty());
        assert_eq!(actual.instances.len(), expected.strokes.len());
        for (index, (packed, stroke)) in actual.instances.iter().zip(&expected.strokes).enumerate()
        {
            for (point, original) in [(packed.a, stroke.a), (packed.b, stroke.b)] {
                for axis in 0..2 {
                    assert!(
                        (f64::from(point[axis]) + f64::from(point[axis + 2]) - original[axis])
                            .abs()
                            < 1e-8
                    );
                }
            }
            assert_eq!(
                packed.ids,
                [
                    texts[0].id.0,
                    texts[0].layer.0,
                    index as u32,
                    (stroke.width as f32).to_bits()
                ]
            );
            assert_eq!(packed.flags, [0; 4]);
        }
        assert!(
            crate::text_instances::PreparedTextInstances::build(
                &texts,
                &SyntheticFont,
                1,
                100,
                0,
                &token,
            )
            .is_err()
        );
    }

    #[test]
    fn streaming_sizing_cannot_override_source_order_budget_diagnostics() {
        let token = CancellationToken::default();
        let mut first = text();
        first.text = "AA".into();
        let mut invalid = text();
        invalid.id = ObjectId(100);
        invalid.width = f64::NAN;
        let sources = [first, invalid];
        for (objects, characters, strokes) in [(0, 100, 100), (2, 0, 100), (2, 100, 0)] {
            let expected = PreparedTexts::build_recovering_missing_glyphs(
                &sources,
                &SyntheticFont,
                objects,
                characters,
                strokes,
                &token,
            )
            .unwrap_err();
            let actual = crate::tracks::PreparedTracks::build_source_texts(
                &sources,
                &SyntheticFont,
                objects,
                characters,
                crate::tracks::TraceLimits {
                    max_instances: strokes,
                    ..Default::default()
                },
                &token,
            )
            .unwrap_err();
            assert_eq!(actual.code, expected.code);
            assert_eq!(actual.object, expected.object);
            for locale in pomelo_core::i18n::Locale::ALL {
                assert_eq!(
                    actual.message.display(locale),
                    expected.message.display(locale)
                );
            }
        }
    }

    #[test]
    fn streaming_gpu_instances_match_aggregate_layout_and_keep_empty_objects() {
        let token = CancellationToken::default();
        let mut first = text();
        first.text = "AA".into();
        first.layer = pomelo_core::model::LayerId(2);
        let mut second = text();
        second.id = ObjectId(100);
        second.layer = pomelo_core::model::LayerId(1);
        second.angle = 45.0;
        let mut blank = text();
        blank.id = ObjectId(101);
        blank.text.clear();
        let mut missing = text();
        missing.id = ObjectId(102);
        missing.text = "?".into();
        let sources = [first, blank, missing, second];
        let limits = crate::tracks::TraceLimits::default();
        let (prepared, diagnostics) = PreparedTexts::build_recovering_missing_glyphs(
            &sources,
            &SyntheticFont,
            4,
            100,
            limits.max_instances,
            &token,
        )
        .unwrap();
        let expected =
            crate::tracks::PreparedTracks::build_texts(&prepared, limits, &token).unwrap();
        let (actual, ids, summary) = crate::tracks::PreparedTracks::build_source_texts(
            &sources,
            &SyntheticFont,
            4,
            100,
            limits,
            &token,
        )
        .unwrap();
        assert_eq!(actual.instances, expected.instances);
        assert_eq!(actual.batches, expected.batches);
        assert_eq!(ids, [sources[0].id, sources[1].id, sources[3].id]);
        assert_eq!(summary.characters, prepared.characters);
        assert_eq!(summary.diagnostics[0].object, diagnostics[0].object);
        assert!(
            crate::tracks::PreparedTracks::build_source_texts(
                &sources,
                &SyntheticFont,
                4,
                100,
                crate::tracks::TraceLimits {
                    max_bytes: 1,
                    ..limits
                },
                &token,
            )
            .is_err()
        );
    }

    #[test]
    fn object_visitor_preserves_geometry_and_missing_object_diagnostics() {
        let token = CancellationToken::default();
        let mut supported = text();
        supported.text = "A\tA\r\nA".into();
        let mut missing = text();
        missing.id = ObjectId(99);
        missing.text = "A?".into();
        let sources = [missing, supported];
        let (prepared, diagnostics) = PreparedTexts::build_recovering_missing_glyphs(
            &sources,
            &SyntheticFont,
            2,
            100,
            100,
            &token,
        )
        .unwrap();
        let mut visited = 0;
        let summary = PreparedTexts::visit_recovering_missing_glyphs(
            &sources,
            &SyntheticFont,
            2,
            100,
            100,
            &token,
            |source, strokes| {
                assert_eq!(source.id, prepared.batches[visited].object);
                assert_eq!(
                    strokes,
                    &prepared.strokes[prepared.batches[visited].strokes.clone()]
                );
                visited += 1;
                Ok(())
            },
        )
        .unwrap();
        assert!(summary.strokes > 0);
        assert_eq!(summary.strokes, prepared.strokes.len());
        assert_eq!(summary.objects, visited);
        assert_eq!(summary.characters, prepared.characters);
        assert_eq!(summary.diagnostics[0].code, diagnostics[0].code);
        assert_eq!(summary.diagnostics[0].object, diagnostics[0].object);
    }

    #[test]
    fn object_visitor_stops_on_sink_error_and_cancellation() {
        let token = CancellationToken::default();
        let sources = [text(), text()];
        let mut visits = 0;
        let result = PreparedTexts::visit_recovering_missing_glyphs(
            &sources,
            &SyntheticFont,
            2,
            100,
            100,
            &token,
            |source, _| {
                visits += 1;
                Err(TextBuildError::AllocationFailed.diagnostic(source.id))
            },
        );
        assert_eq!(visits, 1);
        assert_eq!(result.unwrap_err().code.as_ref(), "RENDER_TEXT_ALLOCATION");
        let result = PreparedTexts::visit_recovering_missing_glyphs(
            &sources,
            &SyntheticFont,
            2,
            100,
            100,
            &token,
            |_, _| {
                token.cancel();
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err().code.as_ref(), "RENDER_TEXT_CANCELLED");
    }
    #[test]
    fn stroke_count_matches_prepared_geometry_without_retaining_transformed_strokes() {
        let token = CancellationToken::default();
        let mut source = text();
        source.text = "A\tA\r\nA".into();
        let (prepared, diagnostics) = PreparedTexts::build_recovering_missing_glyphs(
            &[source.clone()],
            &SyntheticFont,
            10,
            100,
            100,
            &token,
        )
        .unwrap();
        let (count, missing) = count_text_strokes(&[source], &SyntheticFont, 100, &token).unwrap();
        assert_eq!(count, prepared.strokes.len());
        assert_eq!(missing, diagnostics.len());
    }
    #[test]
    fn bundled_text_font_loads_needed_cjk_pages_and_reports_hangul_gap() {
        let token = CancellationToken::default();
        let mut source = text();
        source.text = "A中文繁體日本語".into();
        let limits = FontLimits {
            glyphs: 2048,
            encoded_bytes: 512_000,
            points_per_glyph: 1024,
            strokes: 65_536,
        };
        let font = StrokeFont::bundled_for_texts(
            std::slice::from_ref(&source),
            20,
            512_000,
            limits,
            &token,
        )
        .unwrap();
        for character in source.text.chars() {
            assert!(font.glyph(character).is_some());
        }
        assert!(
            font.glyphs.len() < 23_928,
            "only requested pages should decode"
        );
        source.text = "한".into();
        let diagnostic = StrokeFont::bundled_for_texts(
            std::slice::from_ref(&source),
            20,
            512_000,
            limits,
            &token,
        )
        .unwrap_err();
        assert_eq!(diagnostic.object, Some(source.id));
        assert_eq!(diagnostic.message.key, MessageKey::RenderTextMissingGlyph);
    }
    #[test]
    fn bundled_core_loads_latin_and_greek_without_external_directory() {
        let font = StrokeFont::bundled_core(
            FontLimits {
                glyphs: 512,
                encoded_bytes: 65_536,
                points_per_glyph: 1024,
                strokes: 65_536,
            },
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(font.glyphs.len(), 303);
        for character in "Aa09Ω".chars() {
            assert!(!font.glyph(character).unwrap().is_empty());
        }
        assert!(font.glyph(' ').unwrap().is_empty());
    }
    #[test]
    fn zero_size_page_discovery_skips_missing_glyph_but_checks_geometry_and_cancel() {
        let token = CancellationToken::default();
        let font = StrokeFont::load(
            [],
            FontLimits {
                glyphs: 0,
                encoded_bytes: 0,
                points_per_glyph: 0,
                strokes: 0,
            },
            &token,
        )
        .unwrap();
        let mut source = text();
        source.text = "한".into();
        source.width = 0.0;
        source.height = 0.0;
        source.spacing = 0.0;
        source.line_spacing = 0.0;
        assert!(
            font.required_pages(std::slice::from_ref(&source), &[], 0, &token)
                .unwrap()
                .is_empty()
        );
        source.at.y = f64::NAN;
        assert_eq!(
            font.required_pages(std::slice::from_ref(&source), &[], 0, &token)
                .unwrap_err()
                .code
                .as_ref(),
            "RENDER_TEXT_INVALID_GEOMETRY"
        );
        token.cancel();
        assert_eq!(
            font.required_pages(std::slice::from_ref(&source), &[], 0, &token)
                .unwrap_err()
                .code
                .as_ref(),
            "RENDER_TEXT_CANCELLED"
        );
    }
    #[test]
    fn required_pages_deduplicate_sort_and_report_unsupported_source_character() {
        let token = CancellationToken::default();
        let font = StrokeFont::load(
            [('A', "RR"), (' ', "RR")],
            FontLimits {
                glyphs: 2,
                encoded_bytes: 4,
                points_per_glyph: 0,
                strokes: 0,
            },
            &token,
        )
        .unwrap();
        let mut source = text();
        source.text = "A語中中\r\n\t".into();
        assert_eq!(
            font.required_pages(std::slice::from_ref(&source), &[0x8a, 0x4e], 8, &token)
                .unwrap(),
            [0x4e, 0x8a]
        );
        let diagnostic = font
            .required_pages(std::slice::from_ref(&source), &[0x4e], 8, &token)
            .unwrap_err();
        assert_eq!(diagnostic.object, Some(source.id));
        assert_eq!(diagnostic.message.key, MessageKey::RenderTextMissingGlyph);
        assert!(
            diagnostic
                .message
                .render(pomelo_core::i18n::Locale::English)
                .unwrap()
                .contains('語')
        );
        assert_eq!(
            font.required_pages(std::slice::from_ref(&source), &[0x4e, 0x8a], 1, &token)
                .unwrap_err()
                .code
                .as_ref(),
            "RENDER_TEXT_CHARACTER_LIMIT"
        );
    }
    #[test]
    fn multiple_font_pages_share_budgets_and_reject_cross_page_duplicates() {
        let token = CancellationToken::default();
        let pages = [
            br#"{"A":"RRR[RF"}"#.as_slice(),
            br#"{"B":"RRR[RF"}"#.as_slice(),
        ];
        let limits = FontLimits {
            glyphs: 2,
            encoded_bytes: 12,
            points_per_glyph: 2,
            strokes: 2,
        };
        let font = StrokeFont::load_json_pages(pages, 100, limits, &token).unwrap();
        assert!(font.glyph('A').is_some());
        assert!(font.glyph('B').is_some());
        for smaller in [
            FontLimits {
                glyphs: 1,
                ..limits
            },
            FontLimits {
                encoded_bytes: 11,
                ..limits
            },
            FontLimits {
                strokes: 1,
                ..limits
            },
        ] {
            assert_eq!(
                StrokeFont::load_json_pages(pages, 100, smaller, &token).unwrap_err(),
                FontLoadError::Limit
            );
        }
        assert_eq!(
            StrokeFont::load_json_pages(pages, pages[0].len(), limits, &token).unwrap_err(),
            FontLoadError::Limit
        );
        assert_eq!(
            StrokeFont::load_json_pages([pages[0], pages[0]], 100, limits, &token).unwrap_err(),
            FontLoadError::Duplicate('A')
        );
    }
    #[test]
    #[ignore = "requires external Web stroke resources via POMELO_STROKE_FONT_DIR"]
    fn external_stroke_json_resources_decode_with_bounded_rust_loader() {
        use std::io::Read;
        let directory = std::env::var_os("POMELO_STROKE_FONT_DIR")
            .expect("set POMELO_STROKE_FONT_DIR explicitly");
        let mut pages = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();
        pages.sort();
        assert!(!pages.is_empty());
        let cancellation = CancellationToken::default();
        let mut glyphs = 0;
        let mut strokes = 0;
        let mut cjk_probes = std::collections::BTreeSet::new();
        for path in &pages {
            let mut data = Vec::new();
            std::fs::File::open(path)
                .unwrap()
                .take(65_537)
                .read_to_end(&mut data)
                .unwrap();
            let font = StrokeFont::load_json(
                &data,
                65_536,
                FontLimits {
                    glyphs: 256,
                    encoded_bytes: 65_536,
                    points_per_glyph: 1024,
                    strokes: 65_536,
                },
                &cancellation,
            )
            .unwrap_or_else(|error| panic!("{}: {error:?}", path.display()));
            let block =
                u32::from_str_radix(path.file_stem().unwrap().to_str().unwrap(), 16).unwrap();
            assert!(
                font.glyphs
                    .keys()
                    .all(|character| u32::from(*character) >> 8 == block)
            );
            glyphs += font.glyphs.len();
            strokes += font.glyphs.values().map(Vec::len).sum::<usize>();
            for character in "中文繁體日本語".chars() {
                if font.glyph(character).is_some_and(|glyph| !glyph.is_empty()) {
                    cjk_probes.insert(character);
                }
            }
        }
        assert_eq!(cjk_probes, "中文繁體日本語".chars().collect());
        println!(
            "EXTERNAL_STROKE_FONT pages={} glyphs={glyphs} strokes={strokes}",
            pages.len()
        );
    }
    #[test]
    fn json_parser_enforces_entry_budget_before_reading_another_value() {
        let limits = FontLimits {
            glyphs: 1,
            encoded_bytes: 2,
            points_per_glyph: 2,
            strokes: 1,
        };
        let token = CancellationToken::default();
        assert_eq!(
            StrokeFont::load_json(br#"{"A":"RR","B":false}"#, 100, limits, &token).unwrap_err(),
            FontLoadError::Limit
        );
        assert_eq!(
            StrokeFont::load_json(br#"{"A":"RRR[RF"}"#, 100, limits, &token).unwrap_err(),
            FontLoadError::Limit
        );
        token.cancel();
        assert_eq!(
            StrokeFont::load_json(b"invalid", 0, limits, &token).unwrap_err(),
            FontLoadError::Cancelled
        );
    }
    #[test]
    fn json_font_rejects_duplicate_keys_trailing_data_and_non_glyph_maps() {
        let limits = FontLimits {
            glyphs: 2,
            encoded_bytes: 8,
            points_per_glyph: 2,
            strokes: 1,
        };
        let token = CancellationToken::default();
        let font =
            StrokeFont::load_json(br#"{"A":"RRR[RF"," ":"RR"}"#, 100, limits, &token).unwrap();
        assert_eq!(font.glyph('A').unwrap().len(), 1);
        assert!(matches!(
            StrokeFont::load_json(br#"{"A":"RR","A":"RR"}"#, 100, limits, &token),
            Err(FontLoadError::Duplicate('A'))
        ));
        for data in [
            br#"{"AB":"RR"}"#.as_slice(),
            b"[]",
            b"null",
            br#"{"A":1}"#,
            br#"{"A":"RR"} {}"#,
        ] {
            assert_eq!(
                StrokeFont::load_json(data, 100, limits, &token).unwrap_err(),
                FontLoadError::ResourceInvalid
            );
        }
        assert_eq!(
            StrokeFont::load_json(b"{}", 1, limits, &token).unwrap_err(),
            FontLoadError::Limit
        );
    }
    #[test]
    fn font_diagnostics_render_all_variants_in_five_languages() {
        let errors = [
            FontLoadError::Cancelled,
            FontLoadError::Limit,
            FontLoadError::AllocationFailed,
            FontLoadError::Duplicate('中'),
            FontLoadError::Glyph {
                character: '中',
                error: GlyphDecodeError::InvalidEncoding,
            },
            FontLoadError::Glyph {
                character: '中',
                error: GlyphDecodeError::PointLimit {
                    actual: 3,
                    limit: 2,
                },
            },
            FontLoadError::Glyph {
                character: '中',
                error: GlyphDecodeError::AllocationFailed,
            },
        ];
        for error in errors {
            let diagnostic = error.diagnostic();
            for locale in pomelo_core::i18n::Locale::ALL {
                let rendered = diagnostic.message.render(locale).unwrap();
                assert!(!rendered.is_empty());
                assert!(!rendered.contains("%{"));
                if matches!(
                    error,
                    FontLoadError::Duplicate(_)
                        | FontLoadError::Glyph {
                            error: GlyphDecodeError::InvalidEncoding,
                            ..
                        }
                ) {
                    assert!(rendered.contains('中'));
                }
            }
        }
    }
    #[test]
    fn font_loading_validates_resource_limits_duplicates_and_missing_characters() {
        let token = CancellationToken::default();
        let limits = FontLimits {
            glyphs: 2,
            encoded_bytes: 8,
            points_per_glyph: 2,
            strokes: 1,
        };
        let font = StrokeFont::load([(' ', "RR"), ('中', "RRR[RF")], limits, &token).unwrap();
        assert_eq!(font.glyph('中').unwrap().len(), 1);
        assert_eq!(font.glyph(' '), Some([].as_slice()));
        assert_eq!(font.glyph('?'), None);
        assert!(matches!(
            StrokeFont::load([('A', "RR"), ('A', "RR")], limits, &token),
            Err(FontLoadError::Duplicate('A'))
        ));
        assert!(matches!(
            StrokeFont::load([('A', "RRR[RF"), ('B', "RRR[RF")], limits, &token),
            Err(FontLoadError::Limit)
        ));
        assert!(matches!(
            StrokeFont::load([('A', "R")], limits, &token),
            Err(FontLoadError::Glyph {
                character: 'A',
                error: GlyphDecodeError::InvalidEncoding
            })
        ));
        token.cancel();
        assert!(matches!(
            StrokeFont::load([], limits, &token),
            Err(FontLoadError::Cancelled)
        ));
    }
    #[test]
    fn text_trace_instances_keep_source_identity_and_validate_ranges() {
        let token = CancellationToken::default();
        let mut prepared =
            PreparedTexts::build(&[text()], &SyntheticFont, 1, 2, 2, &token).unwrap();
        let tracks = crate::tracks::PreparedTracks::build_texts(
            &prepared,
            crate::tracks::TraceLimits::default(),
            &token,
        )
        .unwrap();
        assert_eq!(tracks.instances.len(), 2);
        assert!(
            tracks
                .instances
                .iter()
                .all(|instance| instance.ids == [1, 0, 1, 0])
        );
        assert!(tracks.batches.iter().all(|batch| !batch.outline));
        assert_eq!(tracks.instances[0].flags[2], 0.0_f32.to_bits());
        prepared.batches[0].strokes.end = 3;
        assert!(matches!(
            crate::tracks::PreparedTracks::build_texts(
                &prepared,
                crate::tracks::TraceLimits::default(),
                &token
            ),
            Err(crate::tracks::PrepareError::Invalid(ObjectId(1)))
        ));
    }
    #[test]
    fn empty_and_zero_size_text_cannot_bypass_source_geometry_validation() {
        for content in ["", "\r\n", " "] {
            let mut source = text();
            source.text = content.into();
            source.at.x = f64::NAN;
            assert_eq!(
                build_text(
                    &source,
                    &SyntheticFont,
                    10,
                    10,
                    &CancellationToken::default()
                ),
                Err(TextBuildError::Geometry(
                    TextTransformError::InvalidGeometry
                ))
            );
        }
        let mut source = text();
        source.width = 0.0;
        source.height = 0.0;
        source.spacing = 0.0;
        source.line_spacing = 0.0;
        source.angle = f64::INFINITY;
        assert_eq!(
            build_text(&source, &SyntheticFont, 0, 0, &CancellationToken::default()),
            Err(TextBuildError::Geometry(
                TextTransformError::InvalidGeometry
            ))
        );
    }
    #[test]
    fn aggregate_preserves_source_ranges_and_rejects_combined_budget() {
        let first = text();
        let mut second = text();
        second.id = ObjectId(77);
        second.layer = pomelo_core::model::LayerId(9);
        let texts = [first, second];
        let token = CancellationToken::default();
        let prepared = PreparedTexts::build(&texts, &SyntheticFont, 2, 4, 4, &token).unwrap();
        assert_eq!(prepared.characters, 4);
        assert_eq!(prepared.batches[0].strokes, 0..2);
        assert_eq!(prepared.batches[1].strokes, 2..4);
        assert_eq!(prepared.batches[1].object, ObjectId(77));
        assert_eq!(prepared.batches[1].layer, pomelo_core::model::LayerId(9));
        for (objects, characters, strokes, code) in [
            (1, 4, 4, "RENDER_TEXT_OBJECT_LIMIT"),
            (2, 3, 4, "RENDER_TEXT_CHARACTER_LIMIT"),
            (2, 4, 3, "RENDER_TEXT_STROKE_LIMIT"),
        ] {
            let diagnostic =
                PreparedTexts::build(&texts, &SyntheticFont, objects, characters, strokes, &token)
                    .unwrap_err();
            assert_eq!(diagnostic.code.as_ref(), code);
            assert_eq!(diagnostic.object, Some(ObjectId(77)));
        }
    }
    #[test]
    fn cancellation_during_last_glyph_lookup_discards_partial_geometry() {
        struct CancellingFont<'a> {
            token: &'a CancellationToken,
            calls: std::cell::Cell<usize>,
            result: Option<&'a [GlyphStroke]>,
        }
        impl StrokeGlyphs for CancellingFont<'_> {
            fn glyph(&self, _: char) -> Option<&[GlyphStroke]> {
                let calls = self.calls.get() + 1;
                self.calls.set(calls);
                if calls == 2 {
                    self.token.cancel();
                    self.result
                } else {
                    Some(&[GlyphStroke {
                        a: [0.0, 0.0],
                        b: [1.0, 1.0],
                    }])
                }
            }
        }
        for result in [
            Some([].as_slice()),
            None,
            Some(
                [GlyphStroke {
                    a: [0.0, 0.0],
                    b: [1.0, 1.0],
                }]
                .as_slice(),
            ),
        ] {
            let token = CancellationToken::default();
            let font = CancellingFont {
                token: &token,
                calls: std::cell::Cell::new(0),
                result,
            };
            assert_eq!(
                build_text(&text(), &font, 10, 10, &token),
                Err(TextBuildError::Cancelled)
            );
            assert_eq!(font.calls.get(), 2);
        }
    }
    #[test]
    fn diagnostics_preserve_object_and_missing_character_in_five_languages() {
        let errors = [
            TextBuildError::Cancelled,
            TextBuildError::CharacterLimit,
            TextBuildError::StrokeLimit,
            TextBuildError::AllocationFailed,
            TextBuildError::MissingGlyph('𠀀'),
            TextBuildError::Geometry(TextTransformError::InvalidGeometry),
        ];
        for error in errors {
            let diagnostic = error.diagnostic(ObjectId(123));
            assert_eq!(diagnostic.object, Some(ObjectId(123)));
            for locale in pomelo_core::i18n::Locale::ALL {
                let message = diagnostic.message.render(locale).unwrap();
                assert!(!message.is_empty());
                assert!(!message.contains("%{"));
                if matches!(error, TextBuildError::MissingGlyph(_)) {
                    assert!(message.contains('𠀀'));
                }
            }
        }
    }
    impl StrokeGlyphs for SyntheticFont {
        fn glyph(&self, character: char) -> Option<&[GlyphStroke]> {
            match character {
                'A' | '中' => Some(&[GlyphStroke {
                    a: [0.0, 0.0],
                    b: [1.0, 1.0],
                }]),
                ' ' => Some(&[]),
                _ => None,
            }
        }
    }

    #[test]
    fn string_layout_normalizes_newlines_tabs_and_unicode_columns() {
        let mut source = text();
        source.text = "A\t中\r\n中\rA\nA".into();
        let strokes = build_text(
            &source,
            &SyntheticFont,
            20,
            10,
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(strokes.len(), 5);
        assert_eq!(strokes[1].a, [25.0, 20.0]);
        assert_eq!(strokes[2].a, [10.0, 15.0]);
        assert_eq!(strokes[3].a, [10.0, 10.0]);
        assert_eq!(strokes[4].a, [10.0, 5.0]);
        assert!(strokes.iter().all(|stroke| stroke.width == 0.0));
    }

    #[test]
    fn text_build_rejects_missing_glyph_and_limits_and_observes_cancellation() {
        let mut source = text();
        let token = CancellationToken::default();
        assert_eq!(
            build_text(&source, &SyntheticFont, 1, 10, &token),
            Err(TextBuildError::CharacterLimit)
        );
        assert_eq!(
            build_text(&source, &SyntheticFont, 10, 1, &token),
            Err(TextBuildError::StrokeLimit)
        );
        source.text = "A?".into();
        assert_eq!(
            build_text(&source, &SyntheticFont, 10, 10, &token),
            Err(TextBuildError::MissingGlyph('?'))
        );
        token.cancel();
        assert_eq!(
            build_text(&source, &SyntheticFont, 0, 0, &token),
            Err(TextBuildError::Cancelled)
        );
    }

    #[test]
    fn zero_size_source_produces_no_hairline_dots() {
        let mut source = text();
        source.width = 0.0;
        source.height = 0.0;
        source.spacing = 0.0;
        source.line_spacing = 0.0;
        assert!(
            build_text(&source, &SyntheticFont, 0, 0, &CancellationToken::default())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn missing_glyph_recovery_preserves_supported_objects_and_aggregate_budgets() {
        let token = CancellationToken::default();
        let mut supported = text();
        supported.text = "A".into();
        let mut missing = text();
        missing.id = ObjectId(99);
        missing.text = "A?".into();
        let sources = [missing, supported];
        let (prepared, diagnostics) = PreparedTexts::build_recovering_missing_glyphs(
            &sources,
            &SyntheticFont,
            2,
            3,
            10,
            &token,
        )
        .unwrap();
        assert_eq!(prepared.batches.len(), 1);
        assert_eq!(prepared.batches[0].object, sources[1].id);
        assert_eq!(prepared.characters, 3);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].object, Some(ObjectId(99)));
        assert!(matches!(
            diagnostics[0].severity,
            pomelo_core::model::Severity::Warning
        ));
        assert!(
            PreparedTexts::build_recovering_missing_glyphs(
                &sources,
                &SyntheticFont,
                2,
                2,
                10,
                &token
            )
            .is_err()
        );
        assert!(
            PreparedTexts::build_recovering_missing_glyphs(
                &sources,
                &SyntheticFont,
                1,
                3,
                10,
                &token
            )
            .is_err()
        );
        token.cancel();
        assert!(
            PreparedTexts::build_recovering_missing_glyphs(
                &sources,
                &SyntheticFont,
                2,
                3,
                10,
                &token
            )
            .is_err()
        );
    }

    #[test]
    fn cancelled_recovery_preserves_absent_identity_and_precedes_zero_budget() {
        let token = CancellationToken::default();
        token.cancel();
        let empty =
            PreparedTexts::build_recovering_missing_glyphs(&[], &SyntheticFont, 0, 0, 0, &token)
                .unwrap_err();
        assert_eq!(empty.object, None);
        assert_eq!(empty.message.key, MessageKey::Cancelled);
        let source = text();
        let failure = PreparedTexts::build_recovering_missing_glyphs(
            std::slice::from_ref(&source),
            &SyntheticFont,
            0,
            0,
            0,
            &token,
        )
        .unwrap_err();
        assert_eq!(failure.object, Some(source.id));
        assert_eq!(failure.message.key, MessageKey::Cancelled);
        for locale in pomelo_core::i18n::Locale::ALL {
            assert!(!empty.message.display(locale).to_string().is_empty());
            assert!(!failure.message.display(locale).to_string().is_empty());
        }
    }

    #[test]
    fn bundled_recovery_keeps_latin_and_cjk_when_hangul_is_missing() {
        let token = CancellationToken::default();
        let mut supported = text();
        supported.text = "A中".into();
        let mut missing = text();
        missing.id = ObjectId(99);
        missing.text = "한".into();
        let sources = [missing, supported];
        let font = StrokeFont::bundled_for_recovering_texts(
            &sources,
            3,
            4 * 1024 * 1024,
            FontLimits {
                glyphs: 65536,
                encoded_bytes: 4 * 1024 * 1024,
                points_per_glyph: 1024,
                strokes: 1_000_000,
            },
            &token,
        )
        .unwrap();
        let (prepared, diagnostics) =
            PreparedTexts::build_recovering_missing_glyphs(&sources, &font, 2, 3, 1024, &token)
                .unwrap();
        assert_eq!(prepared.batches.len(), 1);
        assert_eq!(prepared.batches[0].object, sources[1].id);
        assert!(!prepared.strokes.is_empty());
        assert_eq!(diagnostics[0].object, Some(ObjectId(99)));
        assert_eq!(
            diagnostics[0].message.key,
            MessageKey::RenderTextMissingGlyph
        );
    }

    fn text() -> BoardText {
        BoardText {
            id: pomelo_core::model::ObjectId(1),
            owner_id: None,
            layer: pomelo_core::model::LayerId(1),
            class_id: 0,
            subclass: 0,
            text: "A中".into(),
            at: pomelo_core::model::Point { x: 10.0, y: 20.0 },
            angle: 0.0,
            mirrored: false,
            align: TextAlignment::Left,
            font_index: 0,
            width: 2.0,
            height: 3.0,
            spacing: 1.0,
            line_spacing: 5.0,
            stroke_width: 0.0,
        }
    }

    #[test]
    fn aligns_using_character_count_and_applies_row_spacing() {
        let mut source = text();
        for (align, expected) in [
            (TextAlignment::Left, [14.0, 18.0]),
            (TextAlignment::Center, [11.5, 18.0]),
            (TextAlignment::Right, [9.0, 18.0]),
        ] {
            source.align = align;
            let transform = GlyphTransform::new(&source, 1, 1, 2).unwrap();
            assert_eq!(transform.point([0.5, 1.0]).unwrap(), expected);
        }
        assert_eq!(source.text, "A中");
    }

    #[test]
    fn mirrors_before_rotation_and_rejects_invalid_geometry() {
        let mut source = text();
        source.mirrored = true;
        source.angle = std::f64::consts::FRAC_PI_2;
        let result = GlyphTransform::new(&source, 0, 0, 2)
            .unwrap()
            .point([0.5, 1.0])
            .unwrap();
        assert!((result[0] - 7.0).abs() < 1e-12);
        assert!((result[1] - 19.0).abs() < 1e-12);
        assert_eq!(
            GlyphTransform::new(&source, 0, 2, 2).unwrap_err(),
            TextTransformError::InvalidColumn
        );
        source.width = f64::MAX;
        assert_eq!(
            GlyphTransform::new(&source, 0, 0, 2).unwrap_err(),
            TextTransformError::InvalidGeometry
        );
        source.width = f64::NAN;
        assert_eq!(
            GlyphTransform::new(&source, 0, 0, 2).unwrap_err(),
            TextTransformError::InvalidGeometry
        );
    }

    #[test]
    fn separates_paths_and_normalizes_without_connecting_them() {
        // Synthetic data: bearings, two paths, then an isolated point.
        let strokes =
            decode_glyph(concat!("RR", "R[", "RF", " R", "R[", "RF", " R", "RR"), 5).unwrap();
        assert_eq!(strokes.len(), 2);
        assert_eq!(strokes[0].a, [0.5, 0.0]);
        assert_eq!(strokes[0].b, [0.5, 1.0]);
        assert_eq!(strokes[1].a, [0.5, 0.0]);
        assert_eq!(strokes[1].b, [0.5, 1.0]);
    }

    #[test]
    fn empty_and_single_point_glyphs_have_no_visible_strokes() {
        for encoded in ["RR", "RR R R", "RRR["] {
            assert!(decode_glyph(encoded, 1).unwrap().is_empty());
        }
    }

    #[test]
    fn validates_encoding_and_budget_before_geometry_allocation() {
        for encoded in ["", "R", "RRR", "RRé", "RR\nR", "RR\u{7f}R"] {
            assert_eq!(
                decode_glyph(encoded, 10),
                Err(GlyphDecodeError::InvalidEncoding)
            );
        }
        assert_eq!(
            decode_glyph("RRR[RFR R[RF", 3),
            Err(GlyphDecodeError::PointLimit {
                actual: 4,
                limit: 3
            })
        );
    }
}
