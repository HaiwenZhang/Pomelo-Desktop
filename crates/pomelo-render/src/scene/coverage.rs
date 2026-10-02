//! Explicit gaps between the imported scene and the current Windows renderer.

use pomelo_core::{
    i18n::{Message, MessageKey},
    model::{BoardScene, Diagnostic, Severity},
};

/// Report parsed text and dimension labels not submitted by the board renderer.
/// Keep these separate from parser diagnostics: valid source data is retained.
pub fn diagnostics(scene: &BoardScene) -> Vec<Diagnostic> {
    diagnostics_with_texts(scene, false)
}

pub fn diagnostics_with_texts(scene: &BoardScene, texts_prepared: bool) -> Vec<Diagnostic> {
    let text_ids = if texts_prepared {
        scene.texts.iter().map(|text| text.id).collect()
    } else {
        std::collections::BTreeSet::new()
    };
    diagnostics_with_text_ids(scene, &text_ids)
}

/// Coverage follows successful source objects, including empty/zero-size text.
pub fn diagnostics_with_text_ids(
    scene: &BoardScene,
    text_ids: &std::collections::BTreeSet<pomelo_core::model::ObjectId>,
) -> Vec<Diagnostic> {
    let mut result = Vec::new();
    let missing = scene
        .texts
        .iter()
        .filter(|text| !text_ids.contains(&text.id));
    let missing_count = missing.clone().count();
    if missing_count != 0 {
        let mut diagnostic = Diagnostic::error(
            "RENDER_TEXT_NOT_IMPLEMENTED",
            MessageKey::RenderTextsUnavailable,
        );
        diagnostic.severity = Severity::Warning;
        diagnostic.message =
            Message::new(MessageKey::RenderTextsUnavailable).arg("count", missing_count);
        diagnostic.object = missing.into_iter().next().map(|text| text.id);
        result.push(diagnostic);
    }
    let labeled_drawings = scene.drawings.iter().filter(|drawing| {
        !drawing.text_ids.is_empty() && drawing.text_ids.iter().any(|id| !text_ids.contains(id))
    });
    let labeled_count = labeled_drawings.clone().count();
    if labeled_count != 0 {
        let mut diagnostic = Diagnostic::error(
            "RENDER_DIMENSION_LABEL_NOT_IMPLEMENTED",
            MessageKey::RenderDrawingsUnavailable,
        );
        diagnostic.severity = Severity::Warning;
        diagnostic.message =
            Message::new(MessageKey::RenderDrawingsUnavailable).arg("count", labeled_count);
        diagnostic.object = labeled_drawings
            .into_iter()
            .next()
            .map(|drawing| drawing.id);
        result.push(diagnostic);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use pomelo_core::model::*;

    #[test]
    fn partial_text_coverage_keeps_missing_object_and_dimension_diagnostics() {
        let scene = scene();
        let supported = std::collections::BTreeSet::from([ObjectId(999)]);
        let diagnostics = diagnostics_with_text_ids(&scene, &supported);
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].object, Some(ObjectId(7)));
        let supported = std::collections::BTreeSet::from([ObjectId(7)]);
        assert!(diagnostics_with_text_ids(&scene, &supported).is_empty());
    }

    fn scene() -> BoardScene {
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
            drawing_layers: vec![],
            diagnostics: vec![],
            bounds: Bounds {
                min: Point::new(0.0, 0.0),
                max: Point::new(1.0, 1.0),
            },
            texts: vec![BoardText {
                id: ObjectId(7),
                owner_id: None,
                layer: LayerId(1),
                class_id: 0,
                subclass: 0,
                text: "A".into(),
                at: Point::new(0.0, 0.0),
                angle: 0.0,
                mirrored: false,
                align: TextAlignment::Left,
                font_index: 0,
                width: 1.0,
                height: 1.0,
                spacing: 0.0,
                line_spacing: 1.0,
                stroke_width: 0.1,
            }],
            drawings: vec![BoardDrawing {
                id: ObjectId(8),
                owner_id: None,
                layer: LayerId(1),
                net: NetId(0),
                graphic_ids: vec![],
                segments: vec![],
                text_ids: vec![ObjectId(7)],
            }],
        }
    }

    #[test]
    fn prepared_text_removes_gap_but_missing_dimension_reference_remains_reported() {
        let mut source = scene();
        assert_eq!(diagnostics(&source).len(), 2);
        assert!(diagnostics_with_texts(&source, true).is_empty());
        source.drawings[0].text_ids.push(ObjectId(99));
        let gaps = diagnostics_with_texts(&source, true);
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].object, Some(ObjectId(8)));
        assert_eq!(gaps[0].message.key, MessageKey::RenderDrawingsUnavailable);
        for locale in pomelo_core::i18n::Locale::ALL {
            assert!(!gaps[0].message.render(locale).unwrap().contains("%{"));
        }
        assert_eq!(source.drawings[0].text_ids, [ObjectId(7), ObjectId(99)]);
    }
}
