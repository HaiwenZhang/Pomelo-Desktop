use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    geometry3d::solid::{SolidError, SolidLimits, SurfaceKind},
    interaction::selection::{MemberLayers, SelectedObject},
    model::{LayerId, NetId, ObjectId, Point, Zone, ZoneKind},
    task::CancellationToken,
};
use pomelo_render::scene::solid::{PadSolidSource, PreparedSolid, SolidPrepareError, ViaWallSpan};

fn zone() -> Zone {
    let ring = |x: f64, y: f64, size: f64| {
        vec![
            Point::new(x, y),
            Point::new(x + size, y),
            Point::new(x + size, y + size),
            Point::new(x, y + size),
        ]
    };
    Zone {
        id: ObjectId(42),
        layer: LayerId(3),
        net: NetId(7),
        kind: ZoneKind::Static,
        paths: vec![],
        mesh: CopperMesh::build(
            &[ring(0.0, 0.0, 10.0), ring(2.0, 2.0, 4.0)],
            &[],
            &MeshLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    }
}

#[test]
fn zone_preparation_preserves_identity_and_does_not_fill_coverage_holes() {
    let source = zone();
    let solid = PreparedSolid::prepare_zone(
        &source,
        [0.1, 0.135],
        0.001,
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(
        (solid.object, solid.layers, solid.net),
        (
            SelectedObject::Zone(ObjectId(42)),
            MemberLayers::Single(LayerId(3)),
            NetId(7)
        )
    );
    assert!(
        solid
            .mesh
            .surfaces
            .iter()
            .any(|s| s.kind == SurfaceKind::HoleWall)
    );
    let bounds = solid.mesh.bounds.unwrap();
    assert_eq!((bounds.min[2], bounds.max[2]), (0.1, 0.135));
    let area: f64 = solid
        .mesh
        .surfaces
        .iter()
        .filter(|s| s.kind == SurfaceKind::Top)
        .map(|s| {
            solid.mesh.indices[s.indices.start as usize..s.indices.end as usize]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    let a = solid.mesh.vertices[t[0] as usize].position;
                    let b = solid.mesh.vertices[t[1] as usize].position;
                    let c = solid.mesh.vertices[t[2] as usize].position;
                    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2.0
                })
                .sum::<f64>()
        })
        .sum();
    assert!((area - 84.0).abs() < 1e-9);
}

#[test]
fn malformed_source_ranges_report_the_source_object() {
    let mut source = zone();
    source.mesh.ring_offsets = vec![0, 9, 4];
    let error = PreparedSolid::prepare_zone(
        &source,
        [0.0, 0.035],
        0.001,
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        SolidPrepareError::Geometry {
            object: SelectedObject::Zone(ObjectId(42)),
            source: SolidError::Invalid("RING_OFFSETS")
        }
    ));
}

#[test]
fn analytic_boundaries_are_resampled_at_the_requested_lod() {
    use pomelo_core::model::{Arc, Segment};
    let mut source = zone();
    source.mesh = CopperMesh::default();
    source.paths = vec![vec![Segment {
        id: ObjectId(43),
        track_id: ObjectId(43),
        layer: source.layer,
        net: source.net,
        a: Point::new(2.0, 0.0),
        b: Point::new(2.0, 0.0),
        width: 0.0,
        bond_wire: None,
        arc: Some(Arc {
            center: Point::default(),
            radius: 2.0,
            start: 0.0,
            sweep: std::f64::consts::TAU,
        }),
    }]];
    let cancel = CancellationToken::default();
    let coarse =
        PreparedSolid::prepare_zone(&source, [0.0, 0.035], 0.1, SolidLimits::default(), &cancel)
            .unwrap();
    let fine = PreparedSolid::prepare_zone(
        &source,
        [0.0, 0.035],
        0.001,
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert!(fine.mesh.vertices.len() > coarse.mesh.vertices.len());
    let bounds = fine.mesh.bounds.unwrap();
    assert!((bounds.max[0] - 2.0).abs() < 1e-6);
    assert!(bounds.min[0] < -1.99 && bounds.min[1] < -1.99 && bounds.max[1] > 1.99);
    assert!(
        fine.mesh
            .surfaces
            .iter()
            .all(|s| s.kind != SurfaceKind::HoleWall)
    );
}

fn top_area(solid: &PreparedSolid) -> f64 {
    solid
        .mesh
        .surfaces
        .iter()
        .filter(|s| s.kind == SurfaceKind::Top)
        .map(|s| {
            solid.mesh.indices[s.indices.start as usize..s.indices.end as usize]
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| {
                    let a = solid.mesh.vertices[t[0] as usize].position;
                    let b = solid.mesh.vertices[t[1] as usize].position;
                    let c = solid.mesh.vertices[t[2] as usize].position;
                    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])) / 2.0
                })
                .sum::<f64>()
        })
        .sum()
}

