use super::*;
use crate::{copper::CopperLimits, pads::PadLimits, tracks::TraceLimits};
use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    model::{CustomPadGeometry, DrillShape, NetId, Pin, Via, Zone},
};
use std::sync::Arc as Shared;

const PALETTE: Palette = Palette {
    background: [16, 26, 23],
    copper: [104, 180, 96],
    pads: [179, 211, 115],
    outline: [227, 199, 86],
    drawings: [127, 169, 132],
};
struct Batches {
    bounds: Bounds,
    tracks: PreparedTracks,
    drawings: PreparedTracks,
    copper: PreparedCopper,
    pads: PreparedPads,
    drills: PreparedPads,
}
impl Batches {
    fn new(
        bounds: Bounds,
        segments: &[Segment],
        pins: &[Pin],
        vias: &[Via],
        zones: &[Zone],
    ) -> Self {
        let cancel = CancellationToken::default();
        let mut pads = PreparedPads::build(pins, vias, PadLimits::default(), &cancel).unwrap();
        pads.custom_mesh = Some(Shared::new(
            pads.build_custom_meshes(CopperLimits::default(), &MeshLimits::default(), &cancel)
                .unwrap(),
        ));
        Self {
            bounds,
            tracks: PreparedTracks::build_with_outline(
                segments,
                &[],
                TraceLimits::default(),
                &cancel,
            )
            .unwrap(),
            drawings: PreparedTracks::build_drawings(&[], TraceLimits::default(), &cancel).unwrap(),
            copper: PreparedCopper::build(zones, CopperLimits::default(), &cancel).unwrap(),
            pads,
            drills: super::super::drills::PreparedDrills::build(
                pins,
                vias,
                PadLimits::default(),
                &cancel,
            )
            .unwrap()
            .geometry,
        }
    }
    fn source(&self) -> Source<'_> {
        Source {
            bounds: self.bounds,
            tracks: &self.tracks,
            drawings: &self.drawings,
            copper: &self.copper,
            pads: &self.pads,
            drills: &self.drills,
        }
    }
    fn render(&self) -> Thumbnail {
        rasterize(
            self.source(),
            PALETTE,
            Limits::default(),
            &CancellationToken::default(),
        )
        .unwrap()
        .unwrap()
    }
    fn pixel(&self, image: &Thumbnail, at: Point) -> [u8; 3] {
        let at = Projection::new(self.bounds).unwrap().screen(at);
        let index = ((at.y.floor() as usize) * WIDTH + at.x.floor() as usize) * 4;
        image.rgba[index..index + 3].try_into().unwrap()
    }
}
fn bounds() -> Bounds {
    Bounds {
        min: Point::new(0.0, 0.0),
        max: Point::new(10.0, 10.0),
    }
}
fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Point> {
    vec![
        Point::new(x0, y0),
        Point::new(x1, y0),
        Point::new(x1, y1),
        Point::new(x0, y1),
    ]
}
fn zone(id: u32, rings: &[Vec<Point>]) -> Zone {
    Zone {
        kind: pomelo_core::model::ZoneKind::Unknown,
        id: ObjectId(id),
        layer: LayerId(1),
        net: NetId(1),
        paths: vec![],
        mesh: CopperMesh::build(
            rings,
            &[],
            &MeshLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    }
}
fn pin(id: u32, at: Point, pads: Vec<Pad>) -> Pin {
    Pin {
        id: ObjectId(id),
        owner_id: ObjectId(0),
        net: NetId(1),
        name: String::new(),
        reference: String::new(),
        at,
        angle: 0.0,
        mirrored: false,
        drill: 0.0,
        drill_shape: DrillShape {
            width: 0.0,
            height: 0.0,
            plated: true,
        },
        pads,
        stackup_region: None,
        die: None,
    }
}
fn line(id: u32, a: Point, b: Point, width: f64) -> Segment {
    Segment {
        id: ObjectId(id),
        track_id: ObjectId(id),
        layer: LayerId(1),
        net: NetId(1),
        a,
        b,
        width,
        arc: None,
        bond_wire: None,
    }
}

#[test]
fn source_backed_copper_thumbnail_matches_materialized_pixels_and_holes() {
    let mut empty = zone(3, &[rectangle(0.0, 0.0, 1.0, 1.0)]);
    empty.mesh = CopperMesh::default();
    let zones = vec![
        zone(1, &[rectangle(0.0, 0.0, 10.0, 10.0)]),
        empty,
        zone(
            2,
            &[rectangle(1.0, 1.0, 9.0, 9.0), rectangle(3.0, 3.0, 7.0, 7.0)],
        ),
    ];
    let mut batches = Batches::new(bounds(), &[], &[], &[], &zones);
    let eager = batches.render();
    let scene = Shared::new(pomelo_core::model::BoardScene {
        layers: vec![],
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
        bounds: bounds(),
        diagnostics: vec![],
    });
    batches.copper = PreparedCopper::build_scene(
        scene,
        CopperLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let source = batches.render();
    assert_eq!(source.rgba, eager.rgba);
    assert_eq!(source.summary.copper, eager.summary.copper);
    assert_eq!(source.summary.limited, eager.summary.limited);
}

#[test]
fn copper_subtracts_union_of_overlapping_holes_without_erasing_lower_coverage() {
    let perforated = zone(
        1,
        &[
            rectangle(0.0, 0.0, 10.0, 10.0),
            rectangle(3.0, 3.0, 7.0, 7.0),
            rectangle(4.0, 4.0, 8.0, 8.0),
        ],
    );
    let batch = Batches::new(bounds(), &[], &[], &[], std::slice::from_ref(&perforated));
    let image = batch.render();
    for at in [
        Point::new(3.5, 3.5),
        Point::new(5.0, 5.0),
        Point::new(7.5, 7.5),
    ] {
        assert_eq!(batch.pixel(&image, at), PALETTE.background);
    }
    let body = batch.pixel(&image, Point::new(1.0, 1.0));
    assert_ne!(body, PALETTE.background);
    let stacked = Batches::new(
        bounds(),
        &[],
        &[],
        &[],
        &[zone(2, &[rectangle(0.0, 0.0, 10.0, 10.0)]), perforated],
    );
    let stacked_image = stacked.render();
    assert_eq!(stacked.pixel(&stacked_image, Point::new(5.0, 5.0)), body);
    assert_ne!(stacked.pixel(&stacked_image, Point::new(1.0, 1.0)), body);
}

#[test]
fn analytic_and_custom_pads_preserve_rotation_mirror_offset_and_holes() {
    let mut rectangle_pad = Pad::circle(LayerId(1), 3.0);
    rectangle_pad.kind = PadKind(5);
    rectangle_pad.height = 1.0;
    let mut rotated = pin(1, Point::new(2.0, 2.0), vec![rectangle_pad]);
    rotated.angle = std::f64::consts::FRAC_PI_2;
    let mut custom_pad = Pad::circle(LayerId(1), 2.0);
    custom_pad.kind = PadKind::CUSTOM;
    custom_pad.offset = Point::new(0.5, 0.0);
    custom_pad.custom = Some(Shared::new(CustomPadGeometry {
        contours: vec![rectangle(0.0, 0.0, 2.0, 2.0), rectangle(0.5, 0.5, 1.5, 1.5)],
        paths: vec![],
    }));
    let mut custom = pin(2, Point::new(5.0, 5.0), vec![custom_pad]);
    custom.mirrored = true;
    custom.angle = std::f64::consts::FRAC_PI_2;
    let batch = Batches::new(bounds(), &[], &[rotated, custom], &[], &[]);
    let image = batch.render();
    assert_eq!(batch.pixel(&image, Point::new(2.0, 3.0)), PALETTE.pads);
    assert_eq!(
        batch.pixel(&image, Point::new(3.0, 2.0)),
        PALETTE.background
    );
    assert_eq!(batch.pixel(&image, Point::new(5.8, 5.2)), PALETTE.pads);
    assert_eq!(
        batch.pixel(&image, Point::new(6.5, 6.0)),
        PALETTE.background
    );
}

#[test]
fn physical_drills_clear_pad_copper_and_donut_openings_remain_independent() {
    let mut donut = Pad::circle(LayerId(1), 2.0);
    donut.kind = PadKind::DONUT;
    donut.inner_diameter = Some(1.0);
    let a = pin(1, Point::new(2.0, 5.0), vec![donut]);
    let mut b = pin(2, Point::new(7.0, 5.0), vec![Pad::circle(LayerId(1), 2.0)]);
    b.drill_shape = DrillShape {
        width: 0.8,
        height: 0.8,
        plated: true,
    };
    let batch = Batches::new(bounds(), &[], &[a, b], &[], &[]);
    let image = batch.render();
    assert_eq!(
        batch.pixel(&image, Point::new(2.0, 5.0)),
        PALETTE.background
    );
    assert_eq!(
        batch.pixel(&image, Point::new(7.0, 5.0)),
        PALETTE.background
    );
    assert_eq!(batch.pixel(&image, Point::new(2.75, 5.0)), PALETTE.pads);
    assert_eq!(batch.pixel(&image, Point::new(7.75, 5.0)), PALETTE.pads);
    assert_eq!(image.summary.drills, 1);
}

#[test]
fn traces_keep_width_round_caps_and_directed_arc_sweep() {
    let mut arc = line(2, Point::new(8.0, 5.0), Point::new(5.0, 8.0), 0.8);
    arc.arc = Some(Arc {
        center: Point::new(5.0, 5.0),
        radius: 3.0,
        start: 0.0,
        sweep: std::f64::consts::FRAC_PI_2,
    });
    let batch = Batches::new(
        bounds(),
        &[
            line(1, Point::new(2.0, 2.0), Point::new(4.0, 2.0), 1.0),
            arc,
        ],
        &[],
        &[],
        &[],
    );
    let image = batch.render();
    assert_eq!(batch.pixel(&image, Point::new(3.0, 2.25)), PALETTE.copper);
    assert_eq!(batch.pixel(&image, Point::new(1.8, 2.0)), PALETTE.copper);
    assert_eq!(batch.pixel(&image, Point::new(5.0, 8.0)), PALETTE.copper);
    assert_eq!(
        batch.pixel(&image, Point::new(2.0, 5.0)),
        PALETTE.background
    );
}

#[test]
fn overview_budget_skips_whole_copper_batches_including_their_holes() {
    let batch = Batches::new(
        bounds(),
        &[],
        &[],
        &[],
        &[zone(
            1,
            &[
                rectangle(0.0, 0.0, 10.0, 10.0),
                rectangle(3.0, 3.0, 7.0, 7.0),
            ],
        )],
    );
    for limits in [
        Limits {
            pixels_per_pass: 1,
            ..Limits::default()
        },
        Limits {
            triangles_per_pass: 2,
            ..Limits::default()
        },
    ] {
        let image = rasterize(
            batch.source(),
            PALETTE,
            limits,
            &CancellationToken::default(),
        )
        .unwrap()
        .unwrap();
        assert!(image.summary.limited);
        assert_eq!(image.summary.copper, 0);
        assert!(
            image
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[..3] == PALETTE.background)
        );
    }
}

#[test]
fn full_circle_batches_survive_float32_sweep_rounding_in_both_directions() {
    for direction in [-1.0, 1.0] {
        let mut circle = line(1, Point::new(8.0, 5.0), Point::new(8.0, 5.0), 0.8);
        circle.arc = Some(Arc {
            center: Point::new(5.0, 5.0),
            radius: 3.0,
            start: 0.0,
            sweep: direction * std::f64::consts::TAU,
        });
        let batch = Batches::new(bounds(), &[circle], &[], &[], &[]);
        let image = batch.render();
        assert_eq!(batch.pixel(&image, Point::new(2.0, 5.0)), PALETTE.copper);
    }
}

#[test]
fn high_residual_coordinates_render_the_same_local_geometry() {
    let geometry = line(1, Point::new(2.0, 2.0), Point::new(8.0, 8.0), 0.7);
    let original = Batches::new(bounds(), std::slice::from_ref(&geometry), &[], &[], &[]);
    let offset = 1_000_000.0;
    let shift = |at: Point| Point::new(at.x + offset, at.y + offset);
    let mut moved = geometry;
    moved.a = shift(moved.a);
    moved.b = shift(moved.b);
    let translated = Batches::new(
        Bounds {
            min: shift(bounds().min),
            max: shift(bounds().max),
        },
        &[moved],
        &[],
        &[],
        &[],
    );
    assert_eq!(original.render().rgba, translated.render().rgba);
}

#[test]
fn cancelled_generation_has_no_preview_and_invalid_bounds_do_not_allocate_one() {
    let mut batch = Batches::new(bounds(), &[], &[], &[], &[]);
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        rasterize(batch.source(), PALETTE, Limits::default(), &cancel),
        Err(PrepareError::Cancelled)
    ));
    batch.bounds.min.x = f64::NAN;
    assert!(
        rasterize(
            batch.source(),
            PALETTE,
            Limits::default(),
            &CancellationToken::default()
        )
        .unwrap()
        .is_none()
    );
}

