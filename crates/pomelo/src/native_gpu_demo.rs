//! Windows-only shader probe using GPUI's own D3D11 context and presentation.

use gpui_kit::base::Selectable;
use gpui_kit::component::{ActiveTheme, Theme, ThemeMode, WindowExt, button::Button};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use pomelo_core::i18n::{Message, MessageKey as Key, text};

use crate::i18n;
use pomelo_render::backend::d3d11::{PcbProbeRenderer, ProbeScene, ProbeTelemetry};
use std::sync::Arc;

actions!(pomelo_native_gpu, [RotateColors, ToggleClip, ShowOverlay]);

pub struct NativeGpuDemo {
    focus: FocusHandle,
    color_rotation: u32,
    clipped: bool,
    dark: bool,
    renderer: Result<NativeGpuHandle, String>,
    telemetry: Arc<ProbeTelemetry>,
    triangle: bool,
}

impl NativeGpuDemo {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let telemetry = Arc::new(ProbeTelemetry::default());
        let renderer = window
            .register_gpu_renderer(PcbProbeRenderer::new(telemetry.clone()))
            .map_err(|error| format!("{error:#}"));
        Self {
            renderer,
            telemetry,
            triangle: std::env::args().any(|arg| arg == "--triangle"),
            focus: cx.focus_handle(),
            color_rotation: 0,
            clipped: false,
            dark: true,
        }
    }

    fn rotate_colors(&mut self, cx: &mut Context<Self>) {
        self.color_rotation = (self.color_rotation + 1) % 3;
        cx.notify();
    }

    fn toggle_clip(&mut self, cx: &mut Context<Self>) {
        self.clipped = !self.clipped;
        cx.notify();
    }

    fn show_overlay(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.open_dialog(cx, |dialog, window, cx| {
            let locale = i18n::current(cx);
            dialog
                // Put the probe dialog over colored geometry, not only the toolbar.
                .margin_top(window.viewport_size().height * 0.4)
                .title(text(locale, Key::NativeGpuOverlay))
                .child(text(locale, Key::NativeGpuOverlayBody))
        });
    }
}

impl Render for NativeGpuDemo {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let locale = i18n::current(cx);
        let specs = window.gpu_specs();
        let scene = Arc::new(if self.triangle {
            ProbeScene::triangle(self.color_rotation)
        } else {
            ProbeScene::pcb(self.color_rotation)
        });
        let renderer = self.renderer.as_ref().ok().cloned();
        let failure = self.renderer.as_ref().err().cloned().or_else(|| {
            renderer
                .as_ref()
                .and_then(|handle| handle.status().last_error)
        });
        if let (Some(path), Some(handle)) = (std::env::var_os("POMELO_NATIVE_GPU_TRACE"), &renderer)
        {
            let status = handle.status();
            let report = serde_json::json!({
                "backend": "GPUI_D3D11",
                "shader_owner": "pomelo-render",
                "scene": if self.triangle { "triangle" } else { "trace_disc_square_zone_with_hole" },
                "stage": if status.presented > 0 && status.last_error.is_none() { "present_succeeded" } else { "pending_or_failed" },
                "adapter": specs.as_ref().map(|specs| &specs.device_name),
                "software_emulated": specs.as_ref().map(|specs| specs.is_software_emulated),
                "submitted": status.submitted,
                "presented": status.presented,
                "last_error": status.last_error,
                "statistics": self.telemetry.snapshot(),
                "cpu_pixel_readback": false,
                "clipped": self.clipped,
                "palette": self.color_rotation,
            });
            if let Err(error) = std::fs::write(path, report.to_string()) {
                eprintln!("{}: {error}", text(locale, Key::GpuFailed));
            }
        }
        let clipped = self.clipped;
        // Require a reported hardware adapter; do not pass this probe with WARP.
        let hardware = specs
            .as_ref()
            .is_some_and(|specs| !specs.is_software_emulated);
        let viewport = canvas(
            |bounds, _, _| bounds,
            move |bounds, _, window, _| {
                if let Some(renderer) = renderer.as_ref().filter(|_| hardware) {
                    let mut draw_bounds = bounds;
                    if clipped {
                        // Deliberately extend outside the canvas to exercise GPUI's mask.
                        draw_bounds.origin.x -= bounds.size.width * 0.2;
                    }
                    if let Err(error) = window.paint_gpu(draw_bounds, renderer, scene) {
                        eprintln!("{}: {error}", text(locale, Key::GpuFailed));
                    }
                }
            },
        )
        .size_full();

        div()
            .size_full()
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .bg(theme.background)
            .text_color(theme.foreground)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &RotateColors, _, cx| this.rotate_colors(cx)))
            .on_action(cx.listener(|this, _: &ToggleClip, _, cx| this.toggle_clip(cx)))
            .on_action(
                cx.listener(|this, _: &ShowOverlay, window, cx| this.show_overlay(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ToggleTheme, window, cx| {
                    this.dark = !this.dark;
                    Theme::change(
                        if this.dark {
                            ThemeMode::Dark
                        } else {
                            ThemeMode::Light
                        },
                        Some(window),
                        cx,
                    );
                    cx.notify();
                }),
            )
            .child(div().text_lg().child(text(locale, Key::NativeGpuTitle)))
            .child(div().text_sm().child(text(locale, Key::NativeGpuLegend)))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        Button::new("native-colors")
                            .outline()
                            .label(text(locale, Key::NativeGpuColors))
                            .on_click(cx.listener(|this, _, _, cx| this.rotate_colors(cx))),
                    )
                    .child(
                        Button::new("native-clip")
                            .outline()
                            .selected(self.clipped)
                            .label(text(locale, Key::NativeGpuClip))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_clip(cx))),
                    )
                    .child(
                        Button::new("native-overlay")
                            .outline()
                            .label(text(locale, Key::NativeGpuOverlay))
                            .on_click(
                                cx.listener(|this, _, window, cx| this.show_overlay(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    .child(viewport),
            )
            .child(div().text_sm().child(if hardware && failure.is_none() {
                Message::new(Key::NativeGpuPath)
                    .arg(
                        "adapter",
                        specs
                            .as_ref()
                            .map(|specs| specs.device_name.as_str())
                            .unwrap_or_default(),
                    )
                    .display(locale)
            } else {
                text(locale, Key::GpuFailed)
            }))
            .when_some(failure, |view, details| {
                view.child(div().text_sm().child(details))
            })
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(text(locale, Key::NativeGpuHint)),
            )
    }
}

impl Focusable for NativeGpuDemo {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
