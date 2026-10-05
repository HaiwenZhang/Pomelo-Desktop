use super::*;

#[test]
#[ignore = "requires a hardware D3D11 adapter; scoped binding restoration"]
fn hardware_pad_draw_scope_restores_output_on_success_and_error() {
    use crate::backend::common::pad::{Pipeline, UploadedPads};
    let (device, context) = compositor_parity::device();
    let gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    let board = compositor_parity::fixture();
    let pipeline = Pipeline::new(&gpu.device).unwrap();
    let mut pads = UploadedPads::new(Arc::clone(board.pads.as_ref().unwrap())).unwrap();
    let mut budget = usize::MAX;
    while pads.uploaded() < pads.source.analytic.len() {
        pads.upload_next(&gpu.device, &mut budget).unwrap();
    }
    let output = || {
        let mut targets = [None];
        let mut depth = None;
        let mut state = None;
        let mut reference = 0;
        let mut vertex_views = [None];
        let mut pixel_views = [None];
        // SAFETY: test-owned context returns retained handles for equality checks.
        unsafe {
            context.OMGetRenderTargets(Some(&mut targets), Some(&mut depth));
            context.OMGetDepthStencilState(Some(&mut state), Some(&mut reference));
            context.VSGetShaderResources(0, Some(&mut vertex_views));
            context.PSGetShaderResources(0, Some(&mut pixel_views));
        }
        (targets, depth, state, reference, vertex_views, pixel_views)
    };
    let mut sentinel = None;
    // SAFETY: valid descriptor; sentinel disables depth but retains a nonzero reference.
    unsafe {
        device
            .CreateDepthStencilState(
                &D3D11_DEPTH_STENCIL_DESC {
                    DepthWriteMask: D3D11_DEPTH_WRITE_MASK_ZERO,
                    DepthFunc: D3D11_COMPARISON_ALWAYS,
                    ..Default::default()
                },
                Some(&mut sentinel),
            )
            .unwrap();
    }
    for fail in [false, true] {
        target.bind(&context);
        // SAFETY: live same-device sentinel used to detect restoration, including its reference.
        unsafe {
            context.OMSetDepthStencilState(sentinel.as_ref(), 37);
        }
        let before = output();
        let result = (|| -> anyhow::Result<()> {
            let _scope = gpu.device.draw_scope();
            pipeline.draw(&gpu, &board.traces, &pads, None, None)?;
            if fail {
                anyhow::bail!("TEST_AFTER_DRAW_ERROR");
            }
            Ok(())
        })();
        assert_eq!(result.is_err(), fail);
        assert_eq!(output(), before, "restore bindings after fail={fail}");
    }
}

fn split_xy(x: f64, y: f64) -> [f32; 4] {
    let [x, dx] = crate::split_position(x);
    let [y, dy] = crate::split_position(y);
    [x, y, dx, dy]
}

