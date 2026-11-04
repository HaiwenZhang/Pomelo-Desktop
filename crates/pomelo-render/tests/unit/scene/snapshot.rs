use super::*;
use crate::text::msdf::{LabelIndex, MsdfFont, PreparedGlyphs};
use pomelo_core::model::{LayerId, NetId, ObjectId};
use std::sync::Arc;

fn pixels(svg: &str) -> Vec<u8> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).unwrap();
    let size = tree.size().to_int_size();
    let mut image = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut image.as_mut(),
    );
    image.data().to_vec()
}

fn text(layer: LayerId) -> pomelo_core::model::BoardText {
    pomelo_core::model::BoardText {
        id: ObjectId(71),
        owner_id: None,
        layer,
        class_id: 6,
        subclass: 0,
        text: "GND中".into(),
        at: Point::new(2.0, 3.0),
        angle: 0.3,
        mirrored: false,
        align: pomelo_core::model::TextAlignment::Left,
        font_index: 0,
        width: 1.2,
        height: 2.0,
        spacing: 0.0,
        line_spacing: 2.0,
        stroke_width: 0.0,
    }
}

#[test]
fn source_text_export_uses_canvas_glyphs_and_logical_pixel_filters() {
    let mut board = scene(vec![]);
    board.texts = vec![text(LayerId(0))];
    let cancel = CancellationToken::default();
    let font = Arc::new(MsdfFont::bundled(["GND中"], &cancel).unwrap());
    let source = PreparedGlyphs::build(&board.texts, font, 1024 * 1024, &cancel).unwrap();
    let colors = BTreeMap::from([(LayerId(0), [0.0, 1.0, 0.0, 1.0])]);
    let mut display = BoardDisplay {
        show_texts: true,
        ..Default::default()
    };
    let render = |display: &BoardDisplay, ratio: f64, flipped| {
        let mut r = request(&board, display, &colors);
        r.width = (200.0 * ratio) as u32;
        r.height = (100.0 * ratio) as u32;
        r.pixel_ratio = ratio;
        r.flipped = flipped;
        r.scale_unit = None;
        r.annotations.source_glyphs = Some(&source);
        let svg = svg(&r, &cancel).unwrap();
        assert!(!svg.contains("<text"), "no dependency on installed fonts");
        pixels(&svg)
    };
    for flipped in [false, true] {
        let image = render(&display, 1.0, flipped);
        assert!(
            image
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[1] > 100)
                .count()
                > 20
        );
        assert!(
            image
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[0] == 0 && p[2] == 0)
        );
    }
    display.object_filter.minimum_pixels = 21;
    for ratio in [1.0, 2.0] {
        assert!(
            render(&display, ratio, false)
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| p[..3] == [0; 3]),
            "2mm text at 10 logical px/mm stays filtered at DPI {ratio}"
        );
    }
    display.object_filter.minimum_pixels = 0;
    display.show_texts = false;
    assert!(
        render(&display, 1.0, false)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3] == [0; 3])
    );
    display.show_texts = true;
    display.hidden_layers.insert(LayerId(0));
    assert!(
        render(&display, 1.0, false)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3] == [0; 3])
    );
}

#[test]
fn source_text_and_upper_copper_follow_the_canvas_global_display_rank() {
    let mut upper = segment(1, (1.0, 4.0), (9.0, 4.0));
    upper.width = 4.0;
    let mut board = scene(vec![upper]);
    let mut t = text(LayerId(1));
    t.text = "GND".into();
    t.angle = 0.0;
    t.at = Point::new(3.0, 3.0);
    t.height = 1.0;
    board.texts = vec![t];
    let cancel = CancellationToken::default();
    let font = Arc::new(MsdfFont::bundled(["GND"], &cancel).unwrap());
    let source = PreparedGlyphs::build(&board.texts, font, 1024 * 1024, &cancel).unwrap();
    let colors = BTreeMap::from([(LayerId(0), [1.0, 0.0, 0.0, 1.0]), (LayerId(1), [1.0; 4])]);
    let display = BoardDisplay {
        show_texts: true,
        ..Default::default()
    };
    let render = |display: &BoardDisplay, with_text: bool| {
        let mut r = request(&board, display, &colors);
        r.scale_unit = None;
        r.layer_order = &[LayerId(0), LayerId(1)];
        r.annotations.source_glyphs = with_text.then_some(&source);
        pixels(&svg(&r, &cancel).unwrap())
    };
    assert!(
        render(&display, true) == render(&display, false),
        "front copper hides text from a lower layer regardless of iteration order"
    );
    let mut active = display;
    active.active_layer = Some(LayerId(1));
    assert!(
        render(&active, true) != render(&active, false),
        "active copper-layer text is promoted as on the canvas"
    );
}

