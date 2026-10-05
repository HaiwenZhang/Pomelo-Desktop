use super::*;
use gpui::{Bounds as GpuBounds, ContentMask, GpuPaintTarget, ScaledPixels, point, size};
use gpui_wgpu::wgpu;
use pomelo_core::{
    copper::{CopperMesh, MeshLimits},
    model::{Bounds, LayerId, NetId, ObjectId, Point, Zone, ZoneKind},
    task::CancellationToken,
};

fn prepared() -> Arc<PreparedCopper> {
    let square = |x: f64, y: f64, side: f64| {
        vec![
            Point::new(x, y),
            Point::new(x + side, y),
            Point::new(x + side, y + side),
            Point::new(x, y + side),
        ]
    };
    let zones: Vec<_> = [2, 0, 1]
        .into_iter()
        .enumerate()
        .map(|(index, layer)| {
            let x = index as f64 * 12.0;
            Zone {
                id: ObjectId(index as u32 + 1),
                layer: LayerId(layer),
                net: NetId(1),
                kind: ZoneKind::Static,
                paths: vec![],
                mesh: CopperMesh::build(
                    &[square(x, 0.0, 10.0), square(x + 3.0, 3.0, 4.0)],
                    &[],
                    &MeshLimits::default(),
                    &CancellationToken::default(),
                )
                .unwrap(),
            }
        })
        .collect();
    Arc::new(
        PreparedCopper::build(
            &zones,
            crate::copper::CopperLimits::default(),
            &CancellationToken::default(),
        )
        .unwrap(),
    )
}

#[test]
fn copper_pages_follow_source_order_and_respect_both_limits() {
    let source = prepared();
    assert_eq!(source.batches[0].object, ObjectId(2));
    for (vertices, indices) in [(8, usize::MAX / 4), (usize::MAX / 16, 12)] {
        let (pages, mapping) = page_ranges(&source, vertices, indices).unwrap();
        assert_eq!(pages.len(), 3);
        assert_eq!(mapping, [1, 2, 0]);
        for (index, page) in pages.iter().enumerate() {
            assert_eq!(page.vertices, index * 8..(index + 1) * 8);
            assert_eq!(page.indices, index * 12..(index + 1) * 12);
            let indices = source.index_block(page.indices.clone()).unwrap();
            assert!(indices.iter().all(|&value| {
                value
                    .checked_sub(page.vertices.start as u32)
                    .is_some_and(|local| (local as usize) < page.vertices.len())
            }));
        }
    }
}

#[test]
fn copper_pages_reject_a_batch_larger_than_the_device_limit() {
    let error = page_ranges(&prepared(), 7, 12).unwrap_err();
    assert!(
        error.to_string().contains("GPU_COPPER_BATCH_SIZE"),
        "{error}"
    );
}

#[test]
fn copper_pages_accept_empty_geometry_without_allocating_buffers() {
    let source = PreparedCopper::build(
        &[],
        crate::copper::CopperLimits::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let (pages, mapping) = page_ranges(&source, 8, 12).unwrap();
    assert!(pages.is_empty() && mapping.is_empty());
}

enum BufferSizing {
    DeviceLimits,
    SmallPages,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DrawScope {
    All,
    OneBatchPerPage,
}

fn paint_context<'a>(
    raw: &'a wgpu::Device,
    encoder: &'a mut wgpu::CommandEncoder,
    view: &'a wgpu::TextureView,
    bounds: GpuBounds<ScaledPixels>,
) -> gpui_wgpu::WgpuPaintContext<'a> {
    gpui_wgpu::WgpuPaintContext {
        target: GpuPaintTarget {
            size: [64, 64],
            bounds,
            clip: bounds,
            scale_factor: 1.0,
            sample_count: 1,
            opacity: 1.0,
        },
        device: raw,
        encoder,
        color_target: view,
        color_format: wgpu::TextureFormat::Rgba8Unorm,
    }
}

