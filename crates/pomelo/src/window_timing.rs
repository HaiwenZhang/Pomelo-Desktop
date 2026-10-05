//! Opt-in intervals from GPUI's window profiler, independent of native draw callbacks.
use gpui::{
    App, AppContext, Subscription, Window,
    profiler::{FrameDurationSnapshot, InputLatencySnapshot},
};
use std::time::{Duration, Instant};

struct WindowActivity {
    notifications: u64,
    _subscription: Subscription,
}

/// Observe existing frames without requesting additional redraws or input.
pub fn install(window: &Window, cx: &App) {
    if pomelo_render::frame_timing::begin().is_none() {
        return;
    }
    let executor = cx.background_executor().clone();
    window.spawn(cx, async move |cx| {
        // Endpoint focus alone misses an inactive/active transition within a sample.
        // Keep the subscription alive only while this opt-in sampler runs.
        let Ok(activity) = cx.update(|window, cx| {
            cx.new(|cx| {
                let subscription = cx.observe_window_activation(
                    window,
                    |activity: &mut WindowActivity, window, _| {
                        activity.notifications = activity.notifications.wrapping_add(1);
                        pomelo_render::frame_timing::record(
                            "window_activity",
                            pomelo_render::frame_timing::begin(),
                            || serde_json::json!({
                                "active": window.is_window_active(),
                                "notification": activity.notifications,
                            }),
                        );
                    },
                );
                WindowActivity { notifications: 0, _subscription: subscription }
            })
        }) else { return; };
        let mut previous: Option<(FrameDurationSnapshot, InputLatencySnapshot, bool, u64)> = None;
        let mut last_sample = Instant::now();
        loop {
            executor.timer(Duration::from_secs(1)).await;
            let Some(started) = pomelo_render::frame_timing::begin() else {
                break;
            };
            let duration = last_sample.elapsed();
            last_sample = Instant::now();
            let sample = cx.update(|window, cx| {
                (
                    window.frame_duration_snapshot(),
                    window.input_latency_snapshot(),
                    window.is_window_active(),
                    activity.read(cx).notifications,
                )
            });
            let Ok((frame, input, active, notifications)) = sample else { break; };
            if let Some((old_frame, old_input, old_active, old_notifications)) = previous.take() {
                let mut delta_frame = frame.clone();
                let mut delta_input = input.clone();
                let valid = delta_frame.draw_duration_histogram.subtract(&old_frame.draw_duration_histogram).is_ok()
                    && delta_frame.dirty_to_present_histogram.subtract(&old_frame.dirty_to_present_histogram).is_ok()
                    && delta_frame.present_interval_histogram.subtract(&old_frame.present_interval_histogram).is_ok()
                    && delta_input.latency_histogram.subtract(&old_input.latency_histogram).is_ok()
                    && delta_input.events_per_frame_histogram.subtract(&old_input.events_per_frame_histogram).is_ok();
                if valid {
                    macro_rules! summary {
                        ($hist:expr, $scale:expr) => {{
                            let hist = &$hist;
                            serde_json::json!({
                                "count":hist.len(),
                                "p50":hist.value_at_quantile(0.5) as f64 / $scale,
                                "p95":hist.value_at_quantile(0.95) as f64 / $scale,
                                "max":hist.max() as f64 / $scale,
                            })
                        }};
                    }
                    pomelo_render::frame_timing::record("window_interval", Some(started), || serde_json::json!({
                        "interval_ms":duration.as_secs_f64() * 1000.0,
                        "active_start":old_active,
                        "active_end":active,
                        "activation_notifications":notifications.wrapping_sub(old_notifications),
                        "active_entire_interval":old_active && active && notifications == old_notifications,
                        "draw_ms":summary!(delta_frame.draw_duration_histogram, 1_000_000.0),
                        "dirty_to_present_ms":summary!(delta_frame.dirty_to_present_histogram, 1_000_000.0),
                        "present_interval_ms":summary!(delta_frame.present_interval_histogram, 1_000_000.0),
                        "input_to_present_ms":summary!(delta_input.latency_histogram, 1_000_000.0),
                        "input_events_per_frame":summary!(delta_input.events_per_frame_histogram, 1.0),
                        "mid_draw_input_dropped":input.mid_draw_events_dropped.saturating_sub(old_input.mid_draw_events_dropped),
                    }));
                }
            }
            previous = Some((frame, input, active, notifications));
        }
    }).detach();
}
