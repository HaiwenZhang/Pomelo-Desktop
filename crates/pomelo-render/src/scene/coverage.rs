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
#[path = "../../tests/unit/scene/coverage.rs"]
mod tests;
