//! Camera motion: navigation frames may resample the previous base image.
use super::*;
use pomelo_core::interaction::Camera;
use std::time::{Duration, Instant};

/// Matches the label layout's navigation idle interval.
const SETTLE: Duration = Duration::from_millis(120);

#[derive(Default)]
pub(super) struct CameraMotion {
    last: Option<Camera>,
    changed_at: Option<Instant>,
    settle: Option<Task<()>>,
}

fn same_view(a: Camera, b: Camera) -> bool {
    a.center == b.center && a.pixels_per_mm == b.pixels_per_mm && a.flipped == b.flipped
}

impl BoardViewport {
    /// Record this frame's camera. True while it changed within `SETTLE`; each
    /// change schedules one repaint after the camera rests, so a resampled
    /// preview never remains on screen.
    pub(super) fn update_camera_motion(&mut self, camera: Camera, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let motion = &mut self.camera_motion;
        if motion.last.is_some_and(|last| !same_view(last, camera)) {
            motion.changed_at = Some(now);
            let rest = cx.background_executor().timer(SETTLE);
            motion.settle = Some(cx.spawn(async move |this, cx| {
                rest.await;
                let _ = this.update(cx, |_, cx| cx.notify());
            }));
        }
        motion.last = Some(camera);
        motion
            .changed_at
            .is_some_and(|changed| now.duration_since(changed) < SETTLE)
    }
}

#[cfg(test)]
mod tests {
    use super::same_view;
    use pomelo_core::interaction::Camera;

    #[test]
    fn camera_views_compare_center_scale_and_flip() {
        let camera = Camera::default();
        assert!(same_view(camera, camera));
        assert!(!same_view(
            camera,
            Camera {
                flipped: !camera.flipped,
                ..camera
            }
        ));
        assert!(!same_view(
            camera,
            Camera {
                pixels_per_mm: camera.pixels_per_mm * 2.0,
                ..camera
            }
        ));
    }
}
