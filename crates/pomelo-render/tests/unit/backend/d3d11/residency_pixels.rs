//! Hardware proof of visible chunk residency with original source draw order.
use super::*;

#[test]
#[ignore = "requires hardware D3D11"]
fn hardware_visible_trace_chunks_pan_evict_and_match_eager_pixels() {
    let (device, context) = compositor_parity::device();
    let gpu = compositor_parity::gpu_context(&device, &context);
    let target = Target::new(&device);
    let cancel = CancellationToken::default();
    let count = 4096;
    let segments: Vec<_> = (0..count * 3)
        .map(|index| {
            let group = index / count;
            let x = if group == 1 { 100.0 } else { 0.0 };
            Segment {
                id: ObjectId(index as u32),
                track_id: ObjectId(group as u32 + 1),
                layer: LayerId(0),
                net: NetId(group as u32 + 1),
                a: Point::new(x - 3.0, 0.0),
                b: Point::new(x + 3.0, 0.0),
                width: 0.2,
                arc: None,
                bond_wire: None,
            }
        })
        .collect();
    let source =
        Arc::new(PreparedTracks::build(&segments, TraceLimits::default(), &cancel).unwrap());
    let mut frame = compositor_parity::fixture().traces;
    frame.tracks = source;
    frame.camera = Some(Camera {
        center: Point::default(),
        pixels_per_mm: 10.0,
        flipped: false,
    });
    frame.opacity = 0.35;
    frame.color_mode = pomelo_core::display::ColorMode::Net;
    let stats = Arc::new(TraceTelemetry::default());
    let mut cached = TraceRenderer::<PreparedTracks>::new(Arc::clone(&stats))
        .with_residency(count * size_of::<crate::tracks::TraceInstance>());
    let mut eager = TraceRenderer::<PreparedTracks>::new(Arc::new(TraceTelemetry::default()));

    for center in [0.0, 100.0, 0.0] {
        frame.camera.as_mut().unwrap().center.x = center;
        target.bind(&context);
        eager.draw(&gpu, &frame).unwrap();
        let expected = target.read(&context);
        target.bind(&context);
        cached.draw(&gpu, &frame).unwrap();
        assert_eq!(
            target.read(&context),
            expected,
            "source order/coverage changed at center={center}"
        );
        let snapshot = stats.snapshot();
        assert!(snapshot.visible_ready);
        assert_eq!(
            snapshot.uploaded_instances,
            if center == 100.0 { count } else { count * 2 } as u64
        );
        let uploaded = snapshot.uploaded_bytes;
        target.bind(&context);
        cached.draw(&gpu, &frame).unwrap();
        assert_eq!(
            stats.snapshot().uploaded_bytes,
            uploaded,
            "stable view uploaded again"
        );
        assert_eq!(target.read(&context), expected);
    }
    assert_eq!(
        stats.snapshot().uploaded_bytes,
        (count * 5 * size_of::<crate::tracks::TraceInstance>()) as u64
    );
    cached.reset();
    target.bind(&context);
    cached.draw(&gpu, &frame).unwrap();
    assert!(stats.snapshot().visible_ready);
    assert_eq!(stats.snapshot().uploaded_instances, (count * 2) as u64);
    println!(
        "VISIBLE_TRACE_RESIDENCY_HARDWARE source_instances={} first_visible={} far_visible={} return_reuploads=true stable_reuploads=false eager_pixels_equal=true reset_verified=true",
        count * 3,
        count * 2,
        count
    );
}
