//! Synchronized production geometry submission timings; excludes GPUI and dynamic labels.
use super::*;

#[test]
#[ignore = "requires explicit local input and a hardware D3D11 adapter; diagnostic timings"]
fn hardware_real_board_frame_profile() {
    use crate::{
        copper::PreparedCopper, drills::PreparedDrills, pads::PreparedPads,
        preparation_memory::PreparationMemory,
    };
    use pomelo_import::{
        BoardImporter, ImportContext, ImportOptions, TextEncoding, allegro::AllegroImporter,
    };
    let path =
        std::env::var_os("POMELO_FRAME_PROFILE_CASE").expect("explicit POMELO_FRAME_PROFILE_CASE");
    let output = std::env::var_os("POMELO_FRAME_PROFILE_OUTPUT").map(std::path::PathBuf::from);
    let cancel = CancellationToken::default();
    let board = AllegroImporter
        .import(
            std::path::Path::new(&path),
            &ImportOptions {
                text_encoding: TextEncoding::Windows1252,
                ..Default::default()
            },
            &ImportContext {
                cancellation: &cancel,
                progress: &|_| {},
            },
        )
        .unwrap();
    let scene = &board.scene;
    let mut memory = PreparationMemory::new(pomelo_core::memory::MemoryBudget::new(0));
    let mut frame = compositor_parity::fixture();
    frame.traces.tracks = Arc::new(
        memory
            .prepare_tracks(|limits| {
                PreparedTracks::build_with_outline(&scene.segments, &scene.outline, limits, &cancel)
            })
            .unwrap(),
    );
    frame.traces.bounds = scene.bounds;
    frame.traces.colors = Arc::new(BTreeMap::new());
    frame.copper = Arc::new(
        memory
            .prepare_copper(|limits| {
                PreparedCopper::build_scene(Arc::clone(scene), limits, &cancel)
            })
            .unwrap(),
    );
    let mut pads = memory
        .prepare_pads(|limits| PreparedPads::build(&scene.pins, &scene.vias, limits, &cancel))
        .unwrap();
    pads.custom_mesh = Some(Arc::new(
        memory
            .prepare_copper(|limits| {
                pads.build_custom_meshes(
                    limits,
                    &pomelo_core::copper::MeshLimits::default(),
                    &cancel,
                )
            })
            .unwrap(),
    ));
    frame.pads = Some(Arc::new(pads));
    frame.drills = Some(Arc::new(
        memory
            .prepare_pads(|limits| {
                PreparedDrills::build(&scene.pins, &scene.vias, limits, &cancel)
                    .map(|value| value.geometry)
            })
            .unwrap(),
    ));
    frame.zone_outlines = Some(Arc::new(
        memory
            .prepare_tracks(|limits| {
                PreparedTracks::build_zone_outlines(&scene.zones, limits, &cancel)
            })
            .unwrap(),
    ));
    frame.drawings = Some(Arc::new(
        memory
            .prepare_tracks(|limits| {
                PreparedTracks::build_drawings(&scene.drawings, limits, &cancel)
            })
            .unwrap(),
    ));
    frame.glyphs = None;
    frame.labels = None;
    frame.curves = None;
    frame.texts = None;
    frame.layer_order = Arc::new(scene.layers.iter().map(|layer| layer.id).collect());
    frame.display = Arc::new(pomelo_core::display::BoardDisplay::default());
    let mut camera = Camera::default();
    assert!(camera.fit(scene.bounds, 1280.0, 720.0, 20.0));
    frame.traces.camera = Some(camera);
    let (device, context) = compositor_parity::device();
    let target = Target::with_size(&device, 1280, 720);
    let bounds = ViewBounds::new(
        point(ScaledPixels(0.0), ScaledPixels(0.0)),
        size(ScaledPixels(1280.0), ScaledPixels(720.0)),
    );
    let gpu = NativeGpuContext {
        device: crate::backend::d3d11::Device::new(&device, &context, 1.0),
        viewport: [1280.0, 720.0],
        bounds,
        content_mask: ContentMask { bounds },
    };
    let mut renderer = compositor_parity::renderer();
    for _ in 0..3000 {
        target.bind(&context);
        renderer.draw(&gpu, &frame).unwrap();
        if renderer.geometry_is_uploaded(&frame) {
            break;
        }
    }
    assert!(renderer.geometry_is_uploaded(&frame));
    let _ = target.read(&context);
    for mode in [
        "fit",
        "local",
        "pan",
        "net",
        "hover_fit",
        "hover_local",
        "object_hover_fit",
        "object_hover_local",
        "mixed_hover_local",
    ] {
        let mut view = camera;
        if !matches!(mode, "fit" | "hover_fit" | "object_hover_fit") {
            view.pixels_per_mm *= 16.0;
        }
        if mode == "pan" {
            view.center.x += 100.0 / view.pixels_per_mm;
        }
        let pointer = (mode.starts_with("object_hover") || mode == "mixed_hover_local")
            .then(|| scene.vias.iter().find(|via| !via.pads.is_empty()))
            .flatten();
        if mode.ends_with("local")
            && let Some(via) = pointer
        {
            view.center = via.at;
        }
        frame.traces.camera = Some(view);
        frame.traces.highlighted_net = (mode == "net" || mode == "mixed_hover_local").then(|| {
            (
                scene.vias.first().map_or(NetId(1), |via| via.net),
                [1.0, 1.0, 1.0, 0.9],
            )
        });
        frame.traces.hover_selection = mode.starts_with("hover").then(|| {
            (
                pomelo_core::selection::SelectionTarget::Net(
                    scene.vias.first().map_or(NetId(1), |via| via.net),
                ),
                Arc::new(Default::default()),
            )
        });
        frame.traces.hovered_object = pointer.map(|via| {
            (
                pomelo_core::selection::SelectedObject::Via(via.id),
                [0.63, 1.0, 0.85, 0.9],
            )
        });
        for trial in 0..7 {
            target.bind(&context);
            let started = std::time::Instant::now();
            renderer.draw(&gpu, &frame).unwrap();
            let submit_us = started.elapsed().as_micros();
            let pixels = target.read(&context);
            let synchronized_us = started.elapsed().as_micros();
            if trial >= 2 {
                println!(
                    "FRAME_PROFILE {}",
                    serde_json::json!({"case":path,"mode":mode,"trial":trial-1,"width":1280,"height":720,"submit_us":submit_us,"synchronized_us":synchronized_us,"labels":false,"gpui":false,"pointer":frame.traces.hovered_object.map(|(object,_)|format!("{object:?}")),"camera":[view.center.x,view.center.y,view.pixels_per_mm]})
                );
            }
            if trial == 6
                && let Some(output) = &output
            {
                std::fs::create_dir_all(output).unwrap();
                std::fs::write(output.join(format!("{mode}.rgba")), &pixels).unwrap();
            }
        }
    }
}
