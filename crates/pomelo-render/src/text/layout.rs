//! Stroke text layout and transforms, independent of font resources and GPU backends.

use pomelo_core::model::{BoardText, TextAlignment};
use pomelo_core::task::CancellationToken;
use pomelo_core::{
    i18n::MessageKey,
    model::{Diagnostic, ObjectId},
};

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
    AllocationFailed,
}

impl FontLoadError {
    pub fn diagnostic(&self) -> Diagnostic {
        let (code, key) = match self {
            Self::ResourceInvalid => (
                "RENDER_FONT_RESOURCE_INVALID",
                MessageKey::RenderFontResourceInvalid,
            ),
            Self::Cancelled => ("RENDER_FONT_CANCELLED", MessageKey::Cancelled),
            Self::Limit => ("RENDER_FONT_LIMIT", MessageKey::RenderFontLimit),
            Self::AllocationFailed => (
                "RENDER_FONT_ALLOCATION",
                MessageKey::RenderPrepareAllocation,
            ),
        };
        Diagnostic::error(code, key)
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
pub fn count_text_strokes<'a>(
    texts: impl IntoIterator<Item = &'a BoardText>,
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
    pub fn visit_recovering_missing_glyphs<'a>(
        texts: impl IntoIterator<Item = &'a BoardText>,
        font: &impl StrokeGlyphs,
        max_objects: usize,
        max_characters: usize,
        max_strokes: usize,
        cancellation: &CancellationToken,
        mut visit: impl FnMut(&BoardText, &[TextStroke]) -> Result<(), Diagnostic>,
    ) -> Result<TextPreparationSummary, Diagnostic> {
        let mut texts = texts.into_iter().peekable();
        let mut last_object = None;
        if cancellation.is_cancelled() {
            let mut diagnostic = Diagnostic::error("RENDER_TEXT_CANCELLED", MessageKey::Cancelled);
            diagnostic.object = texts.peek().map(|text| text.id);
            return Err(diagnostic);
        }
        let mut characters_count = 0;
        let mut strokes_count = 0;
        let mut objects_count = 0;
        let mut diagnostics = Vec::new();
        for (index, text) in texts.enumerate() {
            last_object = Some(text.id);
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
            diagnostic.object = last_object;
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
#[path = "../../tests/unit/text/layout.rs"]
mod tests;
