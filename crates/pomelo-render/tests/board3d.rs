use pomelo_core::{
    display::BoardDisplay,
    geometry3d::solid::SolidLimits,
    interaction::camera3d::PickRay3d,
    model::{
        BoardScene, Bounds, DrillShape, Layer, LayerFunction, LayerId, NetId, ObjectId, Pad, Pin,
        Point, Segment, Via,
    },
    selection::SelectedObject,
    task::CancellationToken,
};
use pomelo_render::scene::board3d::{PreparedBoard3d, PreviewStackup};

#[test]
fn solid_via_pads_share_meshes_while_their_body_profiles_remain_distinct() {
    let mut input = source();
    input.segments.clear();
    input.pins.clear();
    input.vias.truncate(2);
    input.vias[1].drill_shape.rectangular = true;
    let solid = PreparedBoard3d::preview(
        &input,
        PreviewStackup::default(),
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let meshes = |id| {
        solid
            .sources
            .iter()
            .filter(|s| s.object == SelectedObject::Via(id))
            .map(|s| solid.parts[s.parts.start].mesh)
            .collect::<Vec<_>>()
    };
    let round = meshes(input.vias[0].id);
    let rectangular = meshes(input.vias[1].id);
    assert_eq!(round.len(), 3);
    assert_eq!(rectangular.len(), 3);
    assert_eq!(
        &round[..2],
        &rectangular[..2],
        "via pads no longer contain drill cutouts"
    );
    assert_ne!(
        round[2], rectangular[2],
        "solid bodies retain their source profile"
    );
}

#[test]
fn board_preparation_keeps_its_shared_memory_reservation_until_the_last_owner_drops() {
    let budget = pomelo_core::memory::MemoryBudget::new(0);
    let mut memory = pomelo_render::preparation_memory::PreparationMemory::new(budget.clone());
    let board = std::sync::Arc::new(
        memory
            .prepare_board3d(
                &source(),
                PreviewStackup::default(),
                &CancellationToken::default(),
            )
            .unwrap(),
    );
    assert_eq!(budget.used(), board.allocation_bytes());
    let frame = board.clone();
    drop(board);
    assert_eq!(budget.used(), frame.allocation_bytes());
    drop(frame);
    assert_eq!(budget.used(), 0);
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(
        memory
            .prepare_board3d(&source(), PreviewStackup::default(), &cancelled)
            .is_err()
    );
    assert_eq!(budget.used(), 0);
}

#[test]
fn solid_gpu_mesh_reuses_cap_and_wall_positions() {
    let board = PreparedBoard3d::preview(
        &source(),
        PreviewStackup::default(),
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    for batch in &board.pages {
        let mesh = &board.meshes[batch.mesh];
        let mut unique = std::collections::BTreeSet::new();
        let mut used = std::collections::BTreeSet::new();
        for &i in &mesh.indices {
            used.insert(i);
            unique.insert(mesh.vertices[i as usize].position.map(f32::to_bits));
        }
        assert_eq!(
            used.len(),
            unique.len(),
            "cap/wall copies must share GPU vertices"
        );
    }
}

fn source() -> BoardScene {
    let layers = (0..2)
        .map(|i| Layer {
            id: LayerId(i),
            name: format!("L{i}"),
            function: LayerFunction::Conductor,
            color: "#cc8800".into(),
            source_flags: None,
        })
        .collect();
    let pins = (0..100)
        .map(|i| Pin {
            id: ObjectId(i + 2),
            owner_id: ObjectId(1000),
            net: NetId(1),
            name: String::new(),
            reference: String::new(),
            at: Point::new(i as f64 * 0.1, 2.0),
            angle: 0.0,
            mirrored: false,
            drill: 0.0,
            drill_shape: DrillShape {
                custom: None,
                rectangular: false,
                width: 0.0,
                height: 0.0,
                plated: false,
            },
            pads: vec![Pad::circle(LayerId(0), 0.5)],
            stackup_region: None,
            die: None,
        })
        .collect();
    let pads: std::sync::Arc<[Pad]> =
        vec![Pad::circle(LayerId(0), 0.5), Pad::circle(LayerId(1), 0.5)].into();
    let vias = (0..10)
        .map(|i| Via {
            id: ObjectId(200 + i),
            net: NetId(1),
            at: Point::new(i as f64, 5.0),
            drill: 0.2,
            drill_shape: DrillShape {
                custom: None,
                rectangular: false,
                width: 0.2,
                height: 0.2,
                plated: true,
            },
            padstack: ObjectId(900),
            padstack_name: String::new(),
            start_layer: Some(LayerId(0)),
            end_layer: Some(LayerId(1)),
            pads: pads.clone(),
            backdrill: None,
            stackup_region: None,
            angle: 0.0,
            mirrored: false,
            finger: None,
        })
        .collect();
    BoardScene {
        layers,
        special_layers: vec![],
        nets: Default::default(),
        segments: vec![Segment {
            id: ObjectId(1),
            track_id: ObjectId(1),
            layer: LayerId(0),
            net: NetId(1),
            a: Point::default(),
            b: Point::new(5.0, 0.0),
            width: 0.2,
            arc: None,
            bond_wire: None,
        }],
        pins,
        components: vec![],
        vias,
        zones: vec![],
        outline: vec![],
        texts: vec![],
        drawing_layers: vec![],
        drawings: vec![],
        bounds: Bounds {
            min: Point::new(-1.0, -1.0),
            max: Point::new(11.0, 6.0),
        },
        diagnostics: vec![],
    }
}

fn square_zone(id: u32, x: f64, size: f64, hole: bool) -> pomelo_core::model::Zone {
    let ring = |x: f64, y: f64, s: f64| {
        vec![
            Point::new(x, y),
            Point::new(x + s, y),
            Point::new(x + s, y + s),
            Point::new(x, y + s),
        ]
    };
    let mut rings = vec![ring(x, 0.0, size)];
    if hole {
        rings.push(ring(x + size * 0.25, size * 0.25, size * 0.5));
    }
    pomelo_core::model::Zone {
        id: ObjectId(id),
        layer: LayerId(0),
        net: NetId(1),
        kind: pomelo_core::model::ZoneKind::Static,
        paths: vec![],
        mesh: pomelo_core::copper::CopperMesh::build(
            &rings,
            &[],
            &pomelo_core::copper::MeshLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    }
}

#[test]
fn all_via_profiles_are_solid_and_do_not_cut_zones_or_traces() {
    for profile in 0..4 {
        let rectangular = profile == 1;
        let mut input = source();
        input.pins.clear();
        input.vias.truncate(1);
        input.vias[0].at = Point::new(1.5, 1.5);
        input.vias[0].drill_shape = DrillShape {
            custom: None,
            width: 0.4,
            height: if profile == 2 { 0.2 } else { 0.4 },
            plated: true,
            rectangular,
        };
        if profile == 3 {
            input.vias[0].drill_shape.custom =
                Some(std::sync::Arc::new(pomelo_core::model::CustomPadGeometry {
                    contours: vec![vec![
                        Point::new(-0.1, 0.2),
                        Point::new(0.1, 0.2),
                        Point::new(0.2, 0.1),
                        Point::new(0.2, -0.1),
                        Point::new(0.1, -0.2),
                        Point::new(-0.1, -0.2),
                        Point::new(-0.2, -0.1),
                        Point::new(-0.2, 0.1),
                    ]],
                    paths: vec![],
                }));
        }
        input.segments[0].a = Point::new(0.0, 1.5);
        input.segments[0].b = Point::new(3.0, 1.5);
        input.zones = vec![
            square_zone(501, 0.0, 3.0, false),
            square_zone(502, 0.0, 3.0, false),
        ];
        input.zones[1].layer = LayerId(1);
        let solid = PreparedBoard3d::preview(
            &input,
            PreviewStackup::default(),
            SolidLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        let materials = solid.materials(&input, &BoardDisplay::default(), false, None, None);
        let ray = |x, y| PickRay3d {
            origin: [x, y, 10.0],
            direction: [0.0, 0.0, -1.0],
            length: 20.0,
        };
        let mut zones_only = materials.clone();
        let mut traces_only = materials.clone();
        for (i, source) in solid.sources.iter().enumerate() {
            if !matches!(source.object, SelectedObject::Zone(_)) {
                zones_only[i + 1][3] = 0.0;
            }
            if !matches!(source.object, SelectedObject::Segment(_)) {
                traces_only[i + 1][3] = 0.0;
            }
        }
        assert!(
            solid
                .pick(ray(1.5, 1.5), &zones_only, Default::default())
                .is_some(),
            "via holes must not cut copper zones"
        );
        assert_eq!(
            solid.pick(ray(1.5, 1.5), &traces_only, Default::default()),
            Some(SelectedObject::Segment(ObjectId(1))),
            "via holes must not cut traces"
        );
        assert!(
            solid
                .pick(ray(1.5, 1.5), &materials, Default::default())
                .is_some(),
            "every via must have a solid center in the preview"
        );
        assert_eq!(
            solid.pick(
                PickRay3d {
                    origin: [2.0, 1.5, 0.0],
                    direction: [-1.0, 0.0, 0.0],
                    length: 1.0
                },
                &materials,
                Default::default()
            ),
            Some(SelectedObject::Via(input.vias[0].id))
        );
        if rectangular {
            assert!(
                solid
                    .pick(ray(1.68, 1.68), &materials, Default::default())
                    .is_some(),
                "rectangular vias must also have a solid center"
            );
        }
        input.vias[0].end_layer = Some(LayerId(0));
        let blind = PreparedBoard3d::preview(
            &input,
            PreviewStackup::default(),
            SolidLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        let materials = blind.materials(&input, &BoardDisplay::default(), false, None, None);
        assert_eq!(
            blind.pick(ray(1.5, 1.5), &materials, Default::default()),
            Some(SelectedObject::Segment(ObjectId(1))),
            "blind vias must also leave the trace intact"
        );
    }
}

#[test]
fn pin_holes_remain_cut_out_of_the_3d_preview() {
    let mut input = source();
    input.vias.clear();
    input.pins.truncate(1);
    input.pins[0].at = Point::new(1.5, 1.5);
    input.pins[0].pads.clear();
    input.pins[0].drill_shape.width = 0.4;
    input.pins[0].drill_shape.height = 0.4;
    input.segments.clear();
    input.zones = vec![square_zone(501, 0.0, 3.0, false)];
    let solid = PreparedBoard3d::preview(
        &input,
        PreviewStackup::default(),
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let materials = solid.materials(&input, &BoardDisplay::default(), false, None, None);
    assert!(
        solid
            .pick(
                PickRay3d {
                    origin: [1.5, 1.5, 10.0],
                    direction: [0.0, 0.0, -1.0],
                    length: 20.0
                },
                &materials,
                Default::default()
            )
            .is_none()
    );
}

#[test]
fn whole_board_can_exceed_page_limits_and_zone_parts_preserve_triangles_and_holes() {
    use pomelo_render::scene::solid::PreparedSolid;
    let mut input = source();
    input.pins.clear();
    input.vias.clear();
    input.segments.clear();
    input.zones = (0..200)
        .map(|i| square_zone(i + 1, f64::from(i) * 2.0, 1.0, false))
        .collect();
    input.bounds.max = Point::new(401.0, 21.0);
    let limits = SolidLimits {
        max_vertices: 24,
        max_indices: 36,
        ..Default::default()
    };
    let paged = PreparedBoard3d::preview(
        &input,
        PreviewStackup::default(),
        limits,
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(paged.sources.len(), 200);
    assert!(paged.vertex_count() > limits.max_vertices && paged.index_count() > limits.max_indices);
    assert!(
        paged
            .meshes
            .iter()
            .all(|m| m.vertices.len() <= 24 && m.indices.len() <= 36)
    );
    input.zones = vec![square_zone(999, 0.0, 20.0, true)];
    input.bounds.max = Point::new(20.0, 20.0);
    let original = PreparedSolid::prepare_zone(
        &input.zones[0],
        [0.765, 0.8],
        0.005,
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let board = PreparedBoard3d::preview(
        &input,
        PreviewStackup::default(),
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(board.meshes.len() > 1);
    let source = &board.sources[0];
    let triangles = board.parts[source.parts.clone()]
        .iter()
        .map(|p| (p.indices.end - p.indices.start) as usize)
        .sum::<usize>();
    assert_eq!(
        triangles,
        original.mesh.indices.len(),
        "paging must not add boundary walls or drop triangles"
    );
    let colors = board.materials(&input, &BoardDisplay::default(), false, None, None);
    let ray = |x, y| PickRay3d {
        origin: [x, y, 10.0],
        direction: [0.0, 0.0, -1.0],
        length: 20.0,
    };
    assert_eq!(
        board.pick(ray(10.0, 10.0), &colors, Default::default()),
        None
    );
    assert_eq!(
        board.pick(ray(2.0, 2.0), &colors, Default::default()),
        Some(SelectedObject::Zone(ObjectId(999)))
    );
}

#[test]
fn repeated_pads_and_vias_use_shared_geometry_and_keep_source_identity() {
    let source = source();
    let board = PreparedBoard3d::preview(
        &source,
        PreviewStackup::default(),
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(board.warning_count, 0);
    assert_eq!(board.sources.len(), 131);
    assert_eq!(board.instance_count(), 131);
    assert!(
        board.meshes.len() <= 5 && board.pages.len() < board.instance_count() / 2,
        "shared shapes must not create one batch per object"
    );
    assert_eq!(
        board
            .sources
            .iter()
            .filter(|s| matches!(s.object, SelectedObject::Pin(_)))
            .count(),
        100
    );
    assert!((board.bounds.max[2] - 0.8).abs() < 1e-9 && (board.bounds.min[2] + 0.8).abs() < 1e-9);
    let materials = board.materials(&source, &BoardDisplay::default(), false, None, None);
    let ray = PickRay3d {
        origin: [2.0, 0.0, 10.0],
        direction: [0.0, 0.0, -1.0],
        length: 20.0,
    };
    assert_eq!(
        board.pick(ray, &materials, Default::default()),
        Some(SelectedObject::Segment(ObjectId(1)))
    );
    let mut display = BoardDisplay::default();
    display.hidden_layers.insert(LayerId(0));
    assert_eq!(
        board.pick(
            ray,
            &board.materials(&source, &display, false, None, None),
            Default::default()
        ),
        None
    );
    assert_eq!(
        board.pick(ray, &materials, pomelo_core::picking::PickFilter::none()),
        None
    );
    display.global_opacity = 0.0;
    assert_eq!(
        board.pick(
            ray,
            &board.materials(
                &source,
                &display,
                false,
                Some(SelectedObject::Segment(ObjectId(1))),
                None
            ),
            Default::default()
        ),
        None
    );
}

#[test]
fn straight_traces_share_capsules_under_a_bounded_budget_and_pick_stretched_ends() {
    let mut input = source();
    input.pins.clear();
    input.vias.clear();
    input.bounds = Bounds {
        min: Point::new(-1.0, -1.0),
        max: Point::new(2000.0, 1000.0),
    };
    input.segments = (0..5000)
        .map(|i| {
            let a = Point::new((i % 100) as f64 * 20.0, (i / 100) as f64 * 20.0);
            let length = [0.03, 0.75, 8.0][i % 3];
            let angle = (i % 4) as f64 * std::f64::consts::FRAC_PI_4;
            Segment {
                id: ObjectId(i as u32 + 1),
                track_id: ObjectId(i as u32 + 1),
                layer: LayerId(0),
                net: NetId(1),
                a,
                b: Point::new(a.x + length * angle.cos(), a.y + length * angle.sin()),
                width: 0.2,
                arc: None,
                bond_wire: None,
            }
        })
        .collect();
    let board = PreparedBoard3d::preview(
        &input,
        PreviewStackup::default(),
        SolidLimits {
            max_allocation_bytes: 8 * 1024 * 1024,
            ..Default::default()
        },
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(board.warning_count, 0);
    assert_eq!(board.sources.len(), 5000);
    assert_eq!(board.instance_count(), 5000);
    assert!(
        board
            .pages
            .iter()
            .all(|p| p.instances.len() <= pomelo_render::scene::board3d::PAGE_INSTANCES)
    );
    assert_eq!(board.meshes.len(), 1);
    assert!(board.vertex_count() < 256);
    assert!(board.upload_bytes() < 1024 * 1024);
    let materials = board.materials(&input, &BoardDisplay::default(), false, None, None);
    for (i, segment) in input.segments.iter().enumerate().take(3) {
        let delta = Point::new(segment.b.x - segment.a.x, segment.b.y - segment.a.y);
        let length = segment.a.distance(segment.b);
        for (extra, expected) in [
            (0.04, Some(SelectedObject::Segment(segment.id))),
            (0.15, None),
        ] {
            let p = Point::new(
                segment.b.x + delta.x / length * extra,
                segment.b.y + delta.y / length * extra,
            );
            assert_eq!(
                board.pick(
                    PickRay3d {
                        origin: [p.x, p.y, 10.0],
                        direction: [0.0, 0.0, -1.0],
                        length: 20.0
                    },
                    &materials,
                    Default::default()
                ),
                expected,
                "trace {i}, extension {extra}"
            );
        }
    }
}

#[test]
fn paged_frustum_results_match_bruteforce_and_reduce_local_work() {
    use pomelo_core::interaction::camera3d::{Camera3d, CameraView3d};
    let mut input = source();
    input.bounds.max = Point::new(401.0, 21.0);
    input.pins.clear();
    input.vias.clear();
    input.segments.clear();
    input.zones = (0..200)
        .map(|i| square_zone(i + 1, f64::from(i) * 2.0, 1.0, false))
        .collect();
    let board = PreparedBoard3d::preview(
        &input,
        PreviewStackup::default(),
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let mut visible = Vec::new();
    for direction in [
        CameraView3d::Top,
        CameraView3d::Bottom,
        CameraView3d::Isometric,
    ] {
        let mut camera = Camera3d::new([10.0, 0.5, 0.0], [0.0, 0.0, 0.0, 1.0], 10.0).unwrap();
        camera.set_view(direction);
        board
            .visible_pages(camera, [100.0; 2], 5.0, &mut visible)
            .unwrap();
        assert!(visible.len() < board.pages.len() / 2);
        let expected: std::collections::BTreeSet<_> = board
            .pages
            .iter()
            .enumerate()
            .filter_map(|(i, page)| {
                let mut b = page.bounds;
                b.min[2] *= 5.0;
                b.max[2] *= 5.0;
                camera.intersects(b, [100.0; 2]).then_some(i)
            })
            .collect();
        assert_eq!(
            visible
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>(),
            expected
        );
    }
}

#[test]
#[ignore = "large synthetic 1x/2x/4x source-count and paged allocation stress"]
fn synthetic_1x_2x_4x_boards_cross_the_old_million_source_ceiling() {
    for count in [300_000, 600_000, 1_200_000] {
        let mut input = source();
        input.pins.clear();
        input.vias.clear();
        input.bounds.max = Point::new(1200.0, 1200.0);
        input.segments = (0..count)
            .map(|i| {
                let a = Point::new((i % 1200) as f64, (i / 1200) as f64);
                Segment {
                    id: ObjectId(i + 1),
                    track_id: ObjectId(i + 1),
                    layer: LayerId(0),
                    net: NetId(1),
                    a,
                    b: Point::new(a.x + 0.5, a.y),
                    width: 0.1,
                    arc: None,
                    bond_wire: None,
                }
            })
            .collect();
        let board = PreparedBoard3d::preview(
            &input,
            PreviewStackup::default(),
            SolidLimits {
                max_allocation_bytes: 512 * 1024 * 1024,
                ..Default::default()
            },
            &CancellationToken::default(),
        )
        .unwrap();
        assert_eq!(board.sources.len(), count as usize);
        assert_eq!(board.instance_count(), count as usize);
        assert_eq!(board.meshes.len(), 1);
        assert!(
            board
                .pages
                .iter()
                .all(|p| p.instances.len() <= pomelo_render::scene::board3d::PAGE_INSTANCES)
        );
        assert!(board.allocation_bytes() <= 512 * 1024 * 1024);
        assert_eq!(
            board.sources.last().unwrap().object,
            SelectedObject::Segment(ObjectId(count))
        );
    }
}

#[test]
fn invalid_preview_and_exhausted_budget_do_not_publish_partial_boards() {
    let source = source();
    let cancel = CancellationToken::default();
    assert!(
        PreparedBoard3d::preview(
            &source,
            PreviewStackup {
                board_mm: 0.01,
                ..Default::default()
            },
            SolidLimits::default(),
            &cancel
        )
        .is_err()
    );
    assert!(
        PreparedBoard3d::preview(
            &source,
            PreviewStackup::default(),
            SolidLimits {
                max_vertices: 1,
                ..Default::default()
            },
            &cancel
        )
        .is_err()
    );
    cancel.cancel();
    assert!(
        PreparedBoard3d::preview(
            &source,
            PreviewStackup::default(),
            SolidLimits::default(),
            &cancel
        )
        .is_err()
    );
}

#[test]
fn parallel_preparation_preserves_geometry_ownership_and_empty_layer_warnings() {
    let mut input = source();
    input.pins.clear();
    input.layers.insert(
        0,
        Layer {
            id: LayerId(2),
            name: "empty".into(),
            function: LayerFunction::Conductor,
            color: "#cc8800".into(),
            source_flags: None,
        },
    );
    let template = input.segments[0].clone();
    input.segments = (0..10_100)
        .map(|i| Segment {
            id: ObjectId(10_000 + i),
            layer: LayerId(i % 2),
            ..template.clone()
        })
        .collect();
    // An invalid span must produce exactly one warning, including in a partition
    // with no valid geometry (the first copper layer owns invalid-span warnings).
    input.vias[0].start_layer = None;
    input.zones = vec![square_zone(99_000, 0., 4., true)];
    let cancel = CancellationToken::default();
    let serial = PreparedBoard3d::preview_with_workers(
        &input,
        PreviewStackup::default(),
        SolidLimits::default(),
        &cancel,
        1,
    )
    .unwrap();
    let parallel = PreparedBoard3d::preview_with_workers(
        &input,
        PreviewStackup::default(),
        SolidLimits::default(),
        &cancel,
        4,
    )
    .unwrap();
    let signature = |scene: &PreparedBoard3d| {
        let mut result = scene
            .sources
            .iter()
            .map(|s| {
                format!(
                    "{:?}:{:?}:{:?}:{:?}:{:?}",
                    s.object, s.layers, s.bounds, s.transform, s.extension
                )
            })
            .collect::<Vec<_>>();
        result.sort_unstable();
        result
    };
    assert_eq!(signature(&serial), signature(&parallel));
    assert_eq!(serial.bounds, parallel.bounds);
    assert_eq!(serial.index_count(), parallel.index_count());
    assert_eq!(serial.warning_count, parallel.warning_count);
    assert_eq!(parallel.warning_count, 1);
    if std::thread::available_parallelism().map_or(1, usize::from) >= 4 {
        // Confirm this exercised the layer merge instead of silently retrying serial.
        assert_ne!(serial.sources[1].object, parallel.sources[1].object);
    }
    let materials = |scene: &PreparedBoard3d| {
        scene.materials(&input, &BoardDisplay::default(), false, None, None)
    };
    for [x, y] in [
        [0.1, 0.1],
        [2., 2.],
        [3.9, 3.9],
        [0., 5.],
        [9., 5.],
        [20., 20.],
    ] {
        let ray = PickRay3d {
            origin: [x, y, 10.],
            direction: [0., 0., -1.],
            length: 20.,
        };
        assert_eq!(
            serial.pick(
                ray,
                &materials(&serial),
                pomelo_core::picking::PickFilter::default()
            ),
            parallel.pick(
                ray,
                &materials(&parallel),
                pomelo_core::picking::PickFilter::default()
            )
        );
    }

    for page in &parallel.pages {
        let mesh = &parallel.meshes[page.mesh];
        assert!(
            page.sources
                .iter()
                .all(|&id| id > 0 && id as usize <= parallel.sources.len())
        );
        for instance in &page.instances {
            assert!(instance.owner[0] as usize <= parallel.sources.len());
        }
        for vertex in &mesh.vertices {
            assert!(vertex.owner[0] as usize <= parallel.sources.len());
        }
        for &id in &page.sources {
            let source = &parallel.sources[id as usize - 1];
            assert!(
                parallel.parts[source.parts.clone()]
                    .iter()
                    .any(|p| p.mesh == page.mesh)
            );
        }
    }
    // Tiny per-layer shares must fall back without discarding geometry. The full
    // serial allowance can still hold this board.
    let budget = serial.allocation_bytes() * 2 + 128 * 1024;
    let constrained = PreparedBoard3d::preview_with_workers(
        &input,
        PreviewStackup::default(),
        SolidLimits {
            max_allocation_bytes: budget,
            ..Default::default()
        },
        &cancel,
        4,
    )
    .unwrap();
    assert_eq!(signature(&serial), signature(&constrained));
    assert_eq!(
        serial.sources.iter().map(|s| s.object).collect::<Vec<_>>(),
        constrained
            .sources
            .iter()
            .map(|s| s.object)
            .collect::<Vec<_>>()
    );
    cancel.cancel();
    assert!(
        PreparedBoard3d::preview_with_workers(
            &input,
            PreviewStackup::default(),
            SolidLimits::default(),
            &cancel,
            4
        )
        .is_err()
    );
}

#[test]
fn round_instance_shading_does_not_smooth_rectangular_or_custom_vias() {
    let mut board = source();
    board.segments.clear();
    board.pins.clear();
    board.vias.truncate(3);
    board.vias[1].drill_shape.rectangular = true;
    board.vias[2].drill_shape.custom =
        Some(std::sync::Arc::new(pomelo_core::model::CustomPadGeometry {
            contours: vec![vec![
                Point::new(-0.1, -0.1),
                Point::new(0.1, -0.1),
                Point::new(0.1, 0.1),
                Point::new(-0.1, 0.1),
            ]],
            paths: vec![],
        }));
    let scene = PreparedBoard3d::preview(
        &board,
        PreviewStackup::default(),
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let mut round = 0;
    let mut flat = 0;
    for page in &scene.pages {
        for instance in &page.instances {
            let source = &scene.sources[instance.owner[0] as usize - 1];
            if matches!(
                source.layers,
                pomelo_core::selection::MemberLayers::Span(_, _)
            ) {
                if source.object == SelectedObject::Via(board.vias[0].id) {
                    assert_eq!(instance.owner[3], 1);
                    round += 1;
                } else {
                    assert_eq!(instance.owner[3], 0);
                    flat += 1;
                }
            }
        }
    }
    assert!(round > 0 && flat > 0);
    assert_eq!(size_of::<pomelo_render::scene::board3d::Instance3d>(), 32);
}

#[test]
fn circle_lod_uses_subpixel_error_and_preserves_full_precision_picking() {
    let mut board = source();
    board.pins.clear();
    board.segments.clear();
    board.vias.truncate(1);
    let scene = PreparedBoard3d::preview(
        &board,
        PreviewStackup::default(),
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let mut circles = 0;
    for mesh in &scene.meshes {
        if mesh.round_lods.is_empty() {
            continue;
        }
        circles += 1;
        assert!(mesh.vertices.len() >= 256);
        assert_eq!(mesh.draw_indices(1.).len(), 84);
        assert!(mesh.draw_indices(1000.).len() > mesh.draw_indices(1.).len());
        assert_eq!(
            mesh.draw_indices(f32::INFINITY),
            0..mesh.fine_index_count() as u32
        );
        for lod in &mesh.round_lods {
            assert!(lod.indices.start >= mesh.fine_index_count() as u32);
            assert!(
                mesh.indices[lod.indices.start as usize..lod.indices.end as usize]
                    .iter()
                    .all(|i| (*i as usize) < mesh.vertices.len())
            );
        }
    }
    assert!(circles > 0);
    for part in &scene.parts {
        assert!((part.indices.end as usize) <= scene.meshes[part.mesh].fine_index_count());
    }
}
