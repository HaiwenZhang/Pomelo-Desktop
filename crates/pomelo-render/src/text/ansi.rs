//! ANSI single-line font data supplied by the caller, without bundled vendor assets.
use super::{FontLimits, FontLoadError, GlyphStroke, StrokeGlyphs};
use pomelo_core::task::CancellationToken;

/// Normalized ANSI pen paths. This provider does not replace the default MSDF font.
/// The caller owns file access and supplies the font bytes explicitly.
#[derive(Debug)]
pub struct AnsiStrokeFont {
    glyphs: Vec<Vec<GlyphStroke>>,
}

impl AnsiStrokeFont {
    /// Decode Width/Height/Decender headers and sequential ASCII pen programs.
    /// Operation 1 moves the pen; 2, 3 and 4 draw horizontal, vertical and general
    /// lines. Descenders remain below the baseline; move operations never draw.
    ///
    /// # Errors
    /// Rejects malformed or truncated data, unsupported operations, budget excess
    /// and cancellation using the existing localized font diagnostics.
    pub fn load(
        bytes: &[u8],
        limits: FontLimits,
        cancellation: &CancellationToken,
    ) -> Result<Self, FontLoadError> {
        if cancellation.is_cancelled() {
            return Err(FontLoadError::Cancelled);
        }
        if bytes.len() > limits.encoded_bytes {
            return Err(FontLoadError::Limit);
        }
        let input = std::str::from_utf8(bytes).map_err(|_| FontLoadError::ResourceInvalid)?;
        let mut rows = input.lines().filter_map(|line| {
            let line = line
                .split_once("/*")
                .map_or(line, |(prefix, _)| prefix)
                .trim();
            (!line.is_empty()).then_some(line)
        });
        let width = header(rows.next(), "Width")?;
        let height = header(rows.next(), "Height")?;
        header(rows.next(), "Decender")?;
        let mut glyphs = Vec::new();
        let mut total_strokes = 0usize;
        while let Some(count) = rows.next() {
            if cancellation.is_cancelled() {
                return Err(FontLoadError::Cancelled);
            }
            if glyphs.len() >= limits.glyphs.min(128) {
                return Err(FontLoadError::Limit);
            }
            let count: usize = count.parse().map_err(|_| FontLoadError::ResourceInvalid)?;
            if count > limits.points_per_glyph {
                return Err(FontLoadError::Limit);
            }
            let mut glyph = Vec::new();
            let mut pen: Option<[i32; 2]> = None;
            for _ in 0..count {
                if cancellation.is_cancelled() {
                    return Err(FontLoadError::Cancelled);
                }
                let row = rows.next().ok_or(FontLoadError::ResourceInvalid)?;
                let mut fields = row.split_whitespace();
                let operation = number(fields.next())?;
                let point = [number(fields.next())?, number(fields.next())?];
                if fields.next().is_some() {
                    return Err(FontLoadError::ResourceInvalid);
                }
                if operation != 1 {
                    let previous = pen.ok_or(FontLoadError::ResourceInvalid)?;
                    // Real ANSI data contains axis hints that disagree with the
                    // endpoint (notably 'I' and '1'); the explicit endpoint wins.
                    if !(2..=4).contains(&operation) {
                        return Err(FontLoadError::ResourceInvalid);
                    }
                    if total_strokes >= limits.strokes {
                        return Err(FontLoadError::Limit);
                    }
                    glyph
                        .try_reserve(1)
                        .map_err(|_| FontLoadError::AllocationFailed)?;
                    let normalized =
                        |p: [i32; 2]| [f64::from(p[0]) / width, f64::from(p[1]) / height];
                    glyph.push(GlyphStroke {
                        a: normalized(previous),
                        b: normalized(point),
                    });
                    total_strokes += 1;
                }
                pen = Some(point);
            }
            glyphs
                .try_reserve(1)
                .map_err(|_| FontLoadError::AllocationFailed)?;
            glyphs.push(glyph);
        }
        if cancellation.is_cancelled() {
            return Err(FontLoadError::Cancelled);
        }
        if glyphs.is_empty() {
            return Err(FontLoadError::ResourceInvalid);
        }
        Ok(Self { glyphs })
    }
}

fn number(field: Option<&str>) -> Result<i32, FontLoadError> {
    field
        .ok_or(FontLoadError::ResourceInvalid)?
        .parse()
        .map_err(|_| FontLoadError::ResourceInvalid)
}

fn header(row: Option<&str>, name: &str) -> Result<f64, FontLoadError> {
    let mut fields = row
        .ok_or(FontLoadError::ResourceInvalid)?
        .split_whitespace();
    if fields.next() != Some(name) {
        return Err(FontLoadError::ResourceInvalid);
    }
    let value = number(fields.next())?;
    if value <= 0 || fields.next().is_some() {
        return Err(FontLoadError::ResourceInvalid);
    }
    Ok(f64::from(value))
}

impl StrokeGlyphs for AnsiStrokeFont {
    fn glyph(&self, character: char) -> Option<&[GlyphStroke]> {
        self.glyphs.get(character as usize).map(Vec::as_slice)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/text/ansi.rs"]
mod tests;