#[test]
fn copper_names_keep_shape_opacity_and_clip_overlapping_voids_for_paths_and_meshes() {
    use pomelo_core::{
        copper::{CopperMesh, MeshLimits},
        model::{Zone, ZoneKind},
    };
    let cancel = CancellationToken::default();
    let rect = |x0, y0, x1, y1| {
        vec![
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ]
    };
    let rings = vec![
        rect(1.0, 1.0, 9.0, 9.0),
        rect(4.0, 4.0, 6.0, 6.0),
        rect(5.0, 4.0, 7.0, 6.0),
    ];
    let paths: Vec<Vec<Segment>> = rings
        .iter()
        .map(|ring| {
            (0..ring.len())
                .map(|i| {
                    segment(
                        9,
                        (ring[i].x, ring[i].y),
                        (ring[(i + 1) % ring.len()].x, ring[(i + 1) % ring.len()].y),
                    )
                })
                .collect()
        })
        .collect();
    let mut board = scene(vec![]);
    board.nets.insert(NetId(1), "GND".into());
    board.zones = vec![Zone {
        id: ObjectId(9),
        layer: LayerId(0),
        net: NetId(1),
        kind: ZoneKind::Dynamic,
        paths: paths.clone(),
        mesh: CopperMesh::build(&rings, &[], &MeshLimits::default(), &cancel).unwrap(),
    }];
    let colors = BTreeMap::from([(LayerId(0), [1.0, 0.0, 0.0, 1.0])]);
    let mut display = BoardDisplay {
        copper_opacity: 0.5,
        ..Default::default()
    };
    let mut board = Arc::new(board);
    for mesh_only in [false, true] {
        if mesh_only {
            Arc::get_mut(&mut board).unwrap().zones[0].paths.clear();
        }
        let shared = Arc::clone(&board);
        let font = Arc::new(MsdfFont::bundled(["GND"], &cancel).unwrap());
        let index = LabelIndex::build(shared, font, &cancel).unwrap();
        let render = |display: &BoardDisplay, names: bool| {
            let mut r = request(&board, display, &colors);
            r.width = 256;
            r.height = 256;
            r.scale_unit = None;
            r.annotations.label_index = names.then_some(&index);
            pixels(&svg(&r, &cancel).unwrap())
        };
        let full = render(&display, true);
        assert!(
            full != render(&display, false),
            "copper names must actually appear"
        );
        for y in 112..144 {
            for x in 132..150 {
                assert_eq!(
                    &full[(y * 256 + x) * 4..(y * 256 + x) * 4 + 3],
                    &[0; 3],
                    "overlapping holes must stay empty for material and name ink"
                );
            }
        }
        display.global_opacity = 0.0;
        assert!(
            render(&display, true) == full,
            "shape names retain shape alpha independently of global alpha"
        );
        display.global_opacity = 1.0;
    }
}

#[test]
fn automatic_track_names_recompute_zoom_in_logical_pixels() {
    let mut line = segment(1, (1.0, 5.0), (9.0, 5.0));
    line.width = 1.0;
    let mut board = scene(vec![line]);
    board.nets.insert(NetId(1), "GND".into());
    let cancel = CancellationToken::default();
    let font = Arc::new(MsdfFont::bundled(["GND"], &cancel).unwrap());
    let board = Arc::new(board);
    let index = LabelIndex::build(Arc::clone(&board), font, &cancel).unwrap();
    let colors = BTreeMap::from([(LayerId(0), [1.0, 0.0, 0.0, 1.0])]);
    let display = BoardDisplay::default();
    let render = |edge, ratio| {
        let mut r = request(&board, &display, &colors);
        r.width = edge;
        r.height = edge;
        r.pixel_ratio = ratio;
        r.scale_unit = None;
        r.annotations.label_index = Some(&index);
        svg(&r, &cancel).unwrap()
    };
    assert!(!render(100, 1.0).contains("<image"));
    assert!(!render(200, 2.0).contains("<image"));
    assert!(render(200, 1.0).contains("<image"));
}

fn segment(id: u32, a: (f64, f64), b: (f64, f64)) -> Segment {
    Segment {
        id: ObjectId(id),
        track_id: ObjectId(id),
        layer: LayerId(0),
        net: NetId(1),
        a: Point::new(a.0, a.1),
        b: Point::new(b.0, b.1),
        width: 0.2,
        arc: None,
        bond_wire: None,
    }
}

