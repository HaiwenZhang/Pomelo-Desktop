//! Explicit per-text-block source font override; default board text stays MSDF.
use std::{fs::File, io::Read, path::PathBuf};

use pomelo_core::{
    i18n::MessageKey,
    model::{BoardScene, Diagnostic},
    task::CancellationToken,
};
use pomelo_render::text::{AnsiStrokeFont, FontLimits, instances::PreparedTextInstances};

#[derive(Debug, Clone)]
pub struct SourceFontConfig {
    pub path: PathBuf,
    pub text_block: u8,
}

impl SourceFontConfig {
    /// Called on the import worker. The bounded read also catches files growing
    /// after metadata inspection; caller-owned font bytes are never bundled.
    pub fn prepare(
        &self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<PreparedTextInstances, Diagnostic> {
        if cancel.is_cancelled() {
            return Err(pomelo_render::text::FontLoadError::Cancelled.diagnostic());
        }
        const BYTE_LIMIT: usize = 1024 * 1024;
        let read_error = |error: std::io::Error| {
            Diagnostic::error("RENDER_SOURCE_FONT_READ", MessageKey::SourceFontRead)
                .with_path(&self.path)
                .with_details(error.to_string())
        };
        let file = File::open(&self.path).map_err(read_error)?;
        if file.metadata().map_err(read_error)?.len() > BYTE_LIMIT as u64 {
            return Err(pomelo_render::text::FontLoadError::Limit
                .diagnostic()
                .with_path(&self.path));
        }
        let mut bytes = Vec::new();
        file.take((BYTE_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(read_error)?;
        let font = AnsiStrokeFont::load(
            &bytes,
            FontLimits {
                glyphs: 128,
                encoded_bytes: BYTE_LIMIT,
                points_per_glyph: 65536,
                strokes: 65536,
            },
            cancel,
        )
        .map_err(|error| error.diagnostic().with_path(&self.path))?;
        let mut source = PreparedTextInstances::build(
            scene
                .texts
                .iter()
                .filter(|text| text.font_index == self.text_block),
            &font,
            2_000_000,
            2_000_000,
            512 * 1024 * 1024,
            cancel,
        )?;
        for diagnostic in &mut source.summary.diagnostics {
            if diagnostic.message.key == MessageKey::RenderTextMissingGlyph {
                diagnostic.message.key = MessageKey::SourceFontFallback;
                diagnostic.code = "RENDER_SOURCE_FONT_FALLBACK".into();
            }
        }
        source.bind_drawing_owners(scene);
        Ok(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::model::{
        BoardDrawing, BoardText, Bounds, LayerId, NetId, ObjectId, Point, TextAlignment,
    };
    use std::sync::atomic::{AtomicU64, Ordering};

    struct FontFile(PathBuf);
    impl FontFile {
        fn new(bytes: &[u8]) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "pomelo-source-font-{}-{}.dat",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, bytes).unwrap();
            Self(path)
        }
        fn config(&self) -> SourceFontConfig {
            SourceFontConfig {
                path: self.0.clone(),
                text_block: 3,
            }
        }
    }
    impl Drop for FontFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    fn scene() -> BoardScene {
        let text = BoardText {
            id: ObjectId(1),
            owner_id: None,
            layer: LayerId(0),
            class_id: 6,
            subclass: 0,
            text: "A".into(),
            at: Point::new(10.0, 20.0),
            angle: 0.0,
            mirrored: false,
            align: TextAlignment::Left,
            font_index: 3,
            width: 2.0,
            height: 3.0,
            spacing: 1.0,
            line_spacing: 4.0,
            stroke_width: 0.2,
        };
        let mut missing = text.clone();
        missing.id = ObjectId(2);
        missing.text = "A中".into();
        let mut other = text.clone();
        other.id = ObjectId(3);
        other.font_index = 4;
        BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: Default::default(),
            segments: vec![],
            pins: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![text, missing, other],
            drawing_layers: vec![],
            drawings: vec![BoardDrawing {
                id: ObjectId(99),
                owner_id: None,
                layer: LayerId(0),
                net: NetId(0),
                graphic_ids: vec![],
                segments: vec![],
                text_ids: vec![ObjectId(1)],
            }],
            diagnostics: vec![],
            bounds: Bounds {
                min: Point::new(0.0, 0.0),
                max: Point::new(30.0, 30.0),
            },
        }
    }
    fn font_bytes() -> Vec<u8> {
        let mut bytes = b"Width 10\nHeight 10\nDecender 2\n".to_vec();
        for _ in 0..65 {
            bytes.extend_from_slice(b"0\n");
        }
        bytes.extend_from_slice(b"3\n1 0 0\n4 5 10\n4 10 0\n");
        bytes
    }
    #[test]
    fn source_font_scopes_blocks_preserves_parameters_and_falls_back_as_a_whole_object() {
        let file = FontFile::new(&font_bytes());
        let scene = scene();
        let source = file
            .config()
            .prepare(&scene, &CancellationToken::default())
            .unwrap();
        assert_eq!(source.objects, [ObjectId(1)]);
        assert_eq!(source.instances.len(), 2);
        assert_eq!(source.instances[0].a, [10.0, 20.0, 0.0, 0.0]);
        assert_eq!(source.instances[0].b, [11.0, 23.0, 0.0, 0.0]);
        assert_eq!(f32::from_bits(source.instances[0].ids[3]), 0.2);
        assert_eq!(source.instances[0].flags, [99, 1, 0, 0]);
        assert_eq!(source.pick_quads.len(), 2);
        assert!(
            source
                .pick_quads
                .iter()
                .all(|quad| quad.text == ObjectId(1))
        );
        let fallback = &source.summary.diagnostics[0];
        assert_eq!(fallback.object, Some(ObjectId(2)));
        assert_eq!(fallback.message.key, MessageKey::SourceFontFallback);
        for locale in pomelo_core::i18n::Locale::ALL {
            assert!(fallback.message.render(locale).is_ok());
        }
        let msdf = std::sync::Arc::new(
            pomelo_render::text::msdf::MsdfFont::bundled(
                scene.texts.iter().map(|text| text.text.as_str()),
                &CancellationToken::default(),
            )
            .unwrap(),
        );
        let glyphs = pomelo_render::text::msdf::PreparedGlyphs::build(
            scene
                .texts
                .iter()
                .filter(|text| !source.objects.contains(&text.id)),
            msdf,
            1024 * 1024,
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(glyphs.objects, [ObjectId(2), ObjectId(3)]);
        assert!(
            glyphs
                .pick_quads
                .iter()
                .all(|quad| quad.text != ObjectId(1))
        );
    }
    #[test]
    fn source_font_file_failures_and_cancellation_keep_structured_diagnostics() {
        let file = FontFile::new(b"malformed");
        let scene = scene();
        assert_eq!(
            file.config()
                .prepare(&scene, &CancellationToken::default())
                .unwrap_err()
                .message
                .key,
            MessageKey::RenderFontResourceInvalid
        );
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert_eq!(
            file.config()
                .prepare(&scene, &cancel)
                .unwrap_err()
                .message
                .key,
            MessageKey::Cancelled
        );
        let mut config = file.config();
        config.path = file.0.with_extension("missing");
        let error = config
            .prepare(&scene, &CancellationToken::default())
            .unwrap_err();
        assert_eq!(error.message.key, MessageKey::SourceFontRead);
        for locale in pomelo_core::i18n::Locale::ALL {
            assert!(error.message.render(locale).is_ok());
        }
        let oversized = FontFile::new(&vec![b'0'; 1024 * 1024 + 1]);
        assert_eq!(
            oversized
                .config()
                .prepare(&scene, &CancellationToken::default())
                .unwrap_err()
                .message
                .key,
            MessageKey::RenderFontLimit
        );
    }
}