#[test]
#[ignore = "requires a hardware D3D11 adapter; ordered range culling pixel oracle"]
fn hardware_pad_ranges_match_unculled_edges_residuals_and_overlays() {
    use crate::backend::OverlayPass;
    use crate::backend::common::pad::{Pipeline, UploadedPads, Visibility};
    use pomelo_core::selection::{SelectedObject, SelectionTarget};
    let (device, context) = compositor_parity::device();
    let gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    let pipeline = Pipeline::new(&gpu.device).unwrap();
    for offset in [0.0, 9_000_000_000.125] {
        let mut board = compositor_parity::fixture();
        let mut pads = Arc::try_unwrap(board.pads.take().unwrap()).unwrap();
        let template = pads.analytic[0];
        pads.analytic = (0..4097)
            .map(|i| {
                let mut pad = template;
                let local_x = if i < 768 && (i / 8) % 3 != 1 {
                    [-6.9, -6.75, -6.6, 0.0, 6.6, 6.75, 6.9, 0.1][i % 8]
                } else {
                    100.0 + (i % 200) as f64
                };
                let local_y = (i % 17) as f64 * 0.3 - 2.4;
                pad.center = split_xy(offset + local_x, offset + local_y);
                pad.bounds_min = split_xy(offset + local_x - 0.4, offset + local_y - 0.4);
                pad.bounds_max = split_xy(offset + local_x + 0.4, offset + local_y + 0.4);
                pad.shape = [0.8, 0.8, 0.0, 0.0];
                pad.ids = [i as u32, 0, (i % 3 + 1) as u32, 2];
                pad.source[0] = (i % 2) as u32;
                pad
            })
            .collect();
        pads.batches = vec![crate::pads::PadBatch {
            layer: LayerId(0),
            start: 0,
            count: pads.analytic.len() as u32,
        }];
        let pads = Arc::new(pads);
        let mut culled = UploadedPads::new(Arc::clone(&pads)).unwrap();
        let mut reference = UploadedPads::new(pads).unwrap();
        for cache in [&mut culled, &mut reference] {
            let mut budget = usize::MAX;
            while cache.uploaded() < cache.source.analytic.len() {
                cache.upload_next(&gpu.device, &mut budget).unwrap();
            }
        }
        reference.disable_range_culling().unwrap();
        reference.use_cpu_net_selection();
        reference.legacy_visibility = true;
        for scale in [1.0, 2.0] {
            for flipped in [false, true] {
                let mut frame = board.traces.clone();
                frame.bounds = Bounds {
                    min: Point::new(offset - 400.0, offset - 400.0),
                    max: Point::new(offset + 400.0, offset + 400.0),
                };
                frame.camera = Some(Camera {
                    center: Point::new(offset, offset),
                    pixels_per_mm: 10.0 / f64::from(scale),
                    flipped,
                });
                frame.scale_factor = scale;
                frame.opacity = 0.4;
                frame.hover_selection =
                    Some((SelectionTarget::Net(NetId(2)), Arc::new(Default::default())));
                for net in [1, 2, 1, 0] {
                    frame.highlighted_net = Some((NetId(net), [1.0; 4]));
                    for selected_object in
                        [None, Some((SelectedObject::Pin(ObjectId(3)), [1.0; 4]))]
                    {
                        frame.highlighted_object = selected_object;
                        for filled in [false, true] {
                            frame.filled = filled;
                            for pass in [
                                OverlayPass::Base,
                                OverlayPass::Selection,
                                OverlayPass::Hover,
                            ] {
                                frame.pass = pass;
                                for pin_only in [false, true] {
                                    let visible = |pad: &crate::pads::PadInstance| {
                                        !pin_only || pad.source[0] == 0
                                    };
                                    target.bind(&context);
                                    let expected_draws = pipeline
                                        .draw(&gpu, &frame, &reference, None, Some(&visible))
                                        .unwrap();
                                    let expected = target.read(&context);
                                    target.bind(&context);
                                    let actual_draws = pipeline
                                        .draw_scoped(
                                            &gpu,
                                            &frame,
                                            &culled,
                                            None,
                                            if pin_only {
                                                Visibility::Pads {
                                                    pin: true,
                                                    show_backdrills: false,
                                                }
                                            } else {
                                                Visibility::All
                                            },
                                        )
                                        .unwrap();
                                    assert_eq!(
                                        target.read(&context),
                                        expected,
                                        "offset={offset} scale={scale} flipped={flipped} filled={filled} pass={} pin={pin_only}",
                                        pass as u8
                                    );
                                    if selected_object.is_none()
                                        && pass == OverlayPass::Selection
                                        && !pin_only
                                        && net != 0
                                    {
                                        assert!(actual_draws < expected_draws);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "requires hardware D3D11; cached pad and drill visibility pixel oracle"]
fn hardware_cached_pad_visibility_matches_predicates_across_display_and_upload_changes() {
    use crate::backend::common::pad::{Pipeline, UploadedPads, Visibility};
    use pomelo_core::display::{BoardDisplay, LayerPrimitive};
    let (device, context) = compositor_parity::device();
    let gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    let pipeline = Pipeline::new(&gpu.device).unwrap();
    let mut board = compositor_parity::fixture();
    let mut pads = Arc::try_unwrap(board.pads.take().unwrap()).unwrap();
    let template = pads.analytic[0];
    pads.analytic = (0..16385)
        .map(|i| {
            let mut pad = template;
            let x = (i % 21) as f64 * 0.6 - 6.0;
            let y = (i % 17) as f64 * 0.6 - 4.8;
            pad.center = split_xy(x, y);
            pad.bounds_min = split_xy(x - 0.2, y - 0.2);
            pad.bounds_max = split_xy(x + 0.2, y + 0.2);
            pad.shape = [0.4, 0.4, 0.0, 0.0];
            pad.ids = [i as u32, 0, (i % 3 + 1) as u32, 2];
            pad.source = [
                (i % 2) as u32,
                (i % 5) as u32,
                0,
                if i % 7 == 0 {
                    4
                } else if i % 3 == 0 {
                    2
                } else {
                    0
                },
            ];
            pad
        })
        .collect();
    pads.batches = vec![crate::pads::PadBatch {
        layer: LayerId(0),
        start: 0,
        count: pads.analytic.len() as u32,
    }];
    pads.drill_scopes = Some(Arc::new(vec![
        vec![],
        vec![LayerId(0)],
        vec![LayerId(0), LayerId(2)],
    ]));
    pads.backdrill_scopes = Some(Arc::new(vec![vec![LayerId(2)], vec![], vec![LayerId(0)]]));
    let source = Arc::new(pads);
    let mut reference = UploadedPads::new(Arc::clone(&source)).unwrap();
    reference.legacy_visibility = true;
    let mut cached = UploadedPads::new(Arc::clone(&source)).unwrap();
    let mut display = Arc::new(BoardDisplay::default());
    let mut frame = board.traces;
    frame.opacity = 0.4;
    // Test partial uploads, later chunks, and cache reuse without camera keys.
    for uploaded_chunks in [1, 2] {
        for cache in [&mut cached, &mut reference] {
            let mut budget = if uploaded_chunks == 1 {
                16384 * size_of::<crate::pads::PadInstance>()
            } else {
                usize::MAX
            };
            cache.upload_next(&gpu.device, &mut budget).unwrap();
        }
        for state in 0..6 {
            frame.color_mode = if state % 2 == 0 {
                pomelo_core::display::ColorMode::Net
            } else {
                pomelo_core::display::ColorMode::Layer
            };
            let settings = Arc::make_mut(&mut display);
            settings.hidden_layers.clear();
            settings.layer_primitives.clear();
            settings.show_drills = state != 3;
            settings.show_backdrills = state != 4;
            if state == 1 {
                settings.hidden_layers.insert(LayerId(0));
            }
            if state == 2 {
                settings.set_primitive(LayerId(2), LayerPrimitive::Vias, false);
            }
            for zoom in [1.0, 8.0] {
                frame.camera.as_mut().unwrap().pixels_per_mm = 10.0 * zoom;
                for net in [None, Some((NetId(2), [1.0; 4]))] {
                    frame.highlighted_net = net;
                    for pass in [
                        OverlayPass::Base,
                        OverlayPass::Selection,
                        OverlayPass::Hover,
                    ] {
                        frame.pass = pass;
                        for filter in 0..5 {
                            let scope = if filter == 4 {
                                Visibility::Drills(&display)
                            } else {
                                Visibility::Pads {
                                    pin: filter % 2 == 0,
                                    show_backdrills: filter >= 2,
                                }
                            };
                            let predicate = |pad: &crate::pads::PadInstance| {
                                if filter < 4 {
                                    return (pad.source[0] == 0) == (filter % 2 == 0)
                                        && (pad.source[3] & 4 == 0 || filter < 2);
                                }
                                let backdrill = pad.source[3] & 2 != 0;
                                let scopes = if backdrill {
                                    &source.backdrill_scopes
                                } else {
                                    &source.drill_scopes
                                };
                                let scope = if pad.source[0] == 0 {
                                    None
                                } else {
                                    Some(
                                        scopes
                                            .as_ref()
                                            .and_then(|scopes| scopes.get(pad.source[1] as usize))
                                            .map_or(&[][..], Vec::as_slice),
                                    )
                                };
                                if backdrill {
                                    display.backdrill_visible(scope.unwrap_or(&[]))
                                } else {
                                    display.drill_visible(scope)
                                }
                            };
                            target.bind(&context);
                            let reference_draws = pipeline
                                .draw(&gpu, &frame, &reference, None, Some(&predicate))
                                .unwrap();
                            let expected = target.read(&context);
                            target.bind(&context);
                            let cached_draws = pipeline
                                .draw_scoped(&gpu, &frame, &cached, None, scope)
                                .unwrap();
                            assert_eq!(
                                target.read(&context),
                                expected,
                                "chunks={uploaded_chunks} state={state} zoom={zoom} net={net:?} pass={pass:?} filter={filter}"
                            );
                            if pass == OverlayPass::Base && filter < 4 && zoom == 1.0 {
                                assert!(
                                    cached_draws < reference_draws,
                                    "chunks={uploaded_chunks} state={state} net={net:?} filter={filter}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
#[test]
#[ignore = "requires hardware D3D11; cached overlay membership invalidation oracle"]
fn hardware_pad_overlay_cache_matches_original_after_selection_and_hover_mutation() {
    use crate::backend::common::pad::{Pipeline, UploadedPads, Visibility};
    use crate::backend::{OverlayPass, TraceSelection};
    use pomelo_core::selection::{SelectedObject, SelectionTarget};
    use std::collections::BTreeSet;
    let (device, context) = compositor_parity::device();
    let gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    let pipeline = Pipeline::new(&gpu.device).unwrap();
    let mut board = compositor_parity::fixture();
    let mut pads = Arc::try_unwrap(board.pads.take().unwrap()).unwrap();
    let template = pads.analytic[0];
    pads.analytic = (0..16385)
        .map(|i| {
            let mut pad = template;
            let x = (i % 200) as f64 * 0.5 - 50.0;
            let y = (i / 200) as f64 * 0.5 - 20.0;
            pad.center = split_xy(x, y);
            pad.bounds_min = split_xy(x - 0.4, y - 0.4);
            pad.bounds_max = split_xy(x + 0.4, y + 0.4);
            pad.shape = [0.8, 0.8, 0.0, 0.0];
            pad.ids = [i as u32, 0, (i % 3 + 1) as u32, 2];
            pad.source[0] = (i % 2) as u32;
            pad
        })
        .collect();
    pads.batches = vec![crate::pads::PadBatch {
        layer: LayerId(0),
        start: 0,
        count: 16385,
    }];
    let pads = Arc::new(pads);
    let mut cached = UploadedPads::new(Arc::clone(&pads)).unwrap();
    let mut reference = UploadedPads::new(pads).unwrap();
    reference.legacy_visibility = true;
    reference.use_cpu_net_selection();
    let mut frame = board.traces;
    frame.bounds = Bounds {
        min: Point::new(-100.0, -100.0),
        max: Point::new(100.0, 100.0),
    };
    frame.camera = Some(Camera {
        center: Point::new(0.0, 0.0),
        pixels_per_mm: 10.0,
        flipped: false,
    });
    for chunk in 0..2 {
        for cache in [&mut cached, &mut reference] {
            let mut budget = 16384 * size_of::<crate::pads::PadInstance>();
            cache.upload_next(&gpu.device, &mut budget).unwrap();
        }
        frame.hover_selection = Some((SelectionTarget::Net(NetId(2)), Arc::new(BTreeSet::new())));
        frame.highlighted_net = None;
        frame.highlighted_objects = None;
        frame.highlighted_related_objects = None;
        frame.highlighted_object = None;
        frame.highlighted_trace = None;
        frame.hovered_object = None;
        for state in 0..22 {
            match state {
                1 => {
                    frame.hover_selection = Some((
                        SelectionTarget::Component(ObjectId(1)),
                        Arc::new(BTreeSet::from([
                            SelectedObject::Pin(ObjectId(8100)),
                            SelectedObject::Via(ObjectId(8101)),
                        ])),
                    ))
                }
                2 => {
                    Arc::make_mut(&mut frame.hover_selection.as_mut().unwrap().1)
                        .insert(SelectedObject::Via(ObjectId(8103)));
                }
                3 => {
                    frame.highlighted_related_objects = Some((
                        Arc::new(BTreeSet::from([SelectedObject::Via(ObjectId(8101))])),
                        [1.0; 4],
                    ))
                }
                4 => {
                    Arc::make_mut(&mut frame.highlighted_related_objects.as_mut().unwrap().0)
                        .insert(SelectedObject::Via(ObjectId(8103)));
                }
                5 => {
                    frame.highlighted_objects =
                        Some((Arc::new(BTreeSet::from([ObjectId(8100)])), [1.0; 4]))
                }
                6 => {
                    Arc::make_mut(&mut frame.highlighted_objects.as_mut().unwrap().0)
                        .insert(ObjectId(8102));
                }
                7 => frame.hovered_object = Some((SelectedObject::Pin(ObjectId(8102)), [0.5; 4])),
                8 => {
                    frame.hover_selection = Some((
                        SelectionTarget::Track(ObjectId(0)),
                        Arc::new(BTreeSet::new()),
                    ))
                }
                9 => frame.highlighted_trace = Some((TraceSelection::Track(ObjectId(0)), [1.0; 4])),
                10 => {
                    frame.highlighted_object = Some((SelectedObject::Pin(ObjectId(8102)), [1.0; 4]))
                }
                11 => {
                    frame.highlighted_net = Some((NetId(2), [1.0; 4]));
                    frame.hover_selection =
                        Some((SelectionTarget::Net(NetId(2)), Arc::new(BTreeSet::new())));
                }
                12 => {
                    frame.hover_selection = None;
                    frame.highlighted_object = None;
                    frame.highlighted_related_objects = None;
                    frame.highlighted_objects = None;
                    frame.highlighted_trace = None;
                    frame.hovered_object = None;
                    frame.highlighted_net = Some((NetId(0), [1.0; 4]));
                }
                13 => {
                    frame.highlighted_net = None;
                    frame.hover_selection =
                        Some((SelectionTarget::Net(NetId(2)), Arc::new(BTreeSet::new())));
                    frame.hovered_object = Some((SelectedObject::Via(ObjectId(8101)), [0.5; 4]));
                }
                14 => frame.hovered_object = Some((SelectedObject::Pin(ObjectId(8100)), [0.5; 4])),
                15 => frame.highlighted_net = Some((NetId(1), [1.0; 4])),
                16 => {
                    frame.highlighted_net = None;
                    frame.hover_selection =
                        Some((SelectionTarget::Net(NetId(0)), Arc::new(BTreeSet::new())));
                }
                17 => {
                    frame.hover_selection =
                        Some((SelectionTarget::Net(NetId(9)), Arc::new(BTreeSet::new())))
                }
                18 => frame.hovered_object = None,
                19 => {
                    frame.highlighted_net = Some((NetId(1), [1.0; 4]));
                    frame.hover_selection =
                        Some((SelectionTarget::Net(NetId(2)), Arc::new(BTreeSet::new())));
                    frame.hovered_object = Some((SelectedObject::Via(ObjectId(8101)), [0.5; 4]));
                }
                20 => {
                    frame.hover_selection =
                        Some((SelectionTarget::Net(NetId(1)), Arc::new(BTreeSet::new())))
                }
                21 => {
                    frame.hover_selection =
                        Some((SelectionTarget::Net(NetId(2)), Arc::new(BTreeSet::new())));
                    frame.highlighted_trace = Some((TraceSelection::Track(ObjectId(0)), [1.0; 4]));
                }
                _ => {}
            }
            for pass in [
                OverlayPass::Selection,
                OverlayPass::Hover,
                OverlayPass::GroupHover,
            ] {
                frame.pass = pass;
                for repeat in 0..2 {
                    for filter in 0..3 {
                        let scope = if filter == 0 {
                            Visibility::All
                        } else {
                            Visibility::Pads {
                                pin: filter == 1,
                                show_backdrills: false,
                            }
                        };
                        target.bind(&context);
                        let reference_draws = pipeline
                            .draw_scoped(&gpu, &frame, &reference, None, scope)
                            .unwrap();
                        let expected = target.read(&context);
                        target.bind(&context);
                        let cached_draws = pipeline
                            .draw_scoped(&gpu, &frame, &cached, None, scope)
                            .unwrap();
                        assert_eq!(
                            target.read(&context),
                            expected,
                            "chunk={chunk} state={state} pass={pass:?} repeat={repeat} filter={filter}"
                        );
                        if matches!(state, 0 | 13 | 19)
                            && pass != OverlayPass::Selection
                            && filter == 0
                        {
                            assert!(
                                cached_draws < reference_draws,
                                "dense net hover must reduce submissions"
                            );
                        }
                    }
                }
            }
        }
    }
}
