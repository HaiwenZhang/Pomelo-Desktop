//! Die pads use etch visibility and priority while retaining their pin identity.
use super::*;
use crate::backend::{BoardFrame, BoardRenderer, CopperTelemetry};
use pomelo_core::{
    display::{BoardDisplay, DisplayCategory, LayerPrimitives, LayerPriority},
    interaction::Camera,
    model::{CustomPadGeometry, DiePad, DrillShape, LayerId, Pad, PadKind, Pin},
};

#[test]
#[ignore = "requires a Windows hardware D3D11 adapter; diagnostic readback only"]
fn hardware_die_pads_follow_etch_visibility_and_priority_without_reupload() {
    let (mut device, mut context) = (None, None);
    // SAFETY: initialized output slots; only a physical D3D11 adapter is requested.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .unwrap();
    }
    let (device, context) = (device.unwrap(), context.unwrap());
    let target = Target::new(&device);
    let bounds = ViewBounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(SIDE as f32), ScaledPixels(SIDE as f32)),
    );
    let gpu = NativeGpuContext {
        device: crate::backend::d3d11::Device::new(&device, &context, 1.0),
        viewport: [SIDE as f32; 2],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let cancel = CancellationToken::default();
    let mut cases = Vec::new();
    let mut failures = Vec::new();
    let mut caches = Vec::new();
    for geometry in ["circle", "rectangle", "custom-hole"] {
        let mut die_pad = Pad::circle(LayerId::BOND_TOP, 4.0);
        if geometry == "rectangle" {
            die_pad.kind = PadKind(5);
        }
        if geometry == "custom-hole" {
            let square = |r: f64| {
                vec![
                    Point::new(-r, -r),
                    Point::new(r, -r),
                    Point::new(r, r),
                    Point::new(-r, r),
                ]
            };
            die_pad.kind = PadKind::CUSTOM;
            die_pad.custom = Some(Arc::new(CustomPadGeometry {
                contours: vec![square(2.0), square(0.5)],
                paths: vec![],
            }));
        }
        let die = Pin {
            id: ObjectId(50),
            owner_id: ObjectId(49),
            net: NetId(1),
            name: String::new(),
            reference: String::new(),
            at: Point::default(),
            angle: 0.0,
            mirrored: false,
            drill: 0.0,
            drill_shape: DrillShape {
                width: 0.0,
                height: 0.0,
                plated: false,
            },
            pads: vec![die_pad],
            stackup_region: None,
            die: Some(DiePad {
                source_reference: ObjectId(51),
                padstack_name: String::new(),
            }),
        };
        let ordinary = Pin {
            id: ObjectId(60),
            pads: vec![Pad::circle(LayerId(1), 6.0)],
            die: None,
            ..die.clone()
        };
        let mut pads = crate::pads::PreparedPads::build(
            &[die, ordinary],
            &[],
            crate::pads::PadLimits::default(),
            &cancel,
        )
        .unwrap();
        if !pads.custom.is_empty() {
            pads.custom_mesh = Some(Arc::new(
                pads.build_custom_meshes(
                    crate::copper::CopperLimits::default(),
                    &pomelo_core::copper::MeshLimits::default(),
                    &cancel,
                )
                .unwrap(),
            ));
        }
        let mut frame = BoardFrame {
            curves: None,
            traces: TraceFrame {
                tracks: Arc::new(
                    PreparedTracks::build(&[], TraceLimits::default(), &cancel).unwrap(),
                ),
                bounds: Bounds {
                    min: Point::new(-6.0, -6.0),
                    max: Point::new(6.0, 6.0),
                },
                camera: Some(Camera {
                    center: Point::default(),
                    pixels_per_mm: 10.0,
                    flipped: false,
                }),
                scale_factor: 1.0,
                colors: Arc::new(BTreeMap::from([
                    (LayerId::BOND_TOP, [1.0, 1.0, 0.0, 1.0]),
                    (LayerId(1), [0.0, 0.0, 1.0, 1.0]),
                ])),
                fallback_color: [1.0; 4],
                material_override: None,
                opacity: 1.0,
                color_mode: pomelo_core::display::ColorMode::Layer,
                pass: crate::backend::OverlayPass::Base,
                filled: true,
                hover_selection: None,
                highlighted_objects: None,
                highlighted_net: None,
                highlighted_trace: None,
                highlighted_object: None,
                hovered_object: None,
                highlighted_related_objects: None,
            },
            pads: Some(Arc::new(pads)),
            drills: None,
            copper: Arc::new(
                crate::copper::PreparedCopper::build(
                    &[],
                    crate::copper::CopperLimits::default(),
                    &cancel,
                )
                .unwrap(),
            ),
            display: Arc::new(BoardDisplay::default()),
            copper_opacity: 0.25,
            layer_order: Arc::new(vec![LayerId(1), LayerId::BOND_TOP]),
            glyphs: None,
            labels: None,
            zone_outlines: None,
            drawings: None,
            texts: None,
            drill_color: [0.5; 4],
        };
        let pad_stats = Arc::new(TraceTelemetry::default());
        let custom_stats = Arc::new(CopperTelemetry::default());
        let outline_stats = Arc::new(TraceTelemetry::default());
        let mut renderer = BoardRenderer::new(
            Arc::new(TraceTelemetry::default()),
            Arc::new(CopperTelemetry::default()),
            Arc::clone(&pad_stats),
            Arc::clone(&custom_stats),
            Arc::new(TraceTelemetry::default()),
            Arc::new(TraceTelemetry::default()),
            Arc::new(TraceTelemetry::default()),
        )
        .with_custom_outline_telemetry(Arc::clone(&outline_stats));
        for _ in 0..12 {
            target.bind(&context);
            renderer.draw(&gpu, &frame).unwrap();
        }
        let before = (
            pad_stats.snapshot().uploaded_bytes,
            custom_stats.snapshot().uploaded_bytes,
            outline_stats.snapshot().uploaded_bytes,
        );
        for state in [
            "etch-only",
            "pins-only",
            "promoted-etch",
            "default-overlap",
            "hidden-die",
        ] {
            for filled in [false, true] {
                for dpi in [1.0_f32, 2.0] {
                    for flipped in [false, true] {
                        let mut display = BoardDisplay {
                            filled,
                            ..BoardDisplay::default()
                        };
                        if matches!(state, "etch-only" | "pins-only" | "hidden-die") {
                            display.hidden_layers.insert(LayerId(1));
                        }
                        if state == "hidden-die" {
                            display.hidden_layers.insert(LayerId::BOND_TOP);
                        }
                        display.layer_primitives.insert(
                            LayerId::BOND_TOP,
                            LayerPrimitives {
                                traces: state != "pins-only",
                                pads: state != "etch-only",
                                vias: true,
                            },
                        );
                        if state == "promoted-etch" {
                            display.priorities.push(LayerPriority {
                                layer: LayerId::BOND_TOP,
                                category: DisplayCategory::Trace,
                            });
                        }
                        frame.display = Arc::new(display);
                        frame.traces.scale_factor = dpi;
                        frame.traces.camera.as_mut().unwrap().flipped = flipped;
                        target.bind(&context);
                        renderer.draw(&gpu, &frame).unwrap();
                        let pixels = target.read(&context);
                        let visible = !matches!(state, "pins-only" | "hidden-die");
                        let overlap = matches!(state, "promoted-etch" | "default-overlap");
                        // Sample well inside material and just inside the outer edge. These are
                        // independent screen positions, never generated from GPU batch metadata.
                        let zoom = (10.0 * dpi) as usize;
                        let interior = rgb(&pixels, 64 + zoom, 64);
                        let edge = rgb(&pixels, 64 + 2 * zoom - 1, 64);
                        let hole = rgb(&pixels, 64, 64);
                        let expected_interior = if filled && overlap && state == "default-overlap" {
                            "blue"
                        } else if filled && visible {
                            "yellow"
                        } else {
                            "black"
                        };
                        let expected_edge = if filled && overlap && state == "default-overlap" {
                            "blue"
                        } else if visible {
                            "yellow"
                        } else {
                            "black"
                        };
                        let expected_hole = if filled && overlap { "blue" } else { "black" };
                        let matches_color = |pixel: [u8; 3], color| match color {
                            // The edge blends yellow coverage over the underlying blue pin.
                            "yellow" => pixel[0] > 100 && pixel[1] > 100 && pixel[2] < 80,
                            "blue" => pixel[0] < 5 && pixel[1] < 5 && pixel[2] > 240,
                            _ => pixel.into_iter().all(|channel| channel < 5),
                        };
                        if !matches_color(interior, expected_interior)
                            || !matches_color(edge, expected_edge)
                            || (geometry == "custom-hole" && !matches_color(hole, expected_hole))
                        {
                            failures.push(serde_json::json!({"geometry":geometry,"state":state,"filled":filled,"dpi":dpi,"flipped":flipped,
                                "interior":interior,"expected_interior":expected_interior,"edge":edge,"expected_edge":expected_edge,
                                "hole":hole,"expected_hole":expected_hole}));
                        }
                        cases.push(serde_json::json!({"geometry":geometry,"state":state,"filled":filled,"dpi":dpi,"flipped":flipped,
                            "interior":interior,"expected_interior":expected_interior,"edge":edge,"expected_edge":expected_edge,
                            "hole":hole,"expected_hole":if geometry == "custom-hole" { Some(expected_hole) } else { None }}));
                    }
                }
            }
        }
        assert_eq!(
            (
                pad_stats.snapshot().uploaded_bytes,
                custom_stats.snapshot().uploaded_bytes,
                outline_stats.snapshot().uploaded_bytes
            ),
            before
        );
        assert_eq!(pad_stats.snapshot().cache_builds, 1);
        if geometry == "custom-hole" {
            assert_eq!(custom_stats.snapshot().cache_builds, 1);
            assert_eq!(outline_stats.snapshot().cache_builds, 1);
        }
        caches.push(serde_json::json!({"geometry":geometry,"pad_cache_builds":pad_stats.snapshot().cache_builds,
            "custom_cache_builds":custom_stats.snapshot().cache_builds,"outline_cache_builds":outline_stats.snapshot().cache_builds,
            "uploaded_before":before,"uploaded_after":[pad_stats.snapshot().uploaded_bytes,custom_stats.snapshot().uploaded_bytes,outline_stats.snapshot().uploaded_bytes]}));
    }
    if let Some(directory) = std::env::var_os("POMELO_CANVAS_GPU_REPORT_DIR") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            std::path::Path::new(&directory).join("d3d11-die-pads.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"cases":cases,"failures":failures,"caches":caches}),
            )
            .unwrap(),
        )
        .unwrap();
    }
    assert!(
        failures.is_empty(),
        "{} die pad cases failed; first: {:?}",
        failures.len(),
        failures.first()
    );
}