fn scene(segments: Vec<Segment>) -> BoardScene {
    BoardScene {
        layers: vec![],
        special_layers: vec![],
        nets: Default::default(),
        segments,
        pins: vec![],
        components: vec![],
        vias: vec![],
        zones: vec![],
        outline: vec![],
        texts: vec![],
        drawing_layers: vec![],
        drawings: vec![],
        bounds: Bounds {
            min: Point::default(),
            max: Point::new(10.0, 10.0),
        },
        diagnostics: vec![],
    }
}

fn request<'a>(
    scene: &'a BoardScene,
    display: &'a BoardDisplay,
    colors: &'a BTreeMap<LayerId, [f32; 4]>,
) -> SnapshotRequest<'a> {
    SnapshotRequest {
        scene,
        display,
        area: scene.bounds,
        width: 200,
        height: 100,
        pixel_ratio: 1.0,
        annotations: AnnotationSources::default(),
        flipped: false,
        layer_order: &[LayerId(0)],
        colors,
        fallback_color: [0.5, 0.5, 0.5, 1.0],
        drill_color: [0.4, 0.4, 0.4, 1.0],
        outline_color: [1.0, 1.0, 1.0, 1.0],
        background: [0.0, 0.0, 0.0, 1.0],
        mark_color: [1.0, 1.0, 1.0, 1.0],
        object_colors: None,
        marks: Marks::default(),
        scale_unit: Some(LengthUnit::Millimeters),
        max_elements: MAX_ELEMENTS,
    }
}

#[test]
fn board_space_is_fitted_and_y_points_up() {
    let projection = Projection::new(
        Bounds {
            min: Point::default(),
            max: Point::new(10.0, 10.0),
        },
        200,
        100,
        false,
    )
    .unwrap();
    // The 10 mm square fits the 100 px height: 10 px/mm centred horizontally.
    assert_eq!(projection.transform(), "matrix(10 0 0 -10 50 100)");
    let flipped = Projection::new(
        Bounds {
            min: Point::default(),
            max: Point::new(10.0, 10.0),
        },
        200,
        100,
        true,
    )
    .unwrap();
    assert_eq!(flipped.transform(), "matrix(-10 0 0 -10 150 100)");
}

