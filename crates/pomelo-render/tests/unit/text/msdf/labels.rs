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
                custom: None,
                rectangular: false,
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

#[test]
fn multilayer_names_overlap_by_default_and_active_scope_is_explicit() {
    let LabelIndex { scene, font, .. } = index(0.0);
    let mut scene = Arc::try_unwrap(scene).ok().unwrap();
    let mut second = scene.pins[0].pads[0].clone();
    second.layer = LayerId(1);
    scene.pins[0].pads.push(second);
    let index = LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap();
    let mut display = BoardDisplay {
        active_layer: Some(LayerId(1)),
        ..Default::default()
    };
    let layout = |display: &BoardDisplay| {
        index
            .layout(
                Camera {
                    center: Point::new(2.0, 3.0),
                    pixels_per_mm: 100.0,
                    flipped: false,
                },
                500.0,
                500.0,
                display,
                display.label_options,
                &CancellationToken::default(),
            )
            .unwrap()
    };
    let all = layout(&display);
    assert_eq!(
        all.instances.len(),
        6,
        "coincident labels from two visible layers are retained"
    );
    display.active_layer_labels_only = true;
    let active = layout(&display);
    assert_eq!(active.instances.len(), 3);
    assert!(active.instances.iter().all(|g| g.ids[2] == 1));
    display.hidden_layers.insert(LayerId(1));
    assert!(
        layout(&display).instances.is_empty(),
        "active scope must never reveal a hidden layer"
    );
}

#[test]
fn dense_visible_names_keep_complete_words_at_the_glyph_budget() {
    let LabelIndex { scene, font, .. } = index(0.0);
    let mut scene = Arc::try_unwrap(scene).ok().unwrap();
    let pin = scene.pins[0].clone();
    scene.pins = (0..22_000)
        .map(|i| {
            let mut pin = pin.clone();
            pin.id = ObjectId(i + 1);
            pin
        })
        .collect();
    let index = LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap();
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
            display.label_options,
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(labels.instances.len(), 65_535);
    assert!(
        labels
            .instances
            .as_chunks::<3>()
            .0
            .iter()
            .all(|word| word.iter().all(|g| g.ids == word[0].ids))
    );
}

