//! The same image contract runs against D3D11 and wgpu, including real depth and OIT.
use super::*;
use crate::scene::board3d::PreviewStackup;
use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    geometry3d::solid::SolidLimits,
    interaction::camera3d::Camera3d,
    model::{
        BoardScene, Bounds as BoardBounds, Layer, LayerFunction, LayerId, NetId, ObjectId, Point,
        Zone, ZoneKind,
    },
    task::CancellationToken,
};

pub(crate) fn verify(
    mut render: impl FnMut(&mut Option<SolidPipeline>, &SolidFrame, bool, f32) -> Vec<u8>,
) {
    let cancel = CancellationToken::default();
    let square = |size: f64| {
        vec![
            Point::new(-size, -size),
            Point::new(size, -size),
            Point::new(size, size),
            Point::new(-size, size),
        ]
    };
    let layers = (0..2)
        .map(|i| Layer {
            id: LayerId(i),
            name: format!("L{i}"),
            function: LayerFunction::Conductor,
            color: "#ffffff".into(),
            source_flags: None,
        })
        .collect();
    let mut zones = Vec::new();
    for i in 0..2 {
        let rings = if i == 0 {
            vec![square(5.0), square(1.0)]
        } else {
            vec![square(5.0)]
        };
        zones.push(Zone {
            id: ObjectId(10 + i),
            layer: LayerId(i),
            net: NetId(1),
            kind: ZoneKind::Static,
            paths: vec![],
            mesh: CopperMesh::build(&rings, &[], &MeshLimits::default(), &cancel).unwrap(),
        });
    }
    let board = BoardScene {
        layers,
        special_layers: vec![],
        nets: Default::default(),
        segments: vec![],
        pins: vec![],
        components: vec![],
        vias: vec![],
        zones,
        outline: vec![],
        texts: vec![],
        drawing_layers: vec![],
        drawings: vec![],
        bounds: BoardBounds {
            min: Point::new(-5.0, -5.0),
            max: Point::new(5.0, 5.0),
        },
        diagnostics: vec![],
    };
    let scene = Arc::new(
        PreparedBoard3d::preview(
            &board,
            PreviewStackup::default(),
            SolidLimits::default(),
            &cancel,
        )
        .unwrap(),
    );

    let mut camera = Camera3d::default();
    assert!(camera.fit(scene.bounds, [128.0; 2], 8.0));
    let mut frame = SolidFrame {
        scene,
        camera,
        materials: Arc::new(vec![[0.0; 4], [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]]),
        z_scale: 1.0,
    };
    let mut pipeline = None;
    let sample = |image: &[u8], x: usize, y: usize| -> [u8; 4] {
        image[(y * 128 + x) * 4..][..4].try_into().unwrap()
    };
    let opaque = render(&mut pipeline, &frame, false, 1.0);
    let outer = sample(&opaque, 90, 64);
    let hole = sample(&opaque, 64, 64);
    assert!(outer[0] > 180 && outer[2] < 10, "top depth: {outer:?}");
    assert!(
        hole[2] > 180 && hole[0] < 10,
        "hole exposes lower copper: {hole:?}"
    );
    assert_eq!(
        opaque,
        render(&mut pipeline, &frame, false, 1.0),
        "cached redraw"
    );
    assert_eq!(pipeline.as_ref().unwrap().uploaded_this_frame, 0);
    frame.materials = Arc::new(vec![[0.0; 4], [1.0, 0.0, 0.0, 0.3], [0.0, 0.0, 1.0, 0.3]]);
    let transparent = render(&mut pipeline, &frame, false, 1.0);
    pipeline.as_mut().unwrap().transparent_batches.reverse();
    let reverse = render(&mut pipeline, &frame, false, 1.0);
    assert!(
        transparent
            .iter()
            .zip(&reverse)
            .all(|(a, b)| a.abs_diff(*b) <= 1),
        "OIT must be independent of page order"
    );
    let blended = sample(&transparent, 90, 64);
    assert!(
        blended[0] > 20 && blended[1] > 20 && blended[2] > 0,
        "both transparent layers and background: {blended:?}"
    );
    frame.materials = Arc::new(vec![[0.0; 4], [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 0.3]]);
    let mixed = render(&mut pipeline, &frame, false, 1.0);
    assert_eq!(
        sample(&mixed, 90, 64),
        outer,
        "opaque front rejects transparent rear"
    );
    frame.materials = Arc::new(vec![[0.0; 4], [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]]);
    let clipped = render(&mut pipeline, &frame, true, 1.0);
    assert_eq!(sample(&clipped, 90, 64), [0, 255, 0, 255], "content mask");
    assert_eq!(sample(&clipped, 40, 64), sample(&opaque, 40, 64));
    let faded = render(&mut pipeline, &frame, false, 0.5);
    let faded = sample(&faded, 90, 64);
    assert!(
        faded[0] > 80 && (126..=129).contains(&faded[1]),
        "window opacity: {faded:?}"
    );
    pipeline.as_mut().unwrap().visible_mask.fill(false);
    pipeline.as_mut().unwrap().evict_to(0);
    assert_eq!(pipeline.as_ref().unwrap().resident_bytes, 0);
    assert_eq!(
        render(&mut pipeline, &frame, false, 1.0),
        opaque,
        "eviction recovery"
    );
    pipeline = None;
    assert_eq!(
        render(&mut pipeline, &frame, false, 1.0),
        opaque,
        "reset recovery"
    );
    frame.materials = Arc::new(vec![[0.0; 4]; 3]);
    let hidden = render(&mut pipeline, &frame, false, 1.0);
    assert!(
        hidden
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 255, 0, 255]),
        "hidden materials leave background intact"
    );
    // Via drill metadata must not open the center of the solid 3D preview.
    let mut drilled = board;
    for zone in &mut drilled.zones {
        zone.mesh =
            CopperMesh::build(&[square(5.0)], &[], &MeshLimits::default(), &cancel).unwrap();
    }
    drilled.vias.push(pomelo_core::model::Via {
        id: ObjectId(500),
        net: NetId(1),
        at: Point::default(),
        drill: 2.0,
        drill_shape: pomelo_core::model::DrillShape {
            custom: None,
            width: 2.0,
            height: 2.0,
            plated: true,
            rectangular: false,
        },
        padstack: ObjectId(501),
        padstack_name: String::new(),
        start_layer: Some(LayerId(0)),
        end_layer: Some(LayerId(1)),
        pads: vec![
            pomelo_core::model::Pad::circle(LayerId(0), 3.0),
            pomelo_core::model::Pad::circle(LayerId(1), 3.0),
        ]
        .into(),
        backdrill: None,
        stackup_region: None,
        angle: 0.0,
        mirrored: false,
        finger: None,
    });
    let scene = Arc::new(
        PreparedBoard3d::preview(
            &drilled,
            PreviewStackup::default(),
            SolidLimits::default(),
            &cancel,
        )
        .unwrap(),
    );
    let materials = Arc::new(
        std::iter::once([0.0; 4])
            .chain(scene.sources.iter().map(|_| [1.0, 0.0, 0.0, 1.0]))
            .collect(),
    );
    frame = SolidFrame {
        scene,
        camera,
        materials,
        z_scale: 1.0,
    };
    pipeline = None;
    let pixels = render(&mut pipeline, &frame, false, 1.0);
    let center = sample(&pixels, 64, 64);
    assert!(
        center[0] > 180 && center[1] == 0 && center[2] == 0,
        "the via center must render solid copper: {center:?}"
    );
    assert!(
        sample(&pixels, 90, 64)[0] > 180,
        "copper outside the drill disappeared"
    );
    // The same welded mesh must have smooth cylindrical sides and flat end caps.
    drilled.zones.clear();
    drilled.vias[0].pads = Vec::new().into();
    drilled.vias[0].drill_shape.width = 0.2;
    drilled.vias[0].drill_shape.height = 0.2;
    let make = || {
        PreparedBoard3d::preview(
            &drilled,
            PreviewStackup {
                board_mm: 0.2,
                ..Default::default()
            },
            SolidLimits::default(),
            &cancel,
        )
        .unwrap()
    };
    let smooth = Arc::new(make());
    let mut flat = make();
    for page in &mut flat.pages {
        for instance in &mut page.instances {
            instance.owner[3] = 0;
        }
    }
    let flat = Arc::new(flat);
    let mut camera = Camera3d::new(
        [0.; 3],
        [
            std::f64::consts::FRAC_1_SQRT_2,
            0.,
            0.,
            std::f64::consts::FRAC_1_SQRT_2,
        ],
        1.,
    )
    .unwrap();
    assert!(camera.fit(smooth.bounds, [128.; 2], 8.));
    let make_frame = |scene: Arc<PreparedBoard3d>, camera| SolidFrame {
        materials: Arc::new(
            std::iter::once([0.; 4])
                .chain(scene.sources.iter().map(|_| [1., 0., 0., 1.]))
                .collect(),
        ),
        scene,
        camera,
        z_scale: 1.,
    };
    let smooth_frame = make_frame(smooth.clone(), camera);
    let flat_frame = make_frame(flat.clone(), camera);
    let smooth_pixels = render(&mut None, &smooth_frame, false, 1.);
    let flat_pixels = render(&mut None, &flat_frame, false, 1.);
    let shades = |image: &[u8]| {
        image[64 * 128 * 4..65 * 128 * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 0 && p[1] == 0 && p[2] == 0)
            .map(|p| p[0])
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert!(
        shades(&smooth_pixels).len() > shades(&flat_pixels).len() + 8,
        "cylinder side still has flat lighting bands: smooth={} flat={}",
        shades(&smooth_pixels).len(),
        shades(&flat_pixels).len()
    );
    let mut lod_frame = make_frame(smooth.clone(), camera);
    let mut lod_pipeline = None;
    assert!(lod_frame.camera.zoom_at([64.; 2], [128.; 2], 0.02));
    render(&mut lod_pipeline, &lod_frame, false, 1.);
    assert!(lod_frame.camera.zoom_at([64.; 2], [128.; 2], 50.));
    render(&mut lod_pipeline, &lod_frame, false, 1.);
    assert_eq!(
        lod_pipeline.as_ref().unwrap().uploaded_this_frame,
        0,
        "zooming circle LOD must reuse uploaded geometry"
    );
    let mut top = Camera3d::default();
    assert!(top.fit(smooth.bounds, [128.; 2], 8.));
    assert_eq!(
        render(&mut None, &make_frame(smooth, top), false, 1.),
        render(&mut None, &make_frame(flat, top), false, 1.),
        "round shading must preserve flat end caps"
    );
    let top_pixels = render(
        &mut None,
        &make_frame(smooth_frame.scene.clone(), top),
        false,
        1.,
    );
    for x in [40usize, 88] {
        for y in [40usize, 88] {
            assert!(
                sample(&top_pixels, x, y)[0] > 100,
                "LOD cap lost a filled quadrant"
            );
        }
    }
    if let Some(output) = std::env::var_os("POMELO_SOLID_ROUND_OUTPUT") {
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        image::RgbaImage::from_raw(128, 128, smooth_pixels)
            .unwrap()
            .save(output.join("smooth.png"))
            .unwrap();
        image::RgbaImage::from_raw(128, 128, flat_pixels)
            .unwrap()
            .save(output.join("flat.png"))
            .unwrap();
    }
}