#[test]
fn visible_tracks_hidden_layers_and_colors_follow_display_state() {
    let board = scene(vec![
        segment(1, (1.0, 1.0), (9.0, 1.0)),
        segment(2, (40.0, 40.0), (41.0, 41.0)),
    ]);
    let colors = BTreeMap::from([(LayerId(0), [1.0, 0.0, 0.0, 1.0])]);
    let display = BoardDisplay::default();
    let output = svg(
        &request(&board, &display, &colors),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(output.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\""));
    assert!(output.contains("M1 1L9 1"));
    // Geometry outside the output range is culled.
    assert!(!output.contains("M40 40"));
    assert!(output.contains("stroke=\"#ff0000\""));
    let mut hidden = BoardDisplay::default();
    hidden.hidden_layers.insert(LayerId(0));
    let output = svg(
        &request(&board, &hidden, &colors),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(!output.contains("M1 1L9 1"));
}

#[test]
fn highlight_colors_and_marks_are_drawn() {
    let board = scene(vec![segment(1, (1.0, 1.0), (9.0, 1.0))]);
    let colors = BTreeMap::new();
    let display = BoardDisplay::default();
    let object_colors = BTreeMap::from([(SelectedObject::Segment(ObjectId(1)), [0.0, 1.0, 0.0])]);
    let selection = BTreeSet::from([SelectedObject::Segment(ObjectId(1))]);
    let mut request = request(&board, &display, &colors);
    request.object_colors = Some(&object_colors);
    request.marks = Marks {
        selection: Some(&selection),
        measurements: vec![vec![Point::new(0.0, 0.0), Point::new(3.0, 4.0)]],
        clearance: Some((Point::new(1.0, 2.0), Point::new(1.0, 3.0))),
    };
    let output = svg(&request, &CancellationToken::default()).unwrap();
    assert!(output.contains("stroke=\"#00ff00\""));
    assert!(output.contains("<g opacity=\"0.55\"><path d=\"M1 1L9 1\" stroke=\"#ffffff\""));
    assert!(output.contains("M0 0L3 4"));
    assert!(output.contains("stroke-dasharray"));
}

#[test]
fn element_budget_size_and_cancellation_are_enforced() {
    let board = scene(vec![
        segment(1, (1.0, 1.0), (9.0, 1.0)),
        segment(2, (1.0, 2.0), (9.0, 2.0)),
    ]);
    let colors = BTreeMap::new();
    let display = BoardDisplay::default();
    let mut limited = request(&board, &display, &colors);
    limited.max_elements = 1;
    assert_eq!(
        svg(&limited, &CancellationToken::default()),
        Err(SnapshotError::TooLarge)
    );
    let mut oversized = request(&board, &display, &colors);
    oversized.width = MAX_EDGE + 1;
    assert_eq!(
        svg(&oversized, &CancellationToken::default()),
        Err(SnapshotError::TooLarge)
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        svg(&request(&board, &display, &colors), &cancel),
        Err(SnapshotError::Cancelled)
    );
}

#[test]
fn arcs_use_svg_arc_commands_and_full_circles_split() {
    let mut arc = segment(1, (1.0, 0.0), (-1.0, 0.0));
    arc.arc = Some(pomelo_core::model::Arc {
        center: Point::default(),
        radius: 1.0,
        start: 0.0,
        sweep: std::f64::consts::PI,
    });
    assert_eq!(segment_path(&arc), "M1 0A1 1 0 0 1 -1 0");
    let mut circle = segment(2, (1.0, 0.0), (1.0, 0.0));
    circle.arc = Some(pomelo_core::model::Arc {
        center: Point::default(),
        radius: 1.0,
        start: 0.0,
        sweep: -std::f64::consts::TAU,
    });
    assert_eq!(segment_path(&circle), "M1 0A1 1 0 1 0 -1 0A1 1 0 1 0 1 0");
}

#[test]
fn scale_labels_use_unit_symbols_and_supported_glyphs() {
    assert_eq!(scale_label(2.0, LengthUnit::Millimeters), "2 mm");
    assert_eq!(scale_label(0.0254 * 50.0, LengthUnit::Mils), "50 mil");
    assert_eq!(scale_label(0.5, LengthUnit::Millimeters), "0.5 mm");
    for c in "0123456789.-mil".chars() {
        assert!(!glyph(c).is_empty(), "{c}");
    }
}

#[test]
fn grid_export_uses_display_color_below_board_geometry() {
    let scene = scene(vec![segment(1, (1.0, 1.0), (4.0, 4.0))]);
    let mut display = BoardDisplay::default();
    display.grid.visible = true;
    display.appearance.set_color(
        pomelo_core::appearance::ColorTarget::Grid,
        Some(pomelo_core::appearance::RgbColor([12, 34, 56])),
    );
    let colors = BTreeMap::new();
    let image = svg(
        &request(&scene, &display, &colors),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(image.contains("<circle"));
    assert!(image.contains("#0c2238"));
    assert!(image.find("<circle").unwrap() < image.find("<path").unwrap());
}

#[test]
fn snapshot_net_colors_override_automatic_materials_but_object_colors_take_precedence() {
    use pomelo_core::appearance::{ColorTarget, RgbColor, net::NetColorKind};
    let board = scene(vec![segment(1, (1.0, 1.0), (9.0, 1.0))]);
    let colors = BTreeMap::from([(LayerId(0), [1.0, 0.0, 0.0, 1.0])]);
    let mut display = BoardDisplay::default();
    display.appearance.set_color(
        ColorTarget::Net(NetId(1), NetColorKind::All),
        Some(RgbColor([0, 255, 0])),
    );
    let render = |display: &BoardDisplay| {
        svg(
            &request(&board, display, &colors),
            &CancellationToken::default(),
        )
        .unwrap()
    };
    assert!(render(&display).contains("stroke=\"#00ff00\""));
    display.appearance.set_color(
        ColorTarget::Net(NetId(1), NetColorKind::Traces),
        Some(RgbColor([0, 0, 255])),
    );
    assert!(render(&display).contains("stroke=\"#0000ff\""));
    let overrides = BTreeMap::from([(SelectedObject::Segment(ObjectId(1)), [1.0, 1.0, 0.0])]);
    let mut request = request(&board, &display, &colors);
    request.object_colors = Some(&overrides);
    assert!(
        svg(&request, &CancellationToken::default())
            .unwrap()
            .contains("stroke=\"#ffff00\"")
    );
    display.appearance.hide_net_colors = true;
    assert!(render(&display).contains("stroke=\"#ff0000\""));
}
