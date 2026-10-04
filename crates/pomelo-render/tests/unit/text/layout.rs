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
    for (index, (packed, stroke)) in actual.instances.iter().zip(&expected.strokes).enumerate() {
        for (point, original) in [(packed.a, stroke.a), (packed.b, stroke.b)] {
            for axis in 0..2 {
                assert!(
                    (f64::from(point[axis]) + f64::from(point[axis + 2]) - original[axis]).abs()
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
    let expected = crate::tracks::PreparedTracks::build_texts(&prepared, limits, &token).unwrap();
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
fn font_diagnostics_render_all_variants_in_five_languages() {
    let errors = [
        FontLoadError::ResourceInvalid,
        FontLoadError::Cancelled,
        FontLoadError::Limit,
        FontLoadError::AllocationFailed,
    ];
    for error in errors {
        let diagnostic = error.diagnostic();
        for locale in pomelo_core::i18n::Locale::ALL {
            let rendered = diagnostic.message.render(locale).unwrap();
            assert!(!rendered.is_empty());
            assert!(!rendered.contains("%{"));
        }
    }
}
#[test]
fn text_trace_instances_keep_source_identity_and_validate_ranges() {
    let token = CancellationToken::default();
    let mut prepared = PreparedTexts::build(&[text()], &SyntheticFont, 1, 2, 2, &token).unwrap();
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
    let (prepared, diagnostics) =
        PreparedTexts::build_recovering_missing_glyphs(&sources, &SyntheticFont, 2, 3, 10, &token)
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
        PreparedTexts::build_recovering_missing_glyphs(&sources, &SyntheticFont, 2, 2, 10, &token)
            .is_err()
    );
    assert!(
        PreparedTexts::build_recovering_missing_glyphs(&sources, &SyntheticFont, 1, 3, 10, &token)
            .is_err()
    );
    token.cancel();
    assert!(
        PreparedTexts::build_recovering_missing_glyphs(&sources, &SyntheticFont, 2, 3, 10, &token)
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
