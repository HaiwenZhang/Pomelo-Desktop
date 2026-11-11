//! The one floating toolbar at the bottom of the canvas. 2D and 3D share its
//! groups and positions: view, display, tools. Buttons show icons only; the
//! name and shortcut appear on hover, and multi-option tools open a menu.
use super::layout::canvas_separator;
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use pomelo_core::{i18n::Locale, review::measure::MeasureKind};
use review::ReviewTool;

pub(super) fn icon_button(
    id: &'static str,
    icon: IconName,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let label = label.into();
    Button::new(id)
        .custom(crate::theme::canvas_toolbar_button(cx))
        .small()
        .size_8()
        .icon(icon)
        .accessibility_label(label.clone())
        .native_tooltip(label)
}

fn with_shortcut(locale: Locale, key: Key, shortcut: &str) -> SharedString {
    Message::new(Key::WithShortcut)
        .arg("label", text(locale, key))
        .arg("shortcut", crate::shortcuts::shortcut_label(shortcut))
        .display(locale)
        .into()
}

/// Measurement kinds behind the toolbar's measure menu, in menu order. Their
/// names already carry the single-key shortcut.
const MEASURE_TOOLS: [(ReviewTool, Key); 3] = [
    (
        ReviewTool::Measure(MeasureKind::Distance),
        Key::MeasureDistanceTool,
    ),
    (
        ReviewTool::Measure(MeasureKind::Polyline),
        Key::MeasurePolylineTool,
    ),
    (ReviewTool::Clearance, Key::ClearanceTool),
];

impl BoardViewport {
    fn isolate_button(&self, locale: Locale, cx: &mut Context<Self>) -> AnyElement {
        let active = self.isolation_active();
        icon_button(
            "canvas-isolate",
            IconName::Focus,
            text(
                locale,
                if active {
                    Key::IsolateEnd
                } else {
                    Key::IsolateSelection
                },
            ),
            cx,
        )
        .selected(active)
        .toggled(active)
        .disabled(!active && self.selected_target.is_none() && self.review.set.is_empty())
        .on_click(cx.listener(|this, _, _, cx| this.toggle_isolation(cx)))
        .into_any_element()
    }

    /// Fit the selection when there is one, otherwise the whole board.
    pub(super) fn fit_view(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(target) = self.selected_target {
            self.locate_target(target, window, cx);
        } else {
            self.fit_board(&FitBoard, window, cx);
        }
    }

    pub(super) fn render_canvas_toolbar(
        &self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let in_3d = self.view3d.active;
        let mut items = Vec::new();
        // View group.
        let fit = if self.selected_target.is_some() {
            Key::FitSelection
        } else {
            Key::FitBoard
        };
        items.push(
            icon_button(
                "canvas-fit",
                IconName::Scan,
                with_shortcut(locale, fit, crate::shortcuts::keys::FIT_ACTIVE_BOARD),
                cx,
            )
            .on_click(cx.listener(|this, _, window, cx| this.fit_view(window, cx)))
            .into_any_element(),
        );
        if !in_3d {
            let active = self.review.tool == Some(ReviewTool::ZoomBox);
            items.push(
                icon_button(
                    "canvas-zoom-box",
                    IconName::ZoomIn,
                    text(locale, Key::ZoomBoxTool),
                    cx,
                )
                .selected(active)
                .toggled(active)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_review_tool(Some(ReviewTool::ZoomBox), window, cx)
                }))
                .into_any_element(),
            );
        }
        let flipped = !in_3d && self.navigation.camera().flipped;
        items.push(
            icon_button(
                "canvas-flip",
                IconName::FlipHorizontal2,
                text(locale, Key::FlipBoard),
                cx,
            )
            .selected(flipped)
            .toggled(flipped)
            .on_click(cx.listener(|this, _, window, cx| this.flip_board(&FlipBoard, window, cx)))
            .into_any_element(),
        );
        let can_go_back = if in_3d {
            !self.view3d.history.is_empty()
        } else {
            self.review.history.can_go_back()
        };
        items.push(
            icon_button(
                "canvas-back",
                IconName::Undo2,
                with_shortcut(
                    locale,
                    Key::NavigateBack,
                    crate::shortcuts::keys::NAVIGATE_BACK,
                ),
                cx,
            )
            .disabled(!can_go_back)
            .on_click(
                cx.listener(|this, _, window, cx| this.navigate_back(&NavigateBack, window, cx)),
            )
            .into_any_element(),
        );
        // Display group.
        let display = if in_3d {
            self.render_3d_display_tools(locale, cx)
        } else {
            let single = self.display.shadow.enabled;
            vec![
                icon_button(
                    "canvas-single-layer",
                    IconName::Layers2,
                    Message::new(Key::SingleLayerTool)
                        .arg("shortcut", "L")
                        .display(locale),
                    cx,
                )
                .selected(single)
                .toggled(single)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_single_layer(cx)))
                .into_any_element(),
                self.isolate_button(locale, cx),
            ]
        };
        if !display.is_empty() {
            items.push(canvas_separator());
            items.extend(display);
        }
        // Tool group.
        let mut tools = Vec::new();
        if !in_3d {
            let active = MEASURE_TOOLS
                .iter()
                .find(|(tool, _)| self.review.tool == Some(*tool));
            let view = cx.entity().downgrade();
            let current = self.review.tool;
            let label = text(locale, Key::ToolbarMeasure);
            tools.push(
                icon_button(
                    "canvas-measure",
                    IconName::Ruler,
                    active.map_or(label.clone(), |(_, key)| text(locale, *key)),
                    cx,
                )
                .selected(active.is_some())
                .toggled(active.is_some())
                .dropdown_menu(move |mut menu, window, _| {
                    menu = menu.min_w(window.rem_size() * 14.0).label(label.clone());
                    for (tool, key) in MEASURE_TOOLS {
                        let view = view.clone();
                        menu = menu.item(
                            PopupMenuItem::new(text(locale, key))
                                .checked(current == Some(tool))
                                .on_click(move |_, window, cx| {
                                    let _ = view.update(cx, |this, cx| {
                                        this.set_review_tool(Some(tool), window, cx)
                                    });
                                }),
                        );
                    }
                    menu
                })
                .into_any_element(),
            );
            let active = self.review.tool == Some(ReviewTool::BoxSelect);
            tools.push(
                icon_button(
                    "canvas-box-select",
                    IconName::SquareDashedMousePointer,
                    text(locale, Key::BoxSelectTool),
                    cx,
                )
                .selected(active)
                .toggled(active)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_review_tool(Some(ReviewTool::BoxSelect), window, cx)
                }))
                .into_any_element(),
            );
        }
        if in_3d {
            tools.push(self.isolate_button(locale, cx));
        }
        if !tools.is_empty() {
            items.push(canvas_separator());
            items.extend(tools);
        }
        if self.review.tool.is_some() {
            items.push(
                icon_button(
                    "canvas-cancel-tool",
                    IconName::X,
                    with_shortcut(locale, Key::CancelTool, "escape"),
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.cancel_canvas_action(&CancelCanvasAction, window, cx);
                }))
                .into_any_element(),
            );
        }
        items
    }
}
