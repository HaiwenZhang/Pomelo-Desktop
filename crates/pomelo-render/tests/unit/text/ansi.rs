use super::*;

fn limits() -> FontLimits {
    FontLimits {
        glyphs: 128,
        encoded_bytes: 8192,
        points_per_glyph: 64,
        strokes: 256,
    }
}

fn data(program: &str) -> String {
    format!(
        "Width 100\nHeight 200\nDecender 50\n{}{}",
        "0\n".repeat(65),
        program
    )
}

#[test]
fn pen_moves_descenders_and_axis_operations_are_preserved() {
    let input =
        data("6 /* caller-supplied A */\n1 0 200\n3 0 0\n2 100 0\n1 100 -50\n4 50 100\n3 50 200\n");
    let font =
        AnsiStrokeFont::load(input.as_bytes(), limits(), &CancellationToken::default()).unwrap();
    assert_eq!(
        font.glyph('A').unwrap(),
        &[
            GlyphStroke {
                a: [0.0, 1.0],
                b: [0.0, 0.0]
            },
            GlyphStroke {
                a: [0.0, 0.0],
                b: [1.0, 0.0]
            },
            GlyphStroke {
                a: [1.0, -0.25],
                b: [0.5, 0.5]
            },
            GlyphStroke {
                a: [0.5, 0.5],
                b: [0.5, 1.0]
            },
        ]
    );
    assert_eq!(font.glyph(' '), Some([].as_slice()));
    assert!(font.glyph('中').is_none());
}

#[test]
fn malformed_programs_do_not_publish_partial_fonts() {
    for program in [
        "1\n4 0 0\n",
        "1\n",
        "-1\n",
        "2\n1 0 0\n9 1 2\n",
        "2\n1 0 0\n0 1 2\n",
        "1\n1 0 0 extra\n",
        "1\n1 0 2147483648\n",
    ] {
        assert_eq!(
            AnsiStrokeFont::load(
                data(program).as_bytes(),
                limits(),
                &CancellationToken::default()
            )
            .unwrap_err(),
            FontLoadError::ResourceInvalid,
            "{program}"
        );
    }
}

#[test]
fn explicit_endpoints_override_inconsistent_axis_hints() {
    let input = data("3\n1 0 200\n3 100 200\n2 0 0\n");
    let font =
        AnsiStrokeFont::load(input.as_bytes(), limits(), &CancellationToken::default()).unwrap();
    assert_eq!(
        font.glyph('A').unwrap(),
        &[
            GlyphStroke {
                a: [0.0, 1.0],
                b: [1.0, 1.0]
            },
            GlyphStroke {
                a: [1.0, 1.0],
                b: [0.0, 0.0]
            },
        ]
    );
}

#[test]
fn malformed_metrics_and_empty_fonts_are_rejected() {
    for input in [
        "Width 0\nHeight 200\nDecender 50\n0\n",
        "Width 100\nHeight -1\nDecender 50\n0\n",
        "Height 100\nWidth 200\nDecender 50\n0\n",
        "Width 100 extra\nHeight 200\nDecender 50\n0\n",
        "Width 100\nHeight 200\nDecender 50\n",
    ] {
        assert_eq!(
            AnsiStrokeFont::load(input.as_bytes(), limits(), &CancellationToken::default())
                .unwrap_err(),
            FontLoadError::ResourceInvalid
        );
    }
}

#[test]
fn byte_glyph_point_and_stroke_budgets_are_enforced() {
    let input = data("3\n1 0 0\n2 100 0\n3 100 200\n");
    let defaults = limits();
    for bounded in [
        FontLimits {
            encoded_bytes: input.len() - 1,
            ..defaults
        },
        FontLimits {
            glyphs: 65,
            ..defaults
        },
        FontLimits {
            points_per_glyph: 2,
            ..defaults
        },
        FontLimits {
            strokes: 1,
            ..defaults
        },
    ] {
        assert_eq!(
            AnsiStrokeFont::load(input.as_bytes(), bounded, &CancellationToken::default())
                .unwrap_err(),
            FontLoadError::Limit
        );
    }
}

#[test]
fn cancellation_precedes_parsing_and_has_shared_localized_diagnostic() {
    let token = CancellationToken::default();
    token.cancel();
    assert_eq!(
        AnsiStrokeFont::load(b"invalid", limits(), &token).unwrap_err(),
        FontLoadError::Cancelled
    );
    assert_eq!(
        FontLoadError::ResourceInvalid.diagnostic().code.as_ref(),
        "RENDER_FONT_RESOURCE_INVALID"
    );
}
