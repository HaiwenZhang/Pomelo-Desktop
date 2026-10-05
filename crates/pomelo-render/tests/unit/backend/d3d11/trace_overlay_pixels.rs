use super::*;

#[test]
#[ignore = "requires hardware D3D11; cached trace overlay invalidation oracle"]
fn hardware_trace_overlay_matches_scalar_after_mutation_and_partial_residency() {
    use crate::backend::board::TraceScope;
    use crate::backend::common::trace::{Pipeline, UploadedTracks};
    use crate::backend::{OverlayPass, TraceSelection};
    use pomelo_core::selection::{SelectedObject, SelectionTarget};
    use std::collections::BTreeSet;
    let (device, context) = compositor_parity::device();
    let gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    let pipeline = Pipeline::new(&gpu.device).unwrap();
    let mut frame = compositor_parity::fixture().traces;
    let template = frame.tracks.instances[0];
    for owner_flags in [None, Some(8), Some(0)] {
        let source = Arc::new(PreparedTracks {
            instances: (0..16385)
                .map(|i| {
                    let mut line = template;
                    let x = (i % 200) as f64 * 0.5 - 50.0;
                    let y = (i / 200) as f64 * 0.5 - 20.0;
                    let split = |x, y| {
                        let [x, dx] = crate::split_position(x);
                        let [y, dy] = crate::split_position(y);
                        [x, y, dx, dy]
                    };
                    line.a = split(x - 0.15, y);
                    line.b = split(x + 0.15, y);
                    line.bounds_min = split(x - 0.2, y - 0.05);
                    line.bounds_max = split(x + 0.2, y + 0.05);
                    line.ids = [i, i % 4, u32::from(i >= 8192), i % 3 + 1];
                    line.flags = [
                        0,
                        0,
                        0.1_f32.to_bits(),
                        owner_flags.unwrap_or(match i % 5 {
                            0 => 0,
                            1 => 8,
                            2 => 16,
                            3 => 32,
                            _ => 64,
                        }),
                    ];
                    line
                })
                .collect(),
            batches: vec![
                crate::tracks::TraceBatch {
                    layer: LayerId(0),
                    start: 0,
                    count: 8192,
                    outline: false,
                },
                crate::tracks::TraceBatch {
                    layer: LayerId(1),
                    start: 8192,
                    count: 8192,
                    outline: false,
                },
                crate::tracks::TraceBatch {
                    layer: LayerId(1),
                    start: 16384,
                    count: 1,
                    outline: true,
                },
            ],
            memory_reservation: None,
        });
        frame.tracks = Arc::clone(&source);
        frame.bounds = Bounds {
            min: Point::new(-100.0, -100.0),
            max: Point::new(100.0, 100.0),
        };
        frame.camera = Some(Camera {
            center: Point::default(),
            pixels_per_mm: 10.0,
            flipped: false,
        });
        for residency in [false, true] {
            let mut cached = UploadedTracks::new(Arc::clone(&source)).unwrap();
            let mut scalar = UploadedTracks::new(Arc::clone(&source)).unwrap();
            scalar.legacy_overlays = true;
            if residency {
                for cache in [&mut cached, &mut scalar] {
                    cache.enable_residency(8 * 1024 * 1024).unwrap();
                }
            }
            for upload in 0..2 {
                for cache in [&mut cached, &mut scalar] {
                    let mut budget = 16384 * size_of::<crate::tracks::TraceInstance>();
                    cache
                        .upload_visible(&gpu.device, None, &mut budget)
                        .unwrap();
                }
                frame.highlighted_object = None;
                frame.highlighted_net = None;
                frame.highlighted_trace = None;
                frame.highlighted_objects = None;
                frame.highlighted_related_objects = None;
                frame.hovered_object = Some((SelectedObject::Via(ObjectId(8101)), [0.5; 4]));
                frame.hover_selection =
                    Some((SelectionTarget::Net(NetId(2)), Arc::new(BTreeSet::new())));
                for state in 0..17 {
                    match state {
                        1 => frame.highlighted_net = Some((NetId(2), [1.0; 4])),
                        2 => {
                            frame.highlighted_net = None;
                            frame.highlighted_related_objects = Some((
                                Arc::new(BTreeSet::from([SelectedObject::Segment(ObjectId(8100))])),
                                [1.0; 4],
                            ));
                        }
                        3 => {
                            Arc::make_mut(
                                &mut frame.highlighted_related_objects.as_mut().unwrap().0,
                            )
                            .insert(SelectedObject::Zone(ObjectId(8101)));
                        }
                        4 => {
                            frame.hover_selection = Some((
                                SelectionTarget::Component(ObjectId(1)),
                                Arc::new(BTreeSet::from([SelectedObject::Pin(ObjectId(8102))])),
                            ))
                        }
                        5 => {
                            Arc::make_mut(&mut frame.hover_selection.as_mut().unwrap().1)
                                .insert(SelectedObject::Via(ObjectId(8103)));
                        }
                        6 => {
                            frame.highlighted_objects =
                                Some((Arc::new(BTreeSet::from([ObjectId(8102)])), [1.0; 4]))
                        }
                        7 => {
                            frame.hovered_object =
                                Some((SelectedObject::Drawing(ObjectId(8104)), [0.5; 4]))
                        }
                        8 => {
                            frame.highlighted_trace =
                                Some((TraceSelection::Track(ObjectId(1)), [1.0; 4]))
                        }
                        9 => {
                            frame.highlighted_object =
                                Some((SelectedObject::Zone(ObjectId(8101)), [1.0; 4]))
                        }
                        10 => {
                            frame.highlighted_object = None;
                            frame.highlighted_net = None;
                            frame.highlighted_trace = None;
                            frame.highlighted_objects = None;
                            frame.highlighted_related_objects = None;
                            frame.hover_selection =
                                Some((SelectionTarget::Net(NetId(2)), Arc::new(BTreeSet::new())));
                            frame.hovered_object =
                                Some((SelectedObject::Pin(ObjectId(8102)), [0.5; 4]));
                        }
                        11 => {
                            frame.hovered_object =
                                Some((SelectedObject::Zone(ObjectId(8101)), [0.5; 4]))
                        }
                        12 => {
                            frame.hover_selection =
                                Some((SelectionTarget::Net(NetId(9)), Arc::new(BTreeSet::new())))
                        }
                        13 => {
                            frame.highlighted_net = Some((NetId(1), [1.0; 4]));
                            frame.hover_selection =
                                Some((SelectionTarget::Net(NetId(2)), Arc::new(BTreeSet::new())));
                            frame.hovered_object =
                                Some((SelectedObject::Via(ObjectId(8101)), [0.5; 4]));
                        }
                        14 => {
                            frame.hover_selection =
                                Some((SelectionTarget::Net(NetId(1)), Arc::new(BTreeSet::new())))
                        }
                        15 => frame.highlighted_net = Some((NetId(0), [1.0; 4])),
                        16 => {
                            frame.highlighted_related_objects = Some((
                                Arc::new(BTreeSet::from([SelectedObject::Zone(ObjectId(8101))])),
                                [1.0; 4],
                            ))
                        }
                        _ => {}
                    }
                    for pass in [
                        OverlayPass::Selection,
                        OverlayPass::Hover,
                        OverlayPass::GroupHover,
                    ] {
                        frame.pass = pass;
                        for scope in [
                            TraceScope::All,
                            TraceScope::Layer(LayerId(0)),
                            TraceScope::ZoneOutlines(LayerId(1)),
                            TraceScope::Pads(LayerId(0), true),
                            TraceScope::Pads(LayerId(0), false),
                            TraceScope::Outline,
                        ] {
                            for repeat in 0..2 {
                                if repeat == 1
                                    && let Some((_, members)) = frame.hover_selection.as_mut()
                                {
                                    *members = Arc::new((**members).clone());
                                }
                                target.bind(&context);
                                let expected_draws =
                                    pipeline.draw(&gpu, &frame, &scalar, scope).unwrap();
                                let expected = target.read(&context);
                                target.bind(&context);
                                let actual_draws =
                                    pipeline.draw(&gpu, &frame, &cached, scope).unwrap();
                                assert_eq!(
                                    target.read(&context),
                                    expected,
                                    "residency={residency} upload={upload} state={state} pass={pass:?} repeat={repeat}"
                                );
                                assert_eq!(
                                    actual_draws, expected_draws,
                                    "ordered span boundaries changed"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
