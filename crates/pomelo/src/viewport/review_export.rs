//! View screenshots (PNG/SVG) and review summaries (CSV), written to a path the user
//! chose. Content is built from immutable snapshots off the UI thread.
use super::*;
use pomelo_core::{
    review::export::{ExportContent, ExportOptions, review_csv},
    task::CancellationToken,
};
use pomelo_render::scene::snapshot::{self, Marks, SnapshotRequest};
use std::path::PathBuf;

/// Longest edge of a whole-board image.
const BOARD_IMAGE_EDGE: f64 = 4096.0;

pub(super) struct ExportSession {
    pub scope_board: bool,
    pub markers: bool,
    pub anonymize: bool,
    pub pending: bool,
    /// The last outcome, and whether it was a failure.
    pub notice: Option<(Message, bool)>,
    pub cancel: CancellationToken,
    pub task: Option<Task<()>>,
}

impl Default for ExportSession {
    fn default() -> Self {
        Self {
            scope_board: false,
            markers: true,
            anonymize: false,
            pending: false,
            notice: None,
            cancel: CancellationToken::default(),
            task: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ImageFormat {
    Png,
    Svg,
}

/// Board range and output size of an image export.
#[derive(Clone, Copy)]
pub(super) struct ImageFrame {
    pub area: pomelo_core::model::Bounds,
    pub width: u32,
    pub height: u32,
}

fn rgba(color: Hsla) -> [f32; 4] {
    let color: Rgba = color.into();
    [color.r, color.g, color.b, color.a]
}

fn default_directory() -> PathBuf {
    dirs::picture_dir()
        .or_else(dirs::document_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

enum Outcome {
    Saved(PathBuf),
    Failed(PathBuf),
    TooLarge,
    Cancelled,
}

impl BoardViewport {
    /// The visible board range at device resolution, or the whole board.
    pub(super) fn image_frame(&self, scale_factor: f32) -> Option<ImageFrame> {
        if self.review.export.scope_board {
            let bounds = self.scene.bounds;
            if !bounds.is_valid() {
                return None;
            }
            let span_x = (bounds.max.x - bounds.min.x).max(1e-3);
            let span_y = (bounds.max.y - bounds.min.y).max(1e-3);
            let margin = span_x.max(span_y) * 0.03;
            let area = pomelo_core::model::Bounds {
                min: pomelo_core::model::Point::new(bounds.min.x - margin, bounds.min.y - margin),
                max: pomelo_core::model::Point::new(bounds.max.x + margin, bounds.max.y + margin),
            };
            let (w, h) = (span_x + 2.0 * margin, span_y + 2.0 * margin);
            let scale = BOARD_IMAGE_EDGE / w.max(h);
            return Some(ImageFrame {
                area,
                width: (w * scale).round().max(1.0) as u32,
                height: (h * scale).round().max(1.0) as u32,
            });
        }
        let size = self.navigation.size();
        if size.x <= 0.0 || size.y <= 0.0 {
            return None;
        }
        let camera = self.navigation.camera();
        let a = camera.view_to_board(pomelo_core::model::Point::new(0.0, 0.0), size.x, size.y);
        let b = camera.view_to_board(size, size.x, size.y);
        let scale = f64::from(scale_factor.max(1.0));
        let edge = f64::from(snapshot::MAX_EDGE);
        Some(ImageFrame {
            area: pomelo_core::model::Bounds {
                min: pomelo_core::model::Point::new(a.x.min(b.x), a.y.min(b.y)),
                max: pomelo_core::model::Point::new(a.x.max(b.x), a.y.max(b.y)),
            },
            width: (size.x * scale).round().clamp(1.0, edge) as u32,
            height: (size.y * scale).round().clamp(1.0, edge) as u32,
        })
    }

    fn snapshot_layer_order(&self) -> Vec<pomelo_core::model::LayerId> {
        let ids = self
            .scene
            .layers
            .iter()
            .map(|layer| layer.id)
            .chain(self.scene.special_layers.iter().map(|layer| layer.id))
            .chain(self.scene.drawing_layers.iter().map(|layer| layer.id));
        self.display.ordered_layers(ids)
    }

    fn prompt_save_path(
        &mut self,
        suggested: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
        save: impl FnOnce(&mut Self, PathBuf, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let receiver = cx.prompt_for_new_path(&default_directory(), Some(suggested));
        cx.spawn_in(window, async move |this, cx| {
            let result = receiver.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(Ok(Some(path))) => save(this, path, window, cx),
                Ok(Ok(None)) => {}
                _ => {
                    this.review.export.notice = Some((Message::new(Key::FileDialogFailed), true));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn export_image(
        &mut self,
        format: ImageFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let suggested = match format {
            ImageFormat::Png => "pomelo-view.png",
            ImageFormat::Svg => "pomelo-view.svg",
        };
        self.prompt_save_path(suggested, window, cx, move |this, path, window, cx| {
            this.write_image(format, path, window, cx);
        });
    }

    fn write_image(
        &mut self,
        format: ImageFormat,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(frame) = self.image_frame(window.scale_factor()) else {
            return;
        };
        let scene = Arc::clone(&self.scene);
        let source_glyphs = self.texts.clone();
        let source_strokes = self.source_strokes.clone();
        let label_index = self.label_index.clone();
        let pixel_ratio = if self.review.export.scope_board {
            1.0
        } else {
            f64::from(window.scale_factor())
        };
        let display = Arc::clone(&self.display);
        let layer_order = self.snapshot_layer_order();
        let colors = Arc::clone(&self.colors);
        let object_colors = self.review.object_colors.clone();
        let markers = self.review.export.markers;
        let selection = markers.then(|| self.related_highlight());
        let measurements: Vec<Vec<_>> = if markers {
            self.review
                .measurements
                .iter()
                .chain(self.review.active.iter())
                .filter(|measurement| measurement.points.len() >= 2)
                .map(|measurement| measurement.points.iter().map(|p| p.position).collect())
                .collect()
        } else {
            Vec::new()
        };
        let clearance = self
            .review
            .clearance
            .filter(|_| markers)
            .map(|record| (record.result.a, record.result.b));
        let background = rgba(self.canvas_background());
        let mark_color = rgba(crate::theme::canvas_measure());
        let outline_color = rgba(crate::theme::canvas_colors().1);
        let unit = self.display.length_unit;
        let flipped = !self.review.export.scope_board && self.navigation.camera().flipped;
        let cancel = self.start_export();
        let worker_cancel = cancel.clone();
        let worker_path = path.clone();
        let worker = cx.background_spawn(async move {
            let request = SnapshotRequest {
                scene: &scene,
                display: &display,
                area: frame.area,
                width: frame.width,
                height: frame.height,
                pixel_ratio,
                annotations: snapshot::AnnotationSources {
                    source_glyphs: source_glyphs.as_deref(),
                    source_strokes: source_strokes.as_deref(),
                    label_index: label_index.as_deref(),
                },
                flipped,
                layer_order: &layer_order,
                colors: &colors,
                fallback_color: [0.6, 0.68, 0.73, 1.0],
                drill_color: [0.46, 0.49, 0.51, 1.0],
                outline_color,
                background,
                mark_color,
                object_colors: object_colors.as_deref(),
                marks: Marks {
                    selection: selection.as_deref(),
                    measurements,
                    clearance,
                },
                scale_unit: Some(unit),
                max_elements: snapshot::MAX_ELEMENTS,
            };
            let svg = match snapshot::svg(&request, &worker_cancel) {
                Ok(svg) => svg,
                Err(snapshot::SnapshotError::Cancelled) => return Outcome::Cancelled,
                Err(_) => return Outcome::TooLarge,
            };
            let bytes = match format {
                ImageFormat::Svg => svg.into_bytes(),
                ImageFormat::Png => match crate::services::export::rasterize_png(&svg) {
                    Ok(bytes) => bytes,
                    Err(_) => return Outcome::Failed(worker_path),
                },
            };
            if worker_cancel.is_cancelled() {
                return Outcome::Cancelled;
            }
            match crate::services::export::write_atomically(&worker_path, &bytes) {
                Ok(()) => Outcome::Saved(worker_path),
                Err(_) => Outcome::Failed(worker_path),
            }
        });
        self.finish_export(cancel, worker, window, cx);
    }

    pub(super) fn export_csv(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_save_path("pomelo-review.csv", window, cx, |this, path, window, cx| {
            this.write_csv(path, window, cx);
        });
    }

    fn write_csv(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let scene = Arc::clone(&self.scene);
        let measurements = self.review.measurements.clone();
        let clearance = self.review.clearance;
        let selection = Arc::clone(self.review.set.objects());
        let options = ExportOptions {
            unit: self.display.length_unit,
            anonymize: self.review.export.anonymize,
            locale: i18n::current(cx),
        };
        let source_name = self.source_name.clone();
        let cancel = self.start_export();
        let worker_cancel = cancel.clone();
        let worker = cx.background_spawn(async move {
            let content = ExportContent {
                source_name: Some(&source_name),
                measurements: &measurements,
                clearance,
                selection: &selection,
            };
            let csv = match review_csv(&scene, &content, options, &worker_cancel) {
                Ok(csv) => csv,
                Err(_) => return Outcome::Cancelled,
            };
            match crate::services::export::write_atomically(&path, csv.as_bytes()) {
                Ok(()) => Outcome::Saved(path),
                Err(_) => Outcome::Failed(path),
            }
        });
        self.finish_export(cancel, worker, window, cx);
    }

    fn start_export(&mut self) -> CancellationToken {
        let export = &mut self.review.export;
        export.cancel.cancel();
        export.cancel = CancellationToken::default();
        export.pending = true;
        export.notice = None;
        export.cancel.clone()
    }

    fn finish_export(
        &mut self,
        cancel: CancellationToken,
        worker: Task<Outcome>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.review.export.task = Some(cx.spawn_in(window, async move |this, cx| {
            let outcome = worker.await;
            let _ = this.update_in(cx, |this, _, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                let export = &mut this.review.export;
                export.pending = false;
                let display = |path: &PathBuf| path.display().to_string();
                export.notice = match outcome {
                    Outcome::Saved(path) => Some((
                        Message::new(Key::ExportSaved).arg("path", display(&path)),
                        false,
                    )),
                    Outcome::Failed(path) => Some((
                        Message::new(Key::ExportFailed).arg("path", display(&path)),
                        true,
                    )),
                    Outcome::TooLarge => Some((Message::new(Key::ExportTooLarge), true)),
                    Outcome::Cancelled => None,
                };
                cx.notify();
            });
        }));
        cx.notify();
    }
}