#[test]
#[ignore = "requires POMELO_PREVIEW_BOARD_PATH; writes only POMELO_PREVIEW_PNG when supplied"]
fn external_board_thumbnail() {
    use pomelo_import::{
        BoardImporter, ImportContext, ImportOptions, formats::allegro::AllegroImporter,
    };
    let path = std::path::PathBuf::from(
        std::env::var_os("POMELO_PREVIEW_BOARD_PATH").expect("external board path"),
    );
    let cancel = CancellationToken::default();
    let imported = AllegroImporter
        .import(
            &path,
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let scene = &imported.scene;
    let mut batch = Batches::new(
        scene.bounds,
        &scene.segments,
        &scene.pins,
        &scene.vias,
        &scene.zones,
    );
    batch.tracks = PreparedTracks::build_with_outline(
        &scene.segments,
        &scene.outline,
        TraceLimits::default(),
        &cancel,
    )
    .unwrap();
    batch.drawings =
        PreparedTracks::build_drawings(&scene.drawings, TraceLimits::default(), &cancel).unwrap();
    let started = std::time::Instant::now();
    let image = batch.render();
    println!(
        "thumbnail_ms={} summary={:?}",
        started.elapsed().as_millis(),
        image.summary
    );
    assert!(
        image
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[..3] != PALETTE.background)
    );
    if let Some(path) = std::env::var_os("POMELO_PREVIEW_PNG") {
        image::save_buffer_with_format(
            path,
            &image.rgba,
            WIDTH as u32,
            HEIGHT as u32,
            image::ColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .unwrap();
    }
}
