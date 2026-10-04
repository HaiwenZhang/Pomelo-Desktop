use super::*;
use pomelo_core::model::{DrillShape, NetId, Pad, PadKind, Pin};

fn index(angle: f64) -> LabelIndex {
    index_with_pad(angle, 0.5, 3.0)
}

fn index_with_pad(angle: f64, width: f64, height: f64) -> LabelIndex {
    let glyph = Glyph {
        uv: [0.0, 0.0, 1.0, 1.0],
        plane: [0.0, 0.0, 1.0, 1.0],
        advance: 1.0,
        page: 0,
    };
    let font = Arc::new(MsdfFont {
        glyphs: ['G', 'N', 'D', '?']
            .into_iter()
            .map(|ch| (ch, glyph))
            .collect(),
        pages: Vec::new(),
        cap_height: 1.0,
    });
    let scene = Arc::new(BoardScene {
        layers: Vec::new(),
        special_layers: Vec::new(),
        nets: [(NetId(1), "GND".into())].into(),
        segments: Vec::new(),
        pins: vec![Pin {
            id: ObjectId(11),
            owner_id: ObjectId(7),
            net: NetId(1),
            name: "1".into(),
            reference: "U1".into(),
            at: Point::new(2.0, 3.0),
            angle,
            mirrored: false,
            drill: 0.0,
            drill_shape: DrillShape {
                width: 0.0,
                height: 0.0,
                plated: false,
            },
            pads: vec![Pad {
                layer: LayerId(0),
                width,
                height,
                offset: Point::new(0.5, -0.25),
                kind: PadKind(3),
                corner: 0.0,
                inner_diameter: None,
                custom: None,
                backdrill: false,
                backdrill_base: false,
            }],
            stackup_region: None,
            die: None,
        }],
        components: Vec::new(),
        vias: Vec::new(),
        zones: Vec::new(),
        outline: Vec::new(),
        texts: Vec::new(),
        drawing_layers: Vec::new(),
        drawings: Vec::new(),
        bounds: Bounds {
            min: Point::new(-10.0, -10.0),
            max: Point::new(10.0, 10.0),
        },
        diagnostics: Vec::new(),
    });
    LabelIndex::build(scene, font, &CancellationToken::default()).unwrap()
}

#[test]
fn pin_label_toggle_retains_geometry_and_restores_the_same_packets() {
    let index = index(std::f64::consts::FRAC_PI_2);
    let mut display = BoardDisplay {
        horizontal_pin_names: true,
        ..BoardDisplay::default()
    };
    let camera = Camera {
        center: Point::new(2.0, 3.0),
        pixels_per_mm: 1000.0,
        flipped: false,
    };
    let layout = |display: &BoardDisplay| {
        index
            .layout(
                camera,
                1000.0,
                1000.0,
                display,
                display.label_options,
                &CancellationToken::default(),
            )
            .unwrap()
    };
    let visible = layout(&display);
    assert_eq!(visible.instances.len(), 3);
    display.label_options.pin_names = false;
    assert!(layout(&display).instances.is_empty());
    assert_eq!(index.scene.pins.len(), 1);
    display.label_options.pin_names = true;
    let restored = layout(&display);
    assert_eq!(visible.instances.len(), restored.instances.len());
    for (before, after) in visible.instances.iter().zip(&restored.instances) {
        assert_eq!(before.xywh, after.xywh);
        assert_eq!(before.uv, after.uv);
        assert_eq!(before.color, after.color);
        assert_eq!(before.rotation, after.rotation);
        assert_eq!(before.low, after.low);
        assert_eq!(before.ids, after.ids);
    }
}
#[test]
fn horizontal_pin_names_are_upright_for_rotated_and_flipped_pads() {
    for angle in [
        0.0,
        0.5,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
        -0.5,
    ] {
        let index = index(angle);
        for flipped in [false, true] {
            let display = BoardDisplay {
                horizontal_pin_names: true,
                ..BoardDisplay::default()
            };
            let labels = index
                .layout(
                    Camera {
                        center: Point::new(2.0, 3.0),
                        pixels_per_mm: 1000.0,
                        flipped,
                    },
                    1000.0,
                    1000.0,
                    &display,
                    LabelOptions::default(),
                    &CancellationToken::default(),
                )
                .unwrap();
            assert_eq!(labels.instances.len(), 3, "angle={angle} flipped={flipped}");
            for glyph in labels.instances {
                assert_eq!(
                    &glyph.rotation[..3],
                    &[1.0, 0.0, if flipped { -1.0 } else { 1.0 }]
                );
                assert_eq!(glyph.ids, [11, Category::Pin as u32, 0, 1]);
            }
        }
    }
}