fn world_xy(solid: &PreparedSolid, index: u32) -> Point {
    let p = solid.mesh.vertices[index as usize].position;
    let p = Point::new(p[0], p[1]).rotate(solid.placement.angle);
    Point::new(p.x + solid.placement.at.x, p.y + solid.placement.at.y)
}

#[test]
fn traces_cover_lines_arcs_full_circles_and_zero_length_round_caps() {
    use pomelo_core::model::{Arc, Segment};
    let mut source = Segment {
        id: ObjectId(10),
        track_id: ObjectId(11),
        layer: LayerId(1),
        net: NetId(2),
        a: Point::default(),
        b: Point::new(10.0, 0.0),
        width: 2.0,
        arc: None,
        bond_wire: None,
    };
    let cancel = CancellationToken::default();
    let straight = PreparedSolid::prepare_trace(
        &source,
        [0.0, 0.035],
        0.001,
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert_eq!(straight.object, SelectedObject::Segment(ObjectId(10)));
    assert!((top_area(&straight) - (20.0 + std::f64::consts::PI)).abs() < 0.01);
    source.b = source.a;
    let dot = PreparedSolid::prepare_trace(
        &source,
        [0.0, 0.035],
        0.001,
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert!((top_area(&dot) - std::f64::consts::PI).abs() < 0.01);
    source.a = Point::new(5.0, 0.0);
    source.b = Point::new(0.0, 5.0);
    source.arc = Some(Arc {
        center: Point::default(),
        radius: 5.0,
        start: 0.0,
        sweep: std::f64::consts::FRAC_PI_2,
    });
    let arc = PreparedSolid::prepare_trace(
        &source,
        [0.0, 0.035],
        0.001,
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert!(
        (top_area(&arc) - (5.0 * 2.0 * std::f64::consts::FRAC_PI_2 + std::f64::consts::PI)).abs()
            < 0.03
    );
    source.b = source.a;
    source.arc.as_mut().unwrap().sweep = std::f64::consts::TAU;
    let full = PreparedSolid::prepare_trace(
        &source,
        [0.0, 0.035],
        0.001,
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert!((top_area(&full) - 20.0 * std::f64::consts::PI).abs() < 0.05);
    assert!(
        full.mesh
            .surfaces
            .iter()
            .any(|s| s.kind == SurfaceKind::HoleWall)
    );
}

#[test]
fn pad_drill_stays_at_the_owner_when_copper_is_offset_and_rotated() {
    use pomelo_core::{
        model::{DrillShape, Pad},
        pad::PadPlacement,
    };
    let mut pad = Pad::circle(LayerId(2), 4.0);
    pad.offset = Point::new(1.0, 0.0);
    let solid = PreparedSolid::prepare_pad(
        PadSolidSource {
            object: SelectedObject::Pin(ObjectId(8)),
            net: NetId(3),
            pad: &pad,
            placement: PadPlacement {
                at: Point::new(10.0, 20.0),
                angle: std::f64::consts::FRAC_PI_2,
                mirrored: false,
            },
            drill: Some(DrillShape {
                width: 1.0,
                height: 1.0,
                plated: true,
            }),
        },
        [0.0, 0.035],
        0.0005,
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!((top_area(&solid) - 3.75 * std::f64::consts::PI).abs() < 0.01);
    for surface in solid
        .mesh
        .surfaces
        .iter()
        .filter(|s| s.kind == SurfaceKind::HoleWall)
    {
        for &index in
            &solid.mesh.indices[surface.indices.start as usize..surface.indices.end as usize]
        {
            assert!((world_xy(&solid, index).distance(Point::new(10.0, 20.0)) - 0.5).abs() < 2e-6);
        }
    }
}

#[test]
fn mirrored_custom_pad_keeps_offset_and_rotation_order_in_its_instance() {
    use pomelo_core::{
        model::{CustomPadGeometry, Pad, PadKind},
        pad::PadPlacement,
    };
    let mut pad = Pad::circle(LayerId(2), 4.0);
    pad.kind = PadKind::CUSTOM;
    pad.offset = Point::new(1.0, -2.0);
    pad.custom = Some(std::sync::Arc::new(CustomPadGeometry {
        contours: vec![vec![
            Point::default(),
            Point::new(3.0, 0.0),
            Point::new(0.0, 2.0),
        ]],
        paths: vec![],
    }));
    let solid = PreparedSolid::prepare_pad(
        PadSolidSource {
            object: SelectedObject::Pin(ObjectId(8)),
            net: NetId(3),
            pad: &pad,
            placement: PadPlacement {
                at: Point::new(10.0, 20.0),
                angle: std::f64::consts::FRAC_PI_2,
                mirrored: true,
            },
            drill: None,
        },
        [0.0, 0.035],
        0.001,
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!((top_area(&solid) - 3.0).abs() < 1e-9);
    let bounds = pomelo_core::model::Bounds::from_points(
        (0..solid.mesh.vertices.len()).map(|i| world_xy(&solid, i as u32)),
    )
    .unwrap();
    assert!((bounds.min.x - 11.0).abs() < 1e-9 && (bounds.min.y - 18.0).abs() < 1e-9);
    assert!((bounds.max.x - 13.0).abs() < 1e-9 && (bounds.max.y - 21.0).abs() < 1e-9);
    let world = solid.world_bounds().unwrap();
    assert!((world.min[0] - 11.0).abs() < 1e-9 && (world.min[1] - 18.0).abs() < 1e-9);
    assert!((world.max[0] - 13.0).abs() < 1e-9 && (world.max[1] - 21.0).abs() < 1e-9);
    assert!(!solid.placement.mirrored);
}

#[test]
fn a_drill_inside_a_donut_opening_does_not_restore_copper() {
    use pomelo_core::{
        model::{DrillShape, Pad, PadKind},
        pad::PadPlacement,
    };
    let mut pad = Pad::circle(LayerId(2), 4.0);
    pad.kind = PadKind::DONUT;
    pad.inner_diameter = Some(2.0);
    let solid = PreparedSolid::prepare_pad(
        PadSolidSource {
            object: SelectedObject::Via(ObjectId(8)),
            net: NetId(3),
            pad: &pad,
            placement: PadPlacement::default(),
            drill: Some(DrillShape {
                width: 1.0,
                height: 1.0,
                plated: true,
            }),
        },
        [0.0, 0.035],
        0.0005,
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!((top_area(&solid) - 3.0 * std::f64::consts::PI).abs() < 0.01);
}

fn via() -> pomelo_core::model::Via {
    use pomelo_core::model::{DrillShape, Via};
    Via {
        id: ObjectId(12),
        net: NetId(3),
        at: Point::new(10.0, 20.0),
        drill: 1.0,
        drill_shape: DrillShape {
            width: 1.0,
            height: 1.0,
            plated: true,
        },
        padstack: ObjectId(13),
        padstack_name: String::new(),
        start_layer: Some(LayerId(1)),
        end_layer: Some(LayerId(6)),
        pads: std::sync::Arc::from([]),
        backdrill: None,
        stackup_region: None,
        angle: 0.0,
        mirrored: false,
        finger: None,
    }
}

#[test]
fn via_wall_is_hollow_and_uses_only_the_resolved_retained_interval() {
    let source = via();
    let solid = PreparedSolid::prepare_via_wall(
        &source,
        ViaWallSpan {
            z: [-0.4, 0.2],
            layers: [LayerId(2), LayerId(4)],
            plating_thickness_mm: 0.05,
        },
        0.0001,
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(solid.object, SelectedObject::Via(ObjectId(12)));
    assert_eq!(solid.layers, MemberLayers::Span(LayerId(2), LayerId(4)));
    assert!(
        (top_area(&solid) - std::f64::consts::PI * (0.55_f64.powi(2) - 0.5_f64.powi(2))).abs()
            < 0.001
    );
    let bounds = solid.mesh.bounds.unwrap();
    assert_eq!((bounds.min[2], bounds.max[2]), (-0.4, 0.2));
    assert_eq!(solid.placement.at, source.at);
    assert!(
        solid
            .mesh
            .surfaces
            .iter()
            .any(|s| s.kind == SurfaceKind::HoleWall)
    );
}

#[test]
fn plated_slots_preserve_the_straight_section_and_placement() {
    let mut source = via();
    source.drill_shape.width = 2.0;
    source.angle = std::f64::consts::FRAC_PI_4;
    source.mirrored = true;
    let solid = PreparedSolid::prepare_via_wall(
        &source,
        ViaWallSpan {
            z: [-0.4, 0.2],
            layers: [LayerId(2), LayerId(4)],
            plating_thickness_mm: 0.05,
        },
        0.0001,
        SolidLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let expected = 0.1 + std::f64::consts::PI * (0.55_f64.powi(2) - 0.5_f64.powi(2));
    assert!((top_area(&solid) - expected).abs() < 0.001);
    assert_eq!(solid.placement.angle, source.angle);
    assert_eq!(solid.placement.at, source.at);
    assert!(solid.world_bounds().unwrap().is_valid());
}

#[test]
fn nonplated_holes_and_unknown_wall_thickness_do_not_create_metal() {
    let mut source = via();
    source.drill_shape.plated = false;
    let prepare = |via: &pomelo_core::model::Via, thickness| {
        PreparedSolid::prepare_via_wall(
            via,
            ViaWallSpan {
                z: [-0.4, 0.2],
                layers: [LayerId(2), LayerId(4)],
                plating_thickness_mm: thickness,
            },
            0.001,
            SolidLimits::default(),
            &CancellationToken::default(),
        )
    };
    assert!(matches!(
        prepare(&source, 0.05),
        Err(SolidPrepareError::Geometry {
            source: SolidError::Unsupported("NPTH_WALL"),
            ..
        })
    ));
    source.drill_shape.plated = true;
    assert!(matches!(
        prepare(&source, f64::NAN),
        Err(SolidPrepareError::Geometry {
            source: SolidError::Invalid("VIA_WALL"),
            ..
        })
    ));
}

#[test]
fn actual_backdrill_cuts_define_retained_z_independently_of_display_diameter() {
    use pomelo_core::{
        geometry3d::drill::DrillCut3d,
        model::{Backdrill, BackdrillDefinition, BackdrillSpan},
    };
    let mut source = via();
    source.backdrill = Some(Backdrill {
        definition: BackdrillDefinition {
            spans: vec![BackdrillSpan {
                start_layer: LayerId(1),
                stop_layer: LayerId(2),
                protected_layer: LayerId(3),
            }],
            display_diameter: 99.0,
            start_pad_diameter: 99.0,
            label_diameter: 99.0,
        },
        source_reference: ObjectId(45),
        rotation_degrees: 0.0,
        mirrored: false,
    });
    let span = ViaWallSpan {
        z: [-0.8, 0.8],
        layers: [LayerId(1), LayerId(6)],
        plating_thickness_mm: 0.05,
    };
    let cancel = CancellationToken::default();
    assert!(matches!(
        PreparedSolid::prepare_via_walls(
            &source,
            span,
            &[],
            0.001,
            SolidLimits::default(),
            &cancel
        ),
        Err(SolidPrepareError::Geometry {
            source: SolidError::Unsupported("BACKDRILL_CUTS_REQUIRED"),
            ..
        })
    ));
    let cuts = [
        DrillCut3d {
            z: [0.3, 1.0],
            diameter_mm: 1.2,
        },
        DrillCut3d {
            z: [-1.0, -0.2],
            diameter_mm: 1.2,
        },
    ];
    let solids = PreparedSolid::prepare_via_walls(
        &source,
        span,
        &cuts,
        0.001,
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert_eq!(solids.len(), 1);
    let bounds = solids[0].mesh.bounds.unwrap();
    assert_eq!((bounds.min[2], bounds.max[2]), (-0.2, 0.3));
    assert_eq!(solids[0].object, SelectedObject::Via(source.id));
}

#[test]
fn via_cut_segments_share_one_aggregate_geometry_budget() {
    use pomelo_core::geometry3d::drill::{DrillCut3d, DrillSpanError};
    let source = via();
    let span = ViaWallSpan {
        z: [-0.8, 0.8],
        layers: [LayerId(1), LayerId(6)],
        plating_thickness_mm: 0.05,
    };
    let cancel = CancellationToken::default();
    let cuts = [DrillCut3d {
        z: [-0.1, 0.1],
        diameter_mm: 1.2,
    }];
    let solids = PreparedSolid::prepare_via_walls(
        &source,
        span,
        &cuts,
        0.001,
        SolidLimits::default(),
        &cancel,
    )
    .unwrap();
    assert_eq!(solids.len(), 2);
    assert_eq!(solids[0].mesh.bounds.unwrap().max[2], -0.1);
    assert_eq!(solids[1].mesh.bounds.unwrap().min[2], 0.1);
    let limits = SolidLimits {
        max_vertices: solids[0].mesh.vertices.len(),
        ..Default::default()
    };
    assert!(matches!(
        PreparedSolid::prepare_via_walls(&source, span, &cuts, 0.001, limits, &cancel),
        Err(SolidPrepareError::Geometry {
            source: SolidError::Limit {
                resource: "vertices",
                ..
            },
            ..
        })
    ));
    let partial = [DrillCut3d {
        diameter_mm: 1.05,
        ..cuts[0]
    }];
    assert!(matches!(
        PreparedSolid::prepare_via_walls(
            &source,
            span,
            &partial,
            0.001,
            SolidLimits::default(),
            &cancel
        ),
        Err(SolidPrepareError::Drill {
            source: DrillSpanError::PartialWallCut,
            ..
        })
    ));
    let removed = [DrillCut3d {
        z: [-1.0, 1.0],
        diameter_mm: 1.2,
    }];
    assert!(
        PreparedSolid::prepare_via_walls(
            &source,
            span,
            &removed,
            0.001,
            SolidLimits::default(),
            &cancel
        )
        .unwrap()
        .is_empty()
    );
}