#[test]
fn copper_names_use_three_view_relative_placements_and_a_ten_pixel_cutoff() {
    use pomelo_core::{
        copper::{CopperMesh, MeshLimits},
        model::{Zone, ZoneKind},
    };
    let LabelIndex { scene, font, .. } = index(0.0);
    let mut scene = Arc::try_unwrap(scene).ok().unwrap();
    scene.pins.clear();
    let ring = vec![
        Point::new(-10.0, -10.0),
        Point::new(10.0, -10.0),
        Point::new(10.0, 10.0),
        Point::new(-10.0, 10.0),
    ];
    scene.zones = vec![Zone {
        id: ObjectId(70),
        layer: LayerId(0),
        net: NetId(1),
        kind: ZoneKind::Dynamic,
        paths: vec![],
        mesh: CopperMesh::build(
            &[ring],
            &[],
            &MeshLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    }];
    let index = LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap();
    let display = BoardDisplay::default();
    for (scale, count) in [(1.0, 0), (100.0, 9), (1000.0, 9)] {
        let labels = index
            .layout(
                Camera {
                    center: Point::default(),
                    pixels_per_mm: scale,
                    flipped: false,
                },
                1000.0,
                1000.0,
                &display,
                display.label_options,
                &CancellationToken::default(),
            )
            .unwrap();
        assert_eq!(labels.instances.len(), count, "scale={scale}");
        assert!(
            labels
                .instances
                .iter()
                .all(|g| g.ids[1] == Category::Zone as u32)
        );
    }
}

#[test]
fn active_scope_filters_track_copper_and_via_names_not_just_pin_pads() {
    use pomelo_core::{
        copper::{CopperMesh, MeshLimits},
        model::{Segment, Via, Zone, ZoneKind},
    };
    let LabelIndex { scene, font, .. } = index(0.0);
    let mut scene = Arc::try_unwrap(scene).ok().unwrap();
    scene.pins.clear();
    scene.segments = vec![Segment {
        id: ObjectId(20),
        track_id: ObjectId(20),
        layer: LayerId(1),
        net: NetId(1),
        a: Point::new(-4.0, 0.0),
        b: Point::new(4.0, 0.0),
        width: 1.0,
        arc: None,
        bond_wire: None,
    }];
    scene.vias = vec![Via {
        id: ObjectId(30),
        net: NetId(1),
        at: Point::default(),
        drill: 0.2,
        drill_shape: DrillShape {
            custom: None,
            rectangular: false,
            width: 0.2,
            height: 0.2,
            plated: true,
        },
        padstack: ObjectId(31),
        padstack_name: String::new(),
        start_layer: Some(LayerId(1)),
        end_layer: Some(LayerId(2)),
        pads: Arc::from([Pad::circle(LayerId(1), 1.0)]),
        backdrill: None,
        stackup_region: None,
        angle: 0.0,
        mirrored: false,
        finger: None,
    }];
    let ring = vec![
        Point::new(-5.0, -5.0),
        Point::new(5.0, -5.0),
        Point::new(5.0, 5.0),
        Point::new(-5.0, 5.0),
    ];
    scene.zones = vec![Zone {
        id: ObjectId(40),
        layer: LayerId(1),
        net: NetId(1),
        kind: ZoneKind::Dynamic,
        paths: vec![],
        mesh: CopperMesh::build(
            &[ring],
            &[],
            &MeshLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    }];
    let index = LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap();
    let mut display = BoardDisplay {
        active_layer: Some(LayerId(0)),
        ..Default::default()
    };
    display.label_options.via_names = true;
    let layout = |display: &BoardDisplay| {
        index
            .layout(
                Camera {
                    center: Point::default(),
                    pixels_per_mm: 100.0,
                    flipped: false,
                },
                1000.0,
                1000.0,
                display,
                display.label_options,
                &CancellationToken::default(),
            )
            .unwrap()
    };
    let all = layout(&display);
    for category in [Category::Trace, Category::Zone, Category::Drill] {
        assert!(all.instances.iter().any(|g| g.ids[1] == category as u32));
    }
    display.active_layer_labels_only = true;
    assert!(layout(&display).instances.is_empty());
    display.active_layer = Some(LayerId(1));
    assert_eq!(layout(&display).instances.len(), all.instances.len());
}

#[test]
fn foreground_names_win_when_dense_lower_layers_exhaust_the_word_budget() {
    use pomelo_core::model::Segment;
    let LabelIndex { scene, font, .. } = index(0.0);
    let mut scene = Arc::try_unwrap(scene).ok().unwrap();
    let mut pin = scene.pins[0].clone();
    pin.pads[0].layer = LayerId(1);
    scene.pins = (0..22_000)
        .map(|i| {
            let mut p = pin.clone();
            p.id = ObjectId(i + 1);
            p
        })
        .collect();
    scene.segments = vec![Segment {
        id: ObjectId(99_999),
        track_id: ObjectId(99_999),
        layer: LayerId(0),
        net: NetId(1),
        a: Point::new(-4.0, 0.0),
        b: Point::new(4.0, 0.0),
        width: 1.0,
        arc: None,
        bond_wire: None,
    }];
    let index = LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap();
    let display = BoardDisplay {
        active_layer: Some(LayerId(0)),
        ..Default::default()
    };
    let labels = index
        .layout(
            Camera {
                center: Point::default(),
                pixels_per_mm: 100.0,
                flipped: false,
            },
            1000.0,
            1000.0,
            &display,
            display.label_options,
            &CancellationToken::default(),
        )
        .unwrap();
    assert!(labels.instances.iter().any(|g| g.ids[0] == 99_999));
    assert!(labels.instances.len() <= 65_536);
    assert!(
        labels
            .instances
            .as_chunks::<3>()
            .0
            .iter()
            .all(|word| word.iter().all(|g| g.ids == word[0].ids))
    );
}

#[test]
fn embedded_name_filter_does_not_hide_independent_drill_span_labels() {
    use pomelo_core::model::{Layer, LayerFunction, Via};
    let LabelIndex { scene, font, .. } = index(0.0);
    let mut scene = Arc::try_unwrap(scene).ok().unwrap();
    scene.pins.clear();
    scene.layers = (0..2)
        .map(|id| Layer {
            id: LayerId(id),
            name: format!("L{id}"),
            function: LayerFunction::Conductor,
            color: String::new(),
            source_flags: None,
        })
        .collect();
    scene.vias = vec![Via {
        id: ObjectId(21),
        net: NetId(1),
        at: Point::default(),
        drill: 0.2,
        drill_shape: DrillShape {
            custom: None,
            rectangular: false,
            width: 0.2,
            height: 0.2,
            plated: true,
        },
        padstack: ObjectId(22),
        padstack_name: String::new(),
        start_layer: Some(LayerId(0)),
        end_layer: Some(LayerId(1)),
        pads: Arc::from([Pad::circle(LayerId(0), 1.0), Pad::circle(LayerId(1), 1.0)]),
        backdrill: None,
        stackup_region: None,
        angle: 0.0,
        mirrored: false,
        finger: None,
    }];
    let index = LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap();
    let mut display = BoardDisplay::default();
    display.label_options.via_names = true;
    let layout = |display: &BoardDisplay| {
        index
            .layout(
                Camera {
                    center: Point::default(),
                    pixels_per_mm: 50.0,
                    flipped: false,
                },
                1000.0,
                1000.0,
                display,
                display.label_options,
                &CancellationToken::default(),
            )
            .unwrap()
    };
    assert_eq!(layout(&display).instances.len(), 6);
    display.object_filter.minimum_pixels = 20;
    let spans = layout(&display);
    assert_eq!(spans.instances.len(), 3);
    assert!(
        spans
            .instances
            .iter()
            .all(|g| (g.rotation[3] as u32) & 1 == 1)
    );
    display.label_options.thru_labels = false;
    assert!(layout(&display).instances.is_empty());
}

#[test]
fn placement_references_follow_body_zoom_side_and_flip_without_decluttering() {
    use pomelo_core::{
        display::PlacementQuickview,
        model::{ComponentBody, ComponentPlacement, ComponentRegion, Segment},
    };
    let LabelIndex { scene, font, .. } = index(0.0);
    let mut scene = Arc::try_unwrap(scene).ok().unwrap();
    scene.pins.clear();
    let at = Point::new(20.0, -30.0);
    let angle = 0.6_f64;
    let world = |(x, y): (f64, f64)| {
        Point::new(
            at.x + x * angle.cos() - y * angle.sin(),
            at.y + x * angle.sin() + y * angle.cos(),
        )
    };
    let points = [(-3.0, -1.0), (3.0, -1.0), (3.0, 1.0), (-3.0, 1.0)];
    let component = |id, layer| ComponentPlacement {
        id: ObjectId(id),
        source_reference: None,
        reference: "GND".into(),
        at,
        angle,
        mirrored: layer == LayerId(1),
        pins: vec![],
        body: Some(ComponentBody {
            layer,
            regions: vec![ComponentRegion {
                source: ObjectId(id + 100),
                source_layer: LayerId::allegro_drawing(0xfb09),
                paths: vec![
                    points
                        .iter()
                        .enumerate()
                        .map(|(i, &p)| Segment {
                            id: ObjectId(id + 100),
                            track_id: ObjectId(id + 100),
                            layer,
                            net: NetId(0),
                            a: world(p),
                            b: world(points[(i + 1) % points.len()]),
                            width: 0.0,
                            arc: None,
                            bond_wire: None,
                        })
                        .collect(),
                ],
            }],
        }),
    };
    scene.components = vec![
        component(80, LayerId(0)),
        component(81, LayerId(0)),
        component(82, LayerId(1)),
    ];
    let mut missing = component(83, LayerId(0));
    missing.body = None;
    scene.components.push(missing);
    let index = LabelIndex::build(Arc::new(scene), font, &CancellationToken::default()).unwrap();
    let mut display = BoardDisplay {
        show_texts: false,
        active_layer: Some(LayerId(1)),
        active_layer_labels_only: true,
        quickview: Some(PlacementQuickview {
            layer: LayerId(0),
            pins: false,
        }),
        ..Default::default()
    };
    display.hidden_layers.extend([LayerId(0), LayerId(1)]);
    let layout = |display: &BoardDisplay, scale, flipped| {
        index
            .layout(
                Camera {
                    center: at,
                    pixels_per_mm: scale,
                    flipped,
                },
                20_000.0,
                20_000.0,
                display,
                display.label_options,
                &CancellationToken::default(),
            )
            .unwrap()
    };
    assert!(layout(&display, 2.0, false).instances.is_empty());
    for flipped in [false, true] {
        for scale in [20.0, 100.0, 1000.0] {
            let labels = layout(&display, scale, flipped);
            assert_eq!(
                labels.instances.len(),
                6,
                "both overlapping top references survive"
            );
            for (i, g) in labels.instances.iter().enumerate() {
                assert_eq!(
                    g.ids,
                    [80 + (i / 3) as u32, Category::Placement as u32, 0, 0]
                );
                assert!((f64::from(g.xywh[3]) - 1.3).abs() < 1e-5);
                assert_eq!(g.rotation[2], if flipped { -1.0 } else { 1.0 });
                assert!(
                    g.rotation[0] >= 0.0,
                    "reference remains readable after flip"
                );
            }
            // World-space size stays fixed, so the on-screen text has no small pixel cap.
            if scale == 1000.0 {
                assert!(f64::from(labels.instances[0].xywh[3]) * scale > 1000.0);
            }
        }
    }
    display.quickview = Some(PlacementQuickview {
        layer: LayerId(1),
        pins: false,
    });
    let bottom = layout(&display, 100.0, true);
    assert_eq!(bottom.instances.len(), 3);
    assert!(
        bottom
            .instances
            .iter()
            .all(|g| g.ids[0] == 82 && g.ids[2] == 1)
    );
    display.quickview = None;
    assert!(layout(&display, 100.0, false).instances.is_empty());
}

#[test]
fn near_square_placement_prefers_horizontal_but_narrow_body_keeps_vertical() {
    let angle = std::f64::consts::FRAC_PI_2;
    let (_, square) = placement_label_fit(17.0, 16.25, angle, 3.0);
    assert!(
        square.sin().abs() < 1e-10,
        "J3302's almost square body keeps a horizontal name"
    );
    let (_, narrow) = placement_label_fit(10.4, 8.02, angle, 3.0);
    assert!(
        narrow.cos().abs() < 1e-10,
        "J3301's narrower body retains its vertical name"
    );
    let (_, rotated) = placement_label_fit(6.0, 2.0, 0.6, 3.0);
    assert!((rotated - 0.6).abs() < 1e-10);
}

#[test]
fn imported_vertical_angles_do_not_reverse_the_reading_direction_at_one_ulp() {
    let source = 90_000_f64 * std::f64::consts::TAU / 360_000.0;
    for angle in [source, std::f64::consts::FRAC_PI_2, source - 1e-15] {
        assert_eq!(readable(angle), std::f64::consts::FRAC_PI_2);
        assert_eq!(readable(-angle), -std::f64::consts::FRAC_PI_2);
    }
    assert_eq!(readable(std::f64::consts::PI), 0.0);
    assert!((readable(2.0) - (2.0 - std::f64::consts::PI)).abs() < 1e-12);
}