#[test]
fn horizontal_pin_names_are_not_pruned_by_rotated_label_size() {
    // A square at45deg admits a slightly larger horizontal box. Retain a
    // genuine override-only LOD witness after automatic long-axis fitting.
    let index = index_with_pad(std::f64::consts::FRAC_PI_4, 0.5, 0.5);
    let mut display = BoardDisplay::default();
    let camera = Camera {
        center: Point::new(2.0, 3.0),
        pixels_per_mm: 56.0,
        flipped: false,
    };
    let rotated = index
        .layout(
            camera,
            500.0,
            500.0,
            &display,
            LabelOptions::default(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(rotated.instances.is_empty());
    display.horizontal_pin_names = true;
    let horizontal = index
        .layout(
            camera,
            500.0,
            500.0,
            &display,
            LabelOptions::default(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(horizontal.instances.len(), 3);
}

#[test]
fn horizontal_pin_name_box_fits_oblique_pad_local_dimensions() {
    let index = index(0.0);
    let pad = &index.scene.pins[0].pads[0];
    for angle in [0.0, 0.3, 0.8, std::f64::consts::FRAC_PI_2, -1.2] {
        let size = horizontal_pin_name_size(pad, angle, 3.0);
        let (sin, cos) = angle.sin_cos();
        let local_width = size * (3.0 * cos.abs() + sin.abs());
        let local_height = size * (3.0 * sin.abs() + cos.abs());
        assert!(
            local_width <= pad.width * 0.85 + 1e-12 && local_height <= pad.height * 0.85 + 1e-12,
            "angle={angle}: {local_width}x{local_height}"
        );
    }
}

#[test]
fn automatic_pin_names_follow_each_pads_long_axis_and_remain_readable_when_flipped() {
    for angle in [
        0.0,
        0.35,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI - 0.35,
        -0.7,
    ] {
        let LabelIndex { scene, font, .. } = index(angle);
        let mut scene = Arc::try_unwrap(scene).ok().unwrap();
        let mut horizontal = scene.pins[0].pads[0].clone();
        horizontal.layer = LayerId(1);
        horizontal.width = 3.0;
        horizontal.height = 0.5;
        scene.pins[0].pads.push(horizontal);
        let index =
            LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap();
        for flipped in [false, true] {
            let display = BoardDisplay::default();
            let labels = index
                .layout(
                    Camera {
                        center: Point::new(2.0, 3.0),
                        pixels_per_mm: 100.0,
                        flipped,
                    },
                    500.0,
                    500.0,
                    &display,
                    LabelOptions::default(),
                    &CancellationToken::default(),
                )
                .unwrap();
            assert_eq!(labels.instances.len(), 6);
            for glyph in labels.instances {
                let axis = angle
                    + if glyph.ids[2] == 0 {
                        std::f64::consts::FRAC_PI_2
                    } else {
                        0.0
                    };
                let world_x = f64::from(glyph.rotation[0] * glyph.rotation[2]);
                let world_y = f64::from(glyph.rotation[1]);
                assert!(
                    (world_x * axis.sin() - world_y * axis.cos()).abs() < 1e-6,
                    "layer={} angle={angle} flipped={flipped}: baseline must follow local long axis",
                    glyph.ids[2]
                );
                assert!(
                    glyph.rotation[0] >= -1e-6,
                    "camera-readable baseline must face screen-right"
                );
                assert!(
                    (glyph.xywh[2] - 0.325).abs() < 1e-6,
                    "size must fit advance along3mm and cap height across0.5mm"
                );
                assert_eq!(glyph.ids[0], 11);
                assert_eq!(glyph.ids[1], Category::Pin as u32);
            }
        }
    }
}

#[test]
fn automatic_long_axis_names_survive_index_lod_pruning_at_the_actual_fitted_size() {
    for (width, height) in [(0.5, 3.0), (3.0, 0.5)] {
        let index = index_with_pad(0.0, width, height);
        let display = BoardDisplay::default();
        for (scale, expected) in [(24.0, 0), (25.0, 3)] {
            let labels = index
                .layout(
                    Camera {
                        center: Point::new(2.0, 3.0),
                        pixels_per_mm: scale,
                        flipped: false,
                    },
                    500.0,
                    500.0,
                    &display,
                    LabelOptions::default(),
                    &CancellationToken::default(),
                )
                .unwrap();
            assert_eq!(
                labels.instances.len(),
                expected,
                "{width}x{height}mm at{scale}px/mm"
            );
        }
        assert!(
            index
                .entries
                .iter()
                .find(|entry| matches!(entry.source, Source::Pin(0)))
                .unwrap()
                .min_scale
                < 25.0
        );
    }
}

#[test]
fn automatic_equal_axis_pads_keep_source_pin_orientation() {
    let angle = 0.7;
    let index = index_with_pad(angle, 1.0, 1.0);
    let display = BoardDisplay::default();
    let labels = index
        .layout(
            Camera {
                center: Point::new(2.0, 3.0),
                pixels_per_mm: 100.0,
                flipped: false,
            },
            500.0,
            500.0,
            &display,
            LabelOptions::default(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(labels.instances.len(), 3);
    for glyph in labels.instances {
        assert!((f64::from(glyph.rotation[0]) - angle.cos()).abs() < 1e-6);
        assert!((f64::from(glyph.rotation[1]) - angle.sin()).abs() < 1e-6);
    }
}
