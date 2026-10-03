//! Recent-file thumbnail encoding and GPUI image adaptation, off the UI thread.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui_kit::RenderImage;
use image::ImageEncoder as _;
use pomelo_core::task::CancellationToken;
use pomelo_core::{i18n::MessageKey, model::Diagnostic};
use pomelo_render::scene::thumbnail::{self, Source};
use std::sync::Arc;

const MAX_PNG_BYTES: usize = 128 * 1024;
pub const MAX_ENCODED_BYTES: usize = MAX_PNG_BYTES.div_ceil(3) * 4;

pub struct PreparedPreview {
    pub image: Arc<RenderImage>,
    pub png: String,
}

pub fn rasterize(
    source: Source<'_>,
    cancellation: &CancellationToken,
) -> Result<Option<PreparedPreview>, Diagnostic> {
    let Some(thumbnail) = thumbnail::rasterize(
        source,
        crate::theme::thumbnail_palette(),
        thumbnail::Limits::default(),
        cancellation,
    )
    .map_err(|error| failure(MessageKey::RecentPreviewPrepareFailed, error.to_string()))?
    else {
        return Ok(None);
    };
    let pixels = image::RgbaImage::from_raw(
        thumbnail::WIDTH as u32,
        thumbnail::HEIGHT as u32,
        thumbnail.rgba,
    )
    .ok_or_else(|| {
        failure(
            MessageKey::RecentPreviewPrepareFailed,
            "PREVIEW_DIMENSIONS_INVALID",
        )
    })?;
    encode(pixels).map(Some)
}

fn encode(pixels: image::RgbaImage) -> Result<PreparedPreview, Diagnostic> {
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(pixels.as_raw(), 192, 128, image::ExtendedColorType::Rgba8)
        .map_err(|error| failure(MessageKey::RecentPreviewPrepareFailed, error.to_string()))?;
    if png.len() > MAX_PNG_BYTES {
        return Err(failure(
            MessageKey::RecentPreviewPrepareFailed,
            "PNG_SIZE_LIMIT",
        ));
    }
    Ok(PreparedPreview {
        image: render_image(pixels),
        png: STANDARD.encode(png),
    })
}

/// Decode a bounded cache image without trusting either dimensions or compressed bytes.
pub fn decode(encoded: &str) -> Result<Arc<RenderImage>, Diagnostic> {
    let failed = |details| failure(MessageKey::RecentPreviewLoadFailed, details);
    if encoded.len() > MAX_ENCODED_BYTES {
        return Err(failed("ENCODED_SIZE_LIMIT".into()));
    }
    let png = STANDARD
        .decode(encoded)
        .map_err(|_| failed("INVALID_BASE64".into()))?;
    if png.len() > MAX_PNG_BYTES {
        return Err(failed("PNG_SIZE_LIMIT".into()));
    }
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(192);
    limits.max_image_height = Some(128);
    limits.max_alloc = Some(1024 * 1024);
    reader.limits(limits);
    let pixels = reader
        .decode()
        .map_err(|error| failed(error.to_string()))?
        .into_rgba8();
    if pixels.dimensions() != (192, 128) {
        return Err(failed("PREVIEW_DIMENSIONS_INVALID".into()));
    }
    Ok(render_image(pixels))
}

fn render_image(mut rgba: image::RgbaImage) -> Arc<RenderImage> {
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new([image::Frame::new(rgba)]))
}

fn failure(key: MessageKey, details: impl Into<String>) -> Diagnostic {
    let mut diagnostic = Diagnostic::error("RECENT_PREVIEW_FAILED", key).with_details(details);
    diagnostic.severity = pomelo_core::model::Severity::Warning;
    diagnostic
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_png_and_gpui_bgra_preserve_distinct_channels() {
        let pixels = image::RgbaImage::from_pixel(192, 128, image::Rgba([17, 82, 193, 255]));
        let prepared = encode(pixels).unwrap();
        let png = STANDARD.decode(&prepared.png).unwrap();
        let natural = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        assert_eq!(natural.get_pixel(0, 0).0, [17, 82, 193, 255]);
        assert_eq!(
            &prepared.image.as_bytes(0).unwrap()[..4],
            &[193, 82, 17, 255]
        );
        assert_eq!(
            decode(&prepared.png).unwrap().as_bytes(0),
            prepared.image.as_bytes(0)
        );
    }

    #[test]
    fn invalid_or_oversized_previews_fail_with_translatable_diagnostics() {
        let mut wrong_size = Vec::new();
        image::codecs::png::PngEncoder::new(&mut wrong_size)
            .write_image(&[0; 4], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        for encoded in [
            "garbage".into(),
            STANDARD.encode(b"not png"),
            STANDARD.encode(wrong_size),
            "A".repeat(MAX_ENCODED_BYTES + 1),
        ] {
            let error = decode(&encoded).unwrap_err();
            for locale in pomelo_core::i18n::Locale::ALL {
                assert!(
                    !error
                        .message
                        .render(locale)
                        .unwrap()
                        .contains(error.message.key.as_str())
                );
            }
        }
        let valid = encode(image::RgbaImage::new(192, 128)).unwrap();
        let mut png = STANDARD.decode(valid.png).unwrap();
        png.truncate(png.len() / 2);
        assert!(decode(&STANDARD.encode(png)).is_err());

        let mut oversized_dimensions = Vec::new();
        image::codecs::png::PngEncoder::new(&mut oversized_dimensions)
            .write_image(
                image::RgbaImage::new(193, 128).as_raw(),
                193,
                128,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        assert!(decode(&STANDARD.encode(oversized_dimensions)).is_err());
    }
}
