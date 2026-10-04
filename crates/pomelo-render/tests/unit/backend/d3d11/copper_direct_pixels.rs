//! Compare private stencil coverage with direct, source-ordered exterior draws.
use super::*;
use crate::backend::common::copper::{CopperDrawOptions, Pipeline, UploadedCopper};
use crate::copper::{CopperLimits, PreparedCopper};
use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    model::{Zone, ZoneKind},
};
use std::collections::BTreeSet;

#[test]
#[ignore = "requires hardware D3D11; direct copper versus original stencil pixel oracle"]
fn hardware_direct_copper_matches_stencil_materials_holes_annotations_and_edges() {
    let (device, context) = compositor_parity::device();
    let mut gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    let mut pipeline = Pipeline::new(&gpu.device).unwrap();
    let probe = crate::backend::common::probe::Pipeline::new(&gpu.device).unwrap();
    let cancel = CancellationToken::default();
    for offset in [0.0, 9_000_000_000.125] {
        let rectangle = |x: f64, y: f64, w: f64, h: f64| {
            vec![
                Point::new(offset + x, offset + y),
                Point::new(offset + x + w, offset + y),
                Point::new(offset + x + w, offset + y + h),
                Point::new(offset + x, offset + y + h),
            ]
        };
        let zones: Vec<_> = (0..7)
            .map(|i| {
                let mut rings = vec![rectangle(-7.0 + i as f64, -5.0 + i as f64, 10.0, 8.0)];
                if i == 1 {
                    rings.extend([
                        rectangle(-2.0, -2.0, 2.0, 2.0),
                        rectangle(-1.0, -1.0, 2.0, 2.0),
                    ]);
                }
                Zone {
                    id: ObjectId(i + 1),
                    layer: LayerId(0),
                    net: NetId(if i >= 4 { 2 } else { i % 2 + 1 }),
                    kind: if i % 2 == 0 {
                        ZoneKind::Static
                    } else {
                        ZoneKind::Dynamic
                    },
                    paths: vec![],
                    mesh: CopperMesh::build(&rings, &[], &MeshLimits::default(), &cancel).unwrap(),
                }
            })
            .collect();
        let source =
            Arc::new(PreparedCopper::build(&zones, CopperLimits::default(), &cancel).unwrap());
        let mut optimized = UploadedCopper::new(Arc::clone(&source), &gpu.device).unwrap();
        let mut reference = UploadedCopper::new(source, &gpu.device).unwrap();
        reference.force_stencil = true;
        for cache in [&mut optimized, &mut reference] {
            let mut budget = usize::MAX;
            cache.upload_with_budget(&gpu, &mut budget).unwrap();
        }
        let owners = BTreeSet::from([ObjectId(3)]);
        for scale in [1.0, 2.0] {
            for flipped in [false, true] {
                let mut frame = compositor_parity::fixture().traces;
                frame.camera = Some(Camera {
                    center: Point::new(offset, offset),
                    pixels_per_mm: 10.0 / f64::from(scale),
                    flipped,
                });
                frame.scale_factor = scale;
                frame.opacity = 0.37;
                frame.highlighted_net = Some((NetId(1), [0.2, 0.8, 0.6, 0.73]));
                frame.colors = Arc::new(BTreeMap::from([(LayerId(0), [0.9, 0.3, 0.1, 0.53])]));
                gpu.content_mask.bounds = ViewBounds::new(
                    point(ScaledPixels(7.25), ScaledPixels(11.5)),
                    size(ScaledPixels(106.5), ScaledPixels(101.25)),
                );
                for solid in [false, true] {
                    for opacity in [0.0, 0.25, 1.0] {
                        for pass in [
                            OverlayPass::Base,
                            OverlayPass::Selection,
                            OverlayPass::Hover,
                        ] {
                            frame.pass = pass;
                            frame.hovered_object = Some((
                                pomelo_core::selection::SelectedObject::Zone(ObjectId(4)),
                                [1.0; 4],
                            ));
                            let mut results = Vec::new();
                            for cache in [&reference, &optimized] {
                                target.bind(&context);
                                let mut seen = Vec::new();
                                let mut annotate = |batch: &crate::copper::CopperBatch| {
                                    seen.push(batch.object);
                                    probe
                                        .draw(&gpu, &crate::backend::ProbeScene::triangle(1))
                                        .map(|_| ())
                                };
                                let counts = pipeline
                                    .draw(
                                        &gpu,
                                        &frame,
                                        cache,
                                        CopperDrawOptions {
                                            opacity,
                                            layer: None,
                                            visible: None,
                                            annotations: Some(&mut annotate),
                                            annotation_owners: Some(&owners),
                                            overrides: None,
                                            static_shapes_fill_solid: solid,
                                            network_selection: Some(NetId(1)),
                                        },
                                    )
                                    .unwrap();
                                results.push((target.read(&context), counts, seen));
                            }
                            assert_eq!(
                                results[0].0, results[1].0,
                                "offset={offset} dpi={scale} flip={flipped} solid={solid} alpha={opacity} pass={pass:?}"
                            );
                            assert_eq!(results[0].2, results[1].2, "annotation order changed");
                            assert_eq!(
                                results[0].1.1, results[1].1.1,
                                "visible zone count changed"
                            );
                            if opacity > 0.0 && pass == OverlayPass::Base {
                                assert!(
                                    results[1].1.0 < results[0].1.0,
                                    "fast path did not reduce draws"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
