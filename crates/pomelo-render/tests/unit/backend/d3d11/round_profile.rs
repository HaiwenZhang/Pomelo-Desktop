//! Opt-in synthetic dense-instance measurement; no external board inputs.
use super::*;
use crate::{
    backend::{solid_pipeline::SolidPipeline, solid_renderer::SolidFrame},
    scene::board3d::{PreparedBoard3d, PreviewStackup},
};
use pomelo_core::{
    geometry3d::solid::SolidLimits,
    interaction::camera3d::Camera3d,
    model::{BoardScene, CustomPadGeometry, DrillShape, Layer, LayerFunction, Pad, PadKind, Pin},
};
use std::time::{Duration, Instant};
fn query(device: &ID3D11Device, kind: D3D11_QUERY) -> ID3D11Query {
    let mut result = None;
    // SAFETY: valid descriptor and test-owned output slot.
    unsafe {
        device
            .CreateQuery(
                &D3D11_QUERY_DESC {
                    Query: kind,
                    MiscFlags: 0,
                },
                Some(&mut result),
            )
            .unwrap();
    }
    result.unwrap()
}
fn fetch<T: Default>(context: &ID3D11DeviceContext, query: &ID3D11Query) -> T {
    let mut value = T::default();
    let start = Instant::now();
    loop {
        // SAFETY: T matches the query's native result layout and the context owns the query.
        let status = unsafe {
            (context.vtable().GetData)(
                context.as_raw(),
                query.as_raw(),
                (&mut value as *mut T).cast(),
                size_of::<T>() as u32,
                0,
            )
        };
        status.ok().unwrap();
        if status.0 == 0 {
            return value;
        }
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::yield_now();
    }
}
#[test]
#[ignore = "requires hardware D3D11; synthetic round-instance GPU timing"]
fn dense_round_instances_gpu_profile() {
    let cancel = CancellationToken::default();
    let (device, context) = compositor_parity::device();
    let target = Target::with_size(&device, 1920, 1080);
    let bounds = ViewBounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(1920.), ScaledPixels(1080.)),
    );
    let gpu = NativeGpuContext {
        device: crate::backend::d3d11::Device::new(&device, &context, 1.),
        viewport: [1920., 1080.],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let clock = query(&device, D3D11_QUERY_TIMESTAMP_DISJOINT);
    let begin = query(&device, D3D11_QUERY_TIMESTAMP);
    let end = query(&device, D3D11_QUERY_TIMESTAMP);
    for fine in [false, true, true, false] {
        let mut pads = (0..3)
            .map(|i| Pad::circle(LayerId(i), 0.25))
            .collect::<Vec<_>>();
        if !fine {
            let ring = (0..12)
                .map(|i| {
                    let a = i as f64 * std::f64::consts::TAU / 12.;
                    Point::new(a.cos() * 0.125, a.sin() * 0.125)
                })
                .collect();
            let profile = Arc::new(CustomPadGeometry {
                contours: vec![ring],
                paths: vec![],
            });
            for pad in &mut pads {
                pad.kind = PadKind::CUSTOM;
                pad.custom = Some(profile.clone());
            }
        }
        let board = BoardScene {
            layers: (0..3)
                .map(|i| Layer {
                    id: LayerId(i),
                    name: format!("L{i}"),
                    function: LayerFunction::Conductor,
                    color: "#ffffff".into(),
                    source_flags: None,
                })
                .collect(),
            special_layers: vec![],
            nets: Default::default(),
            segments: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            diagnostics: vec![],
            pins: (0..131_072)
                .map(|i| Pin {
                    id: ObjectId(i + 1),
                    owner_id: ObjectId(0),
                    net: NetId(1),
                    name: String::new(),
                    reference: String::new(),
                    at: Point::new((i % 512) as f64 * 0.5, (i / 512) as f64 * 0.5),
                    angle: 0.,
                    mirrored: false,
                    drill: 0.,
                    drill_shape: DrillShape {
                        width: 0.,
                        height: 0.,
                        rectangular: false,
                        plated: false,
                        custom: None,
                    },
                    pads: pads.clone(),
                    stackup_region: None,
                    die: None,
                })
                .collect(),
            bounds: Bounds {
                min: Point::new(-1., -1.),
                max: Point::new(257., 129.),
            },
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
        assert_eq!(scene.warning_count, 0);
        let mut camera = Camera3d::default();
        assert!(camera.fit(scene.bounds, [1920., 1080.], 8.));
        let frame = SolidFrame {
            materials: Arc::new(
                std::iter::once([0.; 4])
                    .chain(scene.sources.iter().map(|_| [0.8, 0.4, 0.1, 1.]))
                    .collect(),
            ),
            scene: scene.clone(),
            camera,
            z_scale: 1.,
            dielectric_only: false,
        };
        let mut pipeline = SolidPipeline::new(&gpu.device, &scene).unwrap();
        for _ in 0..200 {
            target.bind(&context);
            pipeline.draw(&gpu, &frame).unwrap();
            // SAFETY: timestamp query synchronizes this test-owned command stream.
            unsafe {
                context.End(&end);
                context.Flush();
            }
            let _: u64 = fetch(&context, &end);
            if pipeline.pending_pages == 0 {
                break;
            }
        }
        assert_eq!(pipeline.pending_pages, 0);
        for _ in 0..16 {
            target.bind(&context);
            pipeline.draw(&gpu, &frame).unwrap();
        }
        // SAFETY: drain the warmup before starting timestamp samples.
        unsafe {
            context.End(&end);
            context.Flush();
        }
        let _: u64 = fetch(&context, &end);
        let mut samples = Vec::new();
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) {
            target.bind(&context);
            // SAFETY: all queries belong to this device and are read before reuse.
            unsafe {
                context.Begin(&clock);
                context.End(&begin);
            }
            pipeline.draw(&gpu, &frame).unwrap();
            unsafe {
                context.End(&end);
                context.End(&clock);
                context.Flush();
            }
            let frequency: D3D11_QUERY_DATA_TIMESTAMP_DISJOINT = fetch(&context, &clock);
            let a: u64 = fetch(&context, &begin);
            let b: u64 = fetch(&context, &end);
            if !frequency.Disjoint.as_bool() {
                samples.push((b - a) as f64 * 1000. / frequency.Frequency as f64);
            }
        }
        samples.sort_by(f64::total_cmp);
        assert!(!samples.is_empty());
        println!(
            "ROUND_SYNTHETIC fine={fine} sources={} instances={} prototype_vertices={} upload_bytes={} gpu_p50_ms={:.4} gpu_p95_ms={:.4}",
            scene.sources.len(),
            scene.instance_count(),
            scene.vertex_count(),
            scene.upload_bytes(),
            samples[samples.len() / 2],
            samples[((samples.len() - 1) as f64 * 0.95).ceil() as usize]
        );
    }
}
