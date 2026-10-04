use super::*;
use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    model::{Arc as BoardArc, LayerId, NetId, Segment},
};

pub(crate) fn board(zones: Vec<Zone>) -> Arc<BoardScene> {
    Arc::new(BoardScene {
        layers: vec![],
        special_layers: vec![],
        nets: BTreeMap::new(),
        segments: vec![],
        pins: vec![],
        components: vec![],
        vias: vec![],
        zones,
        outline: vec![],
        texts: vec![],
        drawing_layers: vec![],
        drawings: vec![],
        bounds: bounds(-5.0, 5.0),
        diagnostics: vec![],
    })
}
fn bounds(min: f64, max: f64) -> Bounds {
    Bounds {
        min: Point::new(min, min),
        max: Point::new(max, max),
    }
}
pub(crate) fn lines(ring: &[Point]) -> Vec<Segment> {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(&a, &b)| Segment {
            id: ObjectId(0),
            track_id: ObjectId(0),
            layer: LayerId(1),
            net: NetId(1),
            a,
            b,
            width: 0.0,
            arc: None,
            bond_wire: None,
        })
        .collect()
}
pub(crate) fn circle_path(center: Point, radius: f64) -> Vec<Segment> {
    vec![Segment {
        arc: Some(BoardArc {
            center,
            radius,
            start: 0.0,
            sweep: std::f64::consts::TAU,
        }),
        ..lines(&[Point::new(center.x + radius, center.y)])[0].clone()
    }]
}
pub(crate) fn circle_ring(center: Point, radius: f64) -> Vec<Point> {
    (0..12)
        .map(|i| {
            let a = i as f64 * std::f64::consts::TAU / 12.0;
            Point::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}
pub(crate) fn zone(id: u32, rings: Vec<Vec<Point>>, paths: Vec<Vec<Segment>>) -> Zone {
    let mesh = CopperMesh::build(
        &rings,
        &paths,
        &MeshLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    Zone {
        kind: pomelo_core::model::ZoneKind::Unknown,
        id: ObjectId(id),
        layer: LayerId(1),
        net: NetId(1),
        mesh,
        paths,
    }
}
fn disk(id: u32, x: f64) -> Zone {
    let p = Point::new(x, 0.0);
    zone(id, vec![circle_ring(p, 1.0)], vec![circle_path(p, 1.0)])
}
fn view(x: f64, tolerance: f64) -> CurveView {
    CurveView {
        bounds: Bounds {
            min: Point::new(x - 0.5, -0.5),
            max: Point::new(x + 0.5, 0.5),
        },
        tolerance,
    }
}
fn prepare(
    scene: &Arc<BoardScene>,
    view: CurveView,
    previous: &CurveFillCache,
    limits: CurveCacheLimits,
) -> CurveFillCache {
    CurveFillCache::prepare(
        scene,
        view,
        &BoardDisplay::default(),
        previous,
        limits,
        &CancellationToken::default(),
    )
    .unwrap()
}

#[test]
fn refined_curve_cache_keeps_source_static_shape_kind() {
    let mut shape = disk(123, 0.0);
    shape.kind = pomelo_core::model::ZoneKind::Static;
    let scene = board(vec![shape]);
    let refined = prepare(
        &scene,
        view(0.0, 1e-4),
        &CurveFillCache::default(),
        CurveCacheLimits::default(),
    );
    let batch = &refined.entries[&ObjectId(123)].source.batches[0];
    assert_eq!(
        (batch.kind, batch.selected_object),
        (
            pomelo_core::model::ZoneKind::Static,
            pomelo_core::selection::SelectedObject::Zone(ObjectId(123))
        )
    );
}
#[test]
fn threshold_is_physical_and_flip_keeps_normalized_view_bounds() {
    let mut camera = Camera {
        center: Point::new(100_000.0, 0.0),
        pixels_per_mm: 1000.0,
        flipped: false,
    };
    assert!(CurveView::new(camera, 128.0, 64.0, 1.0).is_none());
    let a = CurveView::new(camera, 128.0, 64.0, 2.0).unwrap();
    camera.flipped = true;
    let b = CurveView::new(camera, 128.0, 64.0, 2.0).unwrap();
    assert_eq!((a, a.tolerance), (b, 0.25 / 2048.0));
}
#[test]
fn pan_and_zoom_out_reuse_higher_detail_but_zoom_in_rebuilds() {
    let scene = board(vec![disk(1, 0.0)]);
    let first = prepare(
        &scene,
        view(0.0, 1e-4),
        &CurveFillCache::default(),
        CurveCacheLimits::default(),
    );
    let pan = prepare(&scene, view(0.2, 2e-4), &first, CurveCacheLimits::default());
    assert!(Arc::ptr_eq(
        &first.entries[&ObjectId(1)].source,
        &pan.entries[&ObjectId(1)].source
    ));
    let fine = prepare(&scene, view(0.2, 5e-5), &pan, CurveCacheLimits::default());
    assert!(!Arc::ptr_eq(
        &pan.entries[&ObjectId(1)].source,
        &fine.entries[&ObjectId(1)].source
    ));
    assert_eq!((fine.statistics.hits, fine.statistics.misses), (1, 2));
}
#[test]
fn soft_budget_pins_all_visible_entries_and_evicts_only_offscreen() {
    let scene = board(vec![disk(1, -3.0), disk(2, 3.0)]);
    let limits = CurveCacheLimits {
        soft_bytes: 0,
        ..CurveCacheLimits::default()
    };
    let both = prepare(
        &scene,
        CurveView {
            bounds: bounds(-4.0, 4.0),
            tolerance: 0.01,
        },
        &CurveFillCache::default(),
        limits,
    );
    assert_eq!(both.entries.len(), 2);
    assert_eq!(both.statistics.over_budget_bytes, both.statistics.bytes);
    let next = prepare(&scene, view(3.0, 0.01), &both, limits);
    assert_eq!(
        next.entries.keys().copied().collect::<Vec<_>>(),
        [ObjectId(2)]
    );
    assert!(Arc::ptr_eq(
        &both.entries[&ObjectId(2)].source,
        &next.entries[&ObjectId(2)].source
    ));
    let inactive = next.inactive(0);
    assert!(
        inactive.active.is_empty() && inactive.entries.is_empty() && inactive.statistics.bytes == 0
    );
}
#[test]
fn hard_budget_and_cancellation_preserve_the_previous_snapshot() {
    let scene = board(vec![disk(1, 0.0)]);
    let old = prepare(
        &scene,
        view(0.0, 0.01),
        &CurveFillCache::default(),
        CurveCacheLimits::default(),
    );
    let limits = CurveCacheLimits {
        max_bytes: 1,
        ..CurveCacheLimits::default()
    };
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        CurveFillCache::prepare(
            &scene,
            view(0.0, 1e-8),
            &BoardDisplay::default(),
            &old,
            limits,
            &CancellationToken::default()
        ),
        Err(PrepareError::Limit { .. })
    ));
    assert!(matches!(
        CurveFillCache::prepare(
            &scene,
            view(0.0, 1e-8),
            &BoardDisplay::default(),
            &old,
            CurveCacheLimits::default(),
            &cancelled
        ),
        Err(PrepareError::Cancelled)
    ));
    assert_eq!(old.statistics.misses, 1);
}
#[test]
fn hidden_and_nonfilled_zones_have_no_active_overrides() {
    let scene = board(vec![disk(1, 0.0)]);
    for mut display in [
        BoardDisplay {
            filled: false,
            ..BoardDisplay::default()
        },
        BoardDisplay {
            show_copper: false,
            ..BoardDisplay::default()
        },
        BoardDisplay::default(),
    ] {
        if display.filled && display.show_copper {
            display.hidden_layers.insert(LayerId(1));
        }
        let result = CurveFillCache::prepare(
            &scene,
            view(0.0, 0.01),
            &display,
            &CurveFillCache::default(),
            CurveCacheLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap();
        assert!(result.active.is_empty() && result.entries.is_empty());
    }
}

#[test]
fn a_new_scene_with_the_same_object_id_cannot_reuse_old_geometry() {
    let scene = board(vec![disk(1, 0.0)]);
    let previous = prepare(
        &scene,
        view(0.0, 0.01),
        &CurveFillCache::default(),
        CurveCacheLimits::default(),
    );
    let replacement = board(vec![disk(1, 0.25)]);
    let next = prepare(
        &replacement,
        view(0.0, 0.01),
        &previous,
        CurveCacheLimits::default(),
    );
    assert!(!Arc::ptr_eq(
        &previous.entries[&ObjectId(1)].source,
        &next.entries[&ObjectId(1)].source
    ));
}
