//! The one floating toolbar at the bottom of the canvas. 2D and 3D share its
//! groups and positions: view, display, tools, overlays. Buttons show icons
//! only; the name and shortcut appear on hover, and multi-option tools open a
//! menu. The current canvas tool is one of the few places the accent appears;
//! toggled states such as flip or single-layer use a neutral fill.
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

/// A review tool button takes the accent while it is the current tool.
fn tool_button(
    id: &'static str,
    icon: IconName,
    label: impl Into<SharedString>,
    current: bool,
    cx: &App,
) -> Button {
    icon_button(id, icon, label, cx)
        .when(current, |button| {
            button.custom(crate::theme::canvas_toolbar_current(cx))
        })
        .selected(current)
        .toggled(current)
}

pub(super) fn labeled_value(locale: Locale, label: String, value: String) -> SharedString {
    Message::new(Key::LabeledValue)
        .arg("label", label)
        .arg("value", value)
        .display(locale)
        .into()
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
const MEASURE_TOOLS: [(ReviewTool, Key); 4] = [
    (
        ReviewTool::Measure(MeasureKind::Distance),
        Key::MeasureDistanceTool,
    ),
    (
        ReviewTool::Measure(MeasureKind::Polyline),
        Key::MeasurePolylineTool,
    ),
    (ReviewTool::Clearance, Key::ClearanceTool),
    (ReviewTool::PinPath, Key::PinPathTool),
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
                tool_button(
                    "canvas-zoom-box",
                    IconName::ZoomIn,
                    text(locale, Key::ZoomBoxTool),
                    active,
                    cx,
                )
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
                {
                    let active = self.plane_view_active();
                    icon_button(
                        "canvas-plane-view",
                        IconName::SquareStack,
                        text(locale, Key::PlaneViewTool),
                        cx,
                    )
                    .selected(active)
                    .toggled(active)
                    .disabled(
                        !active
                            && !self
                                .nets
                                .report
                                .as_ref()
                                .is_some_and(|(_, report)| report.is_some()),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_plane_view(cx)))
                    .into_any_element()
                },
            ]
        };
        items.push(canvas_separator());
        items.extend(display);
        items.push(self.render_display_button(locale, cx));
        // Tool group: pointer tools first, then review tools.
        let mut tools = self.render_pointer_tools(locale, cx);
        {
            let active = MEASURE_TOOLS
                .iter()
                .filter(|(tool, _)| !in_3d || matches!(tool, ReviewTool::Measure(_)))
                .find(|(tool, _)| self.review.tool == Some(*tool));
            let view = cx.entity().downgrade();
            let current = self.review.tool;
            let label = text(locale, Key::ToolbarMeasure);
            tools.push(
                tool_button(
                    "canvas-measure",
                    IconName::Ruler,
                    active.map_or(label.clone(), |(_, key)| text(locale, *key)),
                    active.is_some(),
                    cx,
                )
                .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, window, _| {
                    menu = menu.min_w(window.rem_size() * 14.0).label(label.clone());
                    for (tool, key) in MEASURE_TOOLS
                        .into_iter()
                        .filter(|(tool, _)| !in_3d || matches!(tool, ReviewTool::Measure(_)))
                    {
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
        }
        if !in_3d {
            let active = self.review.tool == Some(ReviewTool::BoxSelect);
            tools.push(
                tool_button(
                    "canvas-box-select",
                    IconName::SquareDashedMousePointer,
                    text(locale, Key::BoxSelectTool),
                    active,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.set_review_tool(Some(ReviewTool::BoxSelect), window, cx)
                }))
                .into_any_element(),
            );
        }
        tools.push(self.isolate_button(locale, cx));
        tools.push(
            icon_button(
                "canvas-copy-image",
                IconName::Copy,
                text(locale, Key::CopyImageTool),
                cx,
            )
            .disabled(self.review.export.pending)
            .on_click(cx.listener(|this, _, window, cx| this.copy_view_image(window, cx)))
            .into_any_element(),
        );
        let open = self.length_table.open;
        tools.push(
            icon_button(
                "canvas-length-table",
                IconName::Table,
                text(locale, Key::LengthTableTool),
                cx,
            )
            .selected(open)
            .toggled(open)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_length_table(cx)))
            .into_any_element(),
        );
        items.push(canvas_separator());
        items.extend(tools);
        items.push(canvas_separator());
        items.push(self.render_overlay_menu(locale, cx));
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
