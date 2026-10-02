//! Explicit diagnostic mode: wgpu readback composited by GPUI, isolated from the viewer.

use std::sync::Arc;

use gpui_kit::base::Disableable;
use gpui_kit::component::{ActiveTheme, Theme, ThemeMode, button::Button};
use gpui_kit::*;
use pomelo_core::{
    i18n::{Message, MessageKey as Key, text},
    model::Diagnostic,
};
use pomelo_render::triangle::{ProbeBackend, render_triangle};

use crate::i18n;

actions!(pomelo_gpu, [RenderTriangle]);

enum DemoState {
    Loading,
    Ready {
        image: Arc<RenderImage>,
        adapter: String,
        backend: String,
    },
    Failed(Diagnostic),
}

pub struct GpuDemo {
    state: DemoState,
    task: Option<Task<()>>,
    focus: FocusHandle,
    dark: bool,
}

impl GpuDemo {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut demo = Self {
            state: DemoState::Loading,
            task: None,
            focus: cx.focus_handle(),
            dark: true,
        };
        demo.start(window, cx);
        demo
    }

    fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state = DemoState::Loading;
        let worker = cx.background_spawn(async {
            let frame = render_triangle(ProbeBackend::native(), 768, 512)
                .map_err(|error| error.diagnostic())?;
            // GPUI's RenderImage consumes BGRA, whereas PNG/probe data remains RGBA.
            let mut bgra = frame.rgba;
            for pixel in bgra.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            let pixels = image::RgbaImage::from_raw(frame.report.width, frame.report.height, bgra)
                .ok_or_else(|| Diagnostic::error("GPU_IMAGE_SIZE_INVALID", Key::GpuFailed))?;
            Ok::<_, Diagnostic>(DemoState::Ready {
                image: Arc::new(RenderImage::new([image::Frame::new(pixels)])),
                adapter: frame.report.adapter,
                backend: frame.report.backend,
            })
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                this.state = result.unwrap_or_else(DemoState::Failed);
                this.task = None;
                cx.notify();
            });
        }));
        cx.notify();
    }
}

impl Render for GpuDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let locale = i18n::current(cx);
        let mut canvas = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .items_center()
            .justify_center()
            .border_1()
            .border_color(theme.border)
            .overflow_hidden();
        match &self.state {
            DemoState::Loading => canvas = canvas.child(text(locale, Key::PreparingGpu)),
            DemoState::Ready { image, .. } => {
                canvas = canvas.child(
                    img(image.clone())
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                )
            }
            DemoState::Failed(error) => {
                canvas = canvas.child(
                    div()
                        .flex()
                        .flex_col()
                        .p_6()
                        .gap_3()
                        .child(
                            div()
                                .text_color(theme.danger)
                                .child(error.message.display(locale)),
                        )
                        .child(text(locale, Key::TechnicalDetails))
                        .children(
                            error
                                .technical_details
                                .as_ref()
                                .map(|details| details.to_string()),
                        ),
                );
            }
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .bg(theme.background)
            .text_color(theme.foreground)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &RenderTriangle, window, cx| {
                if !matches!(this.state, DemoState::Loading) {
                    this.start(window, cx);
                }
            }))
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .text_lg()
                            .child(text(locale, Key::GpuDemoTitle)),
                    )
                    .child(
                        Button::new("gpu-rerender")
                            .outline()
                            .label(text(locale, Key::GpuDemoRerender))
                            .disabled(matches!(self.state, DemoState::Loading))
                            .on_click(cx.listener(|this, _, window, cx| this.start(window, cx))),
                    ),
            )
            .child(canvas)
            .children(match &self.state {
                DemoState::Ready {
                    adapter, backend, ..
                } => Some(
                    div().text_sm().child(
                        Message::new(Key::GpuDemoReady)
                            .arg("adapter", adapter.as_str())
                            .arg("backend", backend.as_str())
                            .display(locale),
                    ),
                ),
                _ => None,
            })
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(text(locale, Key::GpuDemoReadback)),
            )
    }
}

impl Focusable for GpuDemo {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
