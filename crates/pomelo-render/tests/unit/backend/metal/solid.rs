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

#[test]
fn nil_metal_allocations_are_errors_before_constructing_owned_resources() {
    assert!(
        unsafe { super::super::owned_gpu_resource::<metal::Buffer>(std::ptr::null_mut()) }.is_err()
    );
    assert!(
        unsafe { super::super::owned_gpu_resource::<metal::Texture>(std::ptr::null_mut()) }
            .is_err()
    );
}

#[test]
#[ignore = "requires a Metal adapter and Xcode tools; real depth/OIT pixel readback"]
fn solid_depth_holes_and_order_independent_transparency_pixels() {
    let dir = std::env::temp_dir().join(format!(
        "pomelo-solid-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let source = dir.join("solid.metal");
    let air = dir.join("solid.air");
    let precompiled = std::env::var_os("POMELO_METAL_LIBRARY").map(std::path::PathBuf::from);
    let library_file = precompiled
        .clone()
        .unwrap_or_else(|| dir.join("solid.metallib"));
    std::fs::write(&source, metal_source().unwrap()).unwrap();
    if let Some(path) = &precompiled {
        assert_eq!(
            std::fs::read_to_string(path.with_extension("metal")).unwrap(),
            metal_source().unwrap(),
            "precompiled test shader is stale"
        );
    }
    for args in [
        vec![
            "-sdk".into(),
            "macosx".into(),
            "metal".into(),
            "-std=macos-metal2.4".into(),
            "-c".into(),
            source.as_os_str().to_owned(),
            "-o".into(),
            air.as_os_str().to_owned(),
        ],
        vec![
            "-sdk".into(),
            "macosx".into(),
            "metallib".into(),
            air.as_os_str().to_owned(),
            "-o".into(),
            library_file.as_os_str().to_owned(),
        ],
    ] {
        if precompiled.is_some() {
            break;
        }
        let output = if let Some(toolchain) = std::env::var_os("POMELO_METAL_TOOLCHAIN") {
            std::process::Command::new(
                std::path::PathBuf::from(toolchain)
                    .join("usr/bin")
                    .join(&args[2]),
            )
            .args(&args[3..])
            .output()
            .unwrap()
        } else {
            std::process::Command::new("xcrun")
                .args(args)
                .output()
                .unwrap()
        };
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let raw = metal::Device::system_default().expect("Metal device");
    let queue = raw.new_command_queue();
    let library = raw.new_library_with_file(&library_file).unwrap();
    let archive = load_archive(&raw);
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
    let mut board = BoardScene {
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
    board.segments.push(pomelo_core::model::Segment {
        id: ObjectId(100),
        track_id: ObjectId(100),
        layer: LayerId(0),
        net: NetId(1),
        a: Point::new(-0.6, 0.0),
        b: Point::new(1.0, 0.0),
        width: 0.2,
        arc: None,
        bond_wire: None,
    });
    board.segments.push(pomelo_core::model::Segment {
        id: ObjectId(101),
        track_id: ObjectId(101),
        layer: LayerId(0),
        net: NetId(1),
        a: Point::new(-0.8, 0.5),
        b: Point::new(-0.2, 0.5),
        width: 0.2,
        arc: None,
        bond_wire: None,
    });
    let trace_scene = Arc::new(
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
    let texture = TextureDescriptor::new();
    texture.set_width(128);
    texture.set_height(128);
    texture.set_pixel_format(MTLPixelFormat::RGBA8Unorm);
    texture.set_storage_mode(MTLStorageMode::Shared);
    texture.set_usage(MTLTextureUsage::RenderTarget);
    let target = raw.new_texture(&texture);
    let bounds = gpui::Bounds::new(
        gpui::point(gpui::ScaledPixels(0.0), gpui::ScaledPixels(0.0)),
        gpui::size(gpui::ScaledPixels(128.0), gpui::ScaledPixels(128.0)),
    );
    let mut pipeline = None;
    let mut images = Vec::new();
    let transparent_materials =
        Arc::new(vec![[0.0; 4], [1.0, 0.0, 0.0, 0.3], [0.0, 0.0, 1.0, 0.3]]);
    for pass in 0..7 {
        if pass == 3 {
            pipeline = None;
        }
        let current = if pass >= 3 { &trace_scene } else { &scene };
        let command = queue.new_command_buffer();
        let clear = RenderPassDescriptor::new();
        let c = clear.color_attachments().object_at(0).unwrap();
        c.set_texture(Some(&target));
        c.set_load_action(MTLLoadAction::Clear);
        c.set_store_action(MTLStoreAction::Store);
        c.set_clear_color(MTLClearColor::new(0.0, 1.0, 0.0, 1.0));
        command.new_render_command_encoder(clear).end_encoding();
        let paint = gpui::MetalPaintContext {
            device: &raw,
            command_buffer: command,
            color_target: &target,
            color_format: MTLPixelFormat::RGBA8Unorm,
            target: gpui::GpuPaintTarget {
                size: [128; 2],
                bounds,
                clip: bounds,
                scale_factor: 1.0,
                sample_count: 1,
                opacity: 1.0,
            },
        };
        let device = Device::new(&paint);
        if pipeline.is_none() {
            pipeline = Some(
                SolidPipeline::with_library(&device, current, &library, archive.as_deref())
                    .unwrap(),
            );
        }
        let p = pipeline.as_mut().unwrap();
        if pass == 6 {
            p.pending_pages = 10;
            p.uploaded_this_frame = 1024;
        }
        if pass == 4 {
            let page = trace_scene
                .pages
                .iter()
                .position(|p| p.sources.contains(&2))
                .unwrap();
            let mesh = trace_scene.pages[page].mesh;
            let shared = trace_scene
                .pages
                .iter()
                .enumerate()
                .find(|(i, p)| *i != page && p.mesh == mesh)
                .unwrap()
                .0;
            assert!(p.instances[shared].is_some());
            let before = p.resident_bytes;
            let bytes = trace_scene.pages[page].instances.len() * size_of::<Instance3d>();
            p.visible_mask[page] = false;
            p.evict_to(before - bytes);
            assert!(p.instances[page].is_none() && p.meshes[mesh].is_some());
            assert_eq!(p.resident_bytes, before - bytes);
        }
        if pass == 5 {
            p.visible_mask.fill(false);
            p.evict_to(0);
            assert_eq!(p.resident_bytes, 0);
            assert!(
                p.instances.iter().all(Option::is_none) && p.meshes.iter().all(Option::is_none)
            );
        }
        let materials = if pass >= 3 {
            Arc::new(vec![
                [0.0; 4],
                [0.0, 1.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
                [0.0, 0.0, 1.0, 1.0],
            ])
        } else if pass == 0 {
            Arc::new(vec![[0.0; 4], [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]])
        } else {
            transparent_materials.clone()
        };
        if pass == 2 {
            p.transparent_batches.reverse();
        }
        let draws = p
            .draw(
                &NativeGpuContext {
                    device,
                    viewport: [128.0; 2],
                    bounds,
                    content_mask: gpui::ContentMask {
                        bounds: if pass == 6 {
                            gpui::Bounds::default()
                        } else {
                            bounds
                        },
                    },
                },
                &SolidFrame {
                    scene: current.clone(),
                    camera,
                    materials,
                    z_scale: 1.0,
                    dielectric_only: false,
                },
            )
            .unwrap();
        if pass == 6 {
            assert_eq!(draws, 0);
            assert_eq!(p.pending_pages, 0);
            assert_eq!(p.uploaded_this_frame, 0);
        }
        if pass == 5 {
            let before = p.resident_bytes;
            p.evict_to(0);
            assert_eq!(p.resident_bytes, before, "visible pages must remain pinned");
        }
        command.commit();
        command.wait_until_completed();
        assert_eq!(command.status(), MTLCommandBufferStatus::Completed);
        let mut pixels = vec![0u8; 128 * 128 * 4];
        target.get_bytes(
            pixels.as_mut_ptr().cast(),
            128 * 4,
            MTLRegion::new_2d(0, 0, 128, 128),
            0,
        );
        images.push(pixels);
    }
    let sample = |image: &Vec<u8>, x: usize, y: usize| image[(y * 128 + x) * 4..][..4].to_vec();
    let outer = sample(&images[0], 90, 64);
    let hole = sample(&images[0], 64, 64);
    assert!(outer[0] > 180 && outer[2] < 10, "top copper: {outer:?}");
    assert!(
        hole[2] > 180 && hole[0] < 10,
        "hole must expose lower copper: {hole:?}"
    );
    assert!(
        images[1]
            .iter()
            .zip(&images[2])
            .all(|(a, b)| a.abs_diff(*b) <= 1),
        "transparent order changed pixels"
    );
    let trace = sample(&images[3], 70, 64);
    assert!(
        trace[1] > 180 && trace[0] < 10 && trace[2] < 10,
        "stretched capsule end must render in the hole: {trace:?}"
    );
    assert_eq!(
        images[3], images[4],
        "partial shared-page eviction changed pixels"
    );
    assert_eq!(images[3], images[5], "full cache recovery changed pixels");
    let shared = sample(&images[5], 59, 58);
    assert!(
        shared[1] > 180 && shared[0] < 10 && shared[2] < 10,
        "shared capsule was lost during eviction: {shared:?}"
    );
    if let Some(output) = std::env::var_os("POMELO_SOLID_PIXEL_OUTPUT") {
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        for (name, pixels) in ["opaque.png", "transparent.png"].into_iter().zip(&images) {
            image::save_buffer(output.join(name), pixels, 128, 128, image::ColorType::Rgba8)
                .unwrap();
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

fn load_archive(raw: &DeviceRef) -> Option<BinaryArchive> {
    std::env::var_os("POMELO_METAL_ARCHIVE").map(|path| {
        let desc = BinaryArchiveDescriptor::new();
        // This factory returns an autoreleased NSURL; the descriptor copies it.
        let url = std::mem::ManuallyDrop::new(URL::new_with_string(&format!(
            "file://{}",
            std::path::PathBuf::from(path).display()
        )));
        desc.set_url(&url);
        raw.new_binary_archive_with_descriptor(&desc).unwrap()
    })
}
