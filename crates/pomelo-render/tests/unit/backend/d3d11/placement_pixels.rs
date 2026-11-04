//! Placement coverage clips generated references independently of silk, nets and pin visibility.
use super::compositor_parity::{device, fixture, gpu_context, renderer};
use super::*;
use pomelo_core::{
    display::PlacementQuickview,
    model::{BoardScene, ComponentBody, ComponentPlacement, ComponentRegion},
};
#[test]
#[ignore = "requires Windows hardware D3D11"]
fn hardware_placement_references_keep_source_holes_and_restore_without_geometry_upload() {
    let path = |points: &[(f64, f64)]| {
        points
            .iter()
            .enumerate()
            .map(|(i, &a)| {
                let b = points[(i + 1) % points.len()];
                Segment {
                    id: ObjectId(81),
                    track_id: ObjectId(81),
                    layer: LayerId(0),
                    net: NetId(0),
                    a: Point::new(a.0, a.1),
                    b: Point::new(b.0, b.1),
                    width: 0.0,
                    arc: None,
                    bond_wire: None,
                }
            })
            .collect()
    };
    let scene = Arc::new(BoardScene {
        layers: vec![],
        special_layers: vec![],
        nets: Default::default(),
        segments: vec![],
        pins: vec![],
        vias: vec![],
        zones: vec![],
        outline: vec![],
        texts: vec![],
        drawing_layers: vec![],
        drawings: vec![],
        diagnostics: vec![],
        components: vec![ComponentPlacement {
            id: ObjectId(80),
            source_reference: None,
            reference: "U10".into(),
            at: Point::default(),
            angle: 0.0,
            mirrored: false,
            pins: vec![],
            body: Some(ComponentBody {
                layer: LayerId(0),
                regions: vec![ComponentRegion {
                    source: ObjectId(81),
                    source_layer: LayerId::allegro_drawing(0xfb09),
                    paths: vec![
                        path(&[(-2.0, -1.0), (2.0, -1.0), (2.0, 1.0), (-2.0, 1.0)]),
                        path(&[(-0.35, -0.4), (0.35, -0.4), (0.35, 0.4), (-0.35, 0.4)]),
                    ],
                }],
            }),
        }],
        bounds: Bounds {
            min: Point::new(-6.0, -6.0),
            max: Point::new(6.0, 6.0),
        },
    });
    let cancel = CancellationToken::default();
    let font = Arc::new(crate::text::msdf::MsdfFont::bundled(["U10"], &cancel).unwrap());
    let index = crate::text::msdf::LabelIndex::build(scene.clone(), font, &cancel).unwrap();
    let mut frame = fixture();
    frame.pads = None;
    frame.drills = None;
    frame.traces.tracks =
        Arc::new(PreparedTracks::build(&[], TraceLimits::default(), &cancel).unwrap());
    frame.copper = Arc::new(
        crate::copper::PreparedCopper::build(&[], crate::copper::CopperLimits::default(), &cancel)
            .unwrap(),
    );
    frame.placements = Some(Arc::new(
        crate::scene::placement::prepare(&scene, crate::copper::CopperLimits::default(), &cancel)
            .unwrap(),
    ));
    frame.placement_outlines = Some(Arc::new(
        PreparedTracks::build_placements(&scene, TraceLimits::default(), &cancel).unwrap(),
    ));
    let display = Arc::make_mut(&mut frame.display);
    display.hidden_layers.extend([LayerId(0), LayerId(2)]);
    display.show_texts = false;
    display.quickview = Some(PlacementQuickview {
        layer: LayerId(0),
        pins: false,
    });
    let labels = index
        .layout(
            frame.traces.camera.unwrap(),
            128.0,
            128.0,
            &frame.display,
            frame.display.label_options,
            &cancel,
        )
        .unwrap();
    assert!(!labels.instances.is_empty());
    frame.labels = Some(Arc::new(labels));
    let telemetry = Arc::new(crate::backend::CopperTelemetry::default());
    let (device, context) = device();
    let gpu = gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut renderer = renderer().with_placement_telemetry(telemetry.clone());
    for _ in 0..16 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
    }
    let pixels = target.read(&context);
    let uploaded = telemetry.snapshot().lifetime_uploaded_bytes;
    assert!(uploaded > 0);
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 150 && p[1] > 150 && p[2] > 150)
            .count()
            > 5,
        "dynamic reference ink must be visible"
    );
    for y in 62..66 {
        for x in 62..66 {
            assert_eq!(
                rgb(&pixels, x, y),
                [0; 3],
                "placement holes clip both coverage and reference ink"
            );
        }
    }
    for y in 0..128 {
        for x in 0..128 {
            if !(43..=85).contains(&x) || !(53..=75).contains(&y) {
                assert_eq!(
                    rgb(&pixels, x, y),
                    [0; 3],
                    "reference must remain inside its source boundary"
                );
            }
        }
    }
    Arc::make_mut(&mut frame.display).quickview = None;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert!(
        target
            .read(&context)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3] == [0; 3]),
        "off must immediately hide old reference packets"
    );
    Arc::make_mut(&mut frame.display).quickview = Some(PlacementQuickview {
        layer: LayerId(0),
        pins: false,
    });
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert_eq!(target.read(&context), pixels);
    assert_eq!(telemetry.snapshot().lifetime_uploaded_bytes, uploaded);

    // The body is its own typed hit identity, distinct from a pin with the same ID.
    frame.labels = None;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let plain = target.read(&context);
    frame.traces.highlighted_object = Some((
        pomelo_core::selection::SelectedObject::Pin(ObjectId(80)),
        [1.0; 4],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert!(
        target.read(&context) == plain,
        "pin identity must not highlight an unrelated placement"
    );
    frame.traces.highlighted_object = None;
    frame.traces.highlighted_related_objects = Some((
        Arc::new(
            [pomelo_core::selection::SelectedObject::Placement(ObjectId(
                80,
            ))]
            .into(),
        ),
        [1.0; 4],
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let selected = target.read(&context);
    assert!(
        selected
            .as_chunks::<4>()
            .0
            .iter()
            .zip(plain.as_chunks::<4>().0)
            .filter(|(a, b)| a[0] > b[0].saturating_add(50))
            .count()
            > 20,
        "selected placement must have a visible boundary overlay"
    );
    frame.traces.highlighted_related_objects = None;
    frame.traces.hover_selection = Some((
        pomelo_core::selection::SelectionTarget::Component(ObjectId(80)),
        Arc::new(
            [pomelo_core::selection::SelectedObject::Placement(ObjectId(
                80,
            ))]
            .into(),
        ),
    ));
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    let hovered = target.read(&context);
    assert!(
        hovered
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[1] > p[0].saturating_add(20))
            .count()
            > 20,
        "hover must use the existing green feedback"
    );
    // Existing selection strokes have a wider antialiased edge; the hole's
    // interior remains unfilled in both feedback passes.
    for y in 63..65 {
        for x in 63..65 {
            assert_eq!(rgb(&selected, x, y), [0; 3]);
            assert_eq!(rgb(&hovered, x, y), [0; 3]);
        }
    }
    Arc::make_mut(&mut frame.display).quickview = None;
    target.bind(&context);
    renderer.draw(&gpu, &frame).unwrap();
    assert!(
        target
            .read(&context)
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3] == [0; 3]),
        "hidden placement must not retain a selection or hover overlay"
    );
    assert_eq!(telemetry.snapshot().lifetime_uploaded_bytes, uploaded);
}
