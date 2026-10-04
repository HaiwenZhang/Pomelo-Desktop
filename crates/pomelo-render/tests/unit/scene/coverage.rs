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