fn render(
    source: Arc<PreparedCopper>,
    bounds: Bounds,
    sizing: BufferSizing,
    scope: DrawScope,
) -> Vec<u8> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        futures::executor::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (raw, queue) = futures::executor::block_on(
        adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: wgpu::Limits::downlevel_defaults()
                .using_resolution(adapter.limits())
                .using_alignment(adapter.limits()),
            ..Default::default()
        }),
    )
    .unwrap();
    let extent = wgpu::Extent3d {
        width: 64,
        height: 64,
        depth_or_array_layers: 1,
    };
    let texture = raw.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let gpu_bounds = GpuBounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(64.0), ScaledPixels(64.0)),
    );
    let mut encoder = raw.create_command_encoder(&Default::default());
    let mut paint = paint_context(&raw, &mut encoder, &view, gpu_bounds);
    let device = Device::new(&mut paint);
    let (mut uploaded, upload_budget) = match sizing {
        BufferSizing::SmallPages => (
            UploadedCopper::new_with_limits(Arc::clone(&source), &device, 8 * 16, 12 * 4).unwrap(),
            36,
        ),
        BufferSizing::DeviceLimits => (
            UploadedCopper::new(Arc::clone(&source), &device).unwrap(),
            UPLOAD_BYTES_PER_FRAME,
        ),
    };
    let mut pipeline = Pipeline::new(&device).unwrap();
    drop(device);
    queue.submit([encoder.finish()]);
    let mut iterations = 0;
    while uploaded.uploaded_bytes() < source.upload_bytes() {
        let mut encoder = raw.create_command_encoder(&Default::default());
        let mut paint = paint_context(&raw, &mut encoder, &view, gpu_bounds);
        let context = NativeGpuContext {
            device: Device::new(&mut paint),
            viewport: [64.0; 2],
            bounds: gpu_bounds,
            content_mask: ContentMask { bounds: gpu_bounds },
        };
        let before = uploaded.uploaded_bytes();
        // Small synthetic budgets cross vertex/index and page boundaries.
        let mut budget = upload_budget;
        uploaded.upload_with_budget(&context, &mut budget).unwrap();
        assert!(uploaded.uploaded_bytes() > before);
        assert!(uploaded.uploaded_bytes() - before <= upload_budget);
        drop(context);
        queue.submit([encoder.finish()]);
        raw.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(30)),
        })
        .unwrap();
        iterations += 1;
    }
    eprintln!(
        "copper pages={} upload_bytes={} frames={iterations}",
        uploaded.pages.len(),
        uploaded.uploaded_bytes()
    );
    if scope == DrawScope::OneBatchPerPage {
        assert!(uploaded.pages.len() > 1);
    }
    let mut encoder = raw.create_command_encoder(&Default::default());
    {
        let attachments = [Some(wgpu::RenderPassColorAttachment {
            view: &view,
            resolve_target: None,
            depth_slice: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })];
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &attachments,
            ..Default::default()
        });
    }
    let mut paint = paint_context(&raw, &mut encoder, &view, gpu_bounds);
    let context = NativeGpuContext {
        device: Device::new(&mut paint),
        viewport: [64.0; 2],
        bounds: gpu_bounds,
        content_mask: ContentMask { bounds: gpu_bounds },
    };
    let tracks = crate::tracks::PreparedTracks::build(
        &[],
        Default::default(),
        &CancellationToken::default(),
    )
    .unwrap();
    let frame = TraceFrame {
        tracks: Arc::new(tracks),
        bounds,
        camera: None,
        scale_factor: 1.0,
        pass: Default::default(),
        filled: true,
        hover_selection: None,
        color_mode: pomelo_core::display::ColorMode::Layer,
        colors: Arc::new(BTreeMap::new()),
        fallback_color: [1.0, 0.0, 0.0, 1.0],
        material_override: None,
        opacity: 1.0,
        highlighted_objects: None,
        highlighted_net: None,
        highlighted_trace: None,
        highlighted_object: None,
        hovered_object: None,
        highlighted_related_objects: None,
    };
    // The real board exercises all uploads and one small batch from each page.
    let selected: BTreeSet<_> = (0..uploaded.pages.len())
        .filter_map(|page| {
            source
                .batches
                .iter()
                .enumerate()
                .filter(|(index, batch)| {
                    uploaded.batch_pages[*index] == page && !batch.outer_indices().is_empty()
                })
                .min_by_key(|(_, batch)| batch.vertex_count)
                .map(|(_, batch)| batch.object)
        })
        .collect();
    let visible = |batch: &crate::copper::CopperBatch| {
        scope == DrawScope::All || selected.contains(&batch.object)
    };
    let (_, zones) = pipeline
        .draw(
            &context,
            &frame,
            &uploaded,
            CopperDrawOptions {
                opacity: 1.0,
                layer: None,
                visible: Some(&visible),
                annotations: None,
                annotation_owners: None,
                overrides: None,
                static_shapes_fill_solid: true,
            },
        )
        .unwrap();
    assert_eq!(
        zones as usize,
        if scope == DrawScope::OneBatchPerPage {
            selected.len()
        } else {
            source.batches.len()
        }
    );
    drop(context);
    let output = raw.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 64 * 64 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &output,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(64),
            },
        },
        extent,
    );
    queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    output
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    raw.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(30)),
    })
    .unwrap();
    receive.recv().unwrap().unwrap();
    output.slice(..).get_mapped_range().to_vec()
}

#[test]
#[ignore = "requires a GPU adapter; copper page pixel readback"]
fn copper_pages_preserve_pixels_and_holes_across_upload_boundaries() {
    let source = prepared();
    let bounds = Bounds {
        min: Point::new(0.0, 0.0),
        max: Point::new(34.0, 10.0),
    };
    let single = render(
        Arc::clone(&source),
        bounds,
        BufferSizing::DeviceLimits,
        DrawScope::All,
    );
    let paged = render(source, bounds, BufferSizing::SmallPages, DrawScope::All);
    assert_eq!(single, paged);
    assert!(paged.as_chunks::<4>().0.contains(&[255, 0, 0, 255]));
    // Hole centers and the spaces between zones remain transparent.
    for x in [12, 32, 52, 22, 42] {
        assert_eq!(&paged[(32 * 64 + x) * 4..][..4], &[0, 0, 0, 0]);
    }
}

#[test]
#[ignore = "requires POMELO_TEST_BOARD and a GPU adapter; real board copper upload"]
fn copper_pages_upload_large_source_backed_board() {
    use pomelo_import::{BoardImporter, ImportContext, ImportOptions, allegro::AllegroImporter};
    let path = std::env::var_os("POMELO_TEST_BOARD").expect("set POMELO_TEST_BOARD");
    let cancel = CancellationToken::default();
    let imported = AllegroImporter
        .import(
            std::path::Path::new(&path),
            &ImportOptions::default(),
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let bounds = imported.scene.bounds;
    let source = PreparedCopper::build_scene(
        imported.scene,
        crate::copper::CopperLimits::default(),
        &cancel,
    )
    .unwrap();
    render(
        Arc::new(source),
        bounds,
        BufferSizing::DeviceLimits,
        DrawScope::OneBatchPerPage,
    );
}
