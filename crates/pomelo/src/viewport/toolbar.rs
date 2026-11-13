//! Board commands outside the side panels. The window's title row carries the
//! board search, the 2D/3D switch and the panel toggles; the canvas toolbar
//! carries the pointer tools and what a click picks.
use std::rc::Rc;

use gpui_kit::{
    assets::IconName,
    base::Selectable,
    component::{
        ActiveTheme, Icon, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
        menu::{DropdownMenu, PopupMenuItem},
        tab::{Tab, TabBar},
    },
    prelude::FluentBuilder,
    *,
};
use pomelo_core::{
    i18n::{Locale, MessageKey as Key, text},
    interaction::SelectionMode,
    picking::{PickCategory, PickFilter},
};

use super::BoardViewport;
use super::canvas_toolbar::icon_button;
use crate::{panels::picking, tooltips::ButtonTooltipExt, workbench::panel_layout::Side};

/// Selection modes in menu order with their localized names.
pub(super) const SELECTION_MODES: [(SelectionMode, Key); 4] = [
    (SelectionMode::Object, Key::ModeObject),
    (SelectionMode::Track, Key::ModeTrack),
    (SelectionMode::Net, Key::ModeNet),
    (SelectionMode::Component, Key::ModeComponent),
];

/// The board's share of the window title row.
pub(crate) struct TitleControls {
    /// Board search, centered between the document tabs and the trailing controls.
    pub search: AnyElement,
    /// The 2D/3D switch and the side panel toggles.
    pub trailing: AnyElement,
}

impl BoardViewport {
    /// Render the board controls that live in the window title row. The
    /// workbench places them; behavior stays with this viewport.
    pub(crate) fn render_title_controls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TitleControls {
        let locale = crate::i18n::current(cx);
        let panels = crate::workbench::panel_layout::fit_to_window(
            self.panel_layout.read(cx).snapshot(cx),
            window.viewport_size().width.as_f32(),
            0.0,
        );
        let theme = cx.theme();
        let search = div()
            .w_full()
            .max_w(rems(28.0))
            .min_w(rems(12.0))
            .child(
                Input::new(&self.search_input)
                    .small()
                    .prefix(Icon::new(IconName::Search).small())
                    .suffix(
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(crate::shortcuts::shortcut_label(
                                crate::shortcuts::keys::FOCUS_SEARCH,
                            )),
                    )
                    .aria_label(text(locale, Key::SearchBoard)),
            )
            .into_any_element();
        let view3d = self.view3d.active;
        let trailing = div()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .child(
                TabBar::new("board-view-mode")
                    .segmented()
                    .small()
                    .flex_shrink_0()
                    .selected_index(usize::from(view3d))
                    .child(Tab::new().label(text(locale, Key::View2d)))
                    .child(
                        Tab::new()
                            .label(text(locale, Key::View3d))
                            .aria_label(text(locale, Key::View3dPreview))
                            .disabled(!pomelo_render::backend::native::SolidRenderer::supported())
                            .native_tooltip(text(locale, Key::Preview3dHint)),
                    )
                    .on_click(cx.listener(move |this, index: &usize, window, cx| {
                        if (*index == 1) != this.view3d.active {
                            this.toggle_3d(window, cx);
                        }
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_0p5()
                    .child(panel_toggle(Side::Left, !panels.left_collapsed, locale))
                    .child(panel_toggle(Side::Right, !panels.right_collapsed, locale)),
            )
            .into_any_element();
        TitleControls { search, trailing }
    }

    /// Choose the pointer tool. Picking a pointer tool ends any review tool.
    fn set_pointer_tool(&mut self, pan: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.pan_tool = pan;
        self.review.tool = None;
        self.review.active = None;
        self.review.drag = None;
        self.pointer_gesture = None;
        if pan {
            self.invalidate_hover();
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Select (or orbit in 3D), pan, and what a click picks. The current
    /// pointer tool takes the accent, like any current canvas tool.
    pub(super) fn render_pointer_tools(
        &self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let in_3d = self.view3d.active;
        let idle = self.review.tool.is_none();
        let select_current = idle && !self.pan_tool;
        let pan_current = idle && self.pan_tool;
        let select_label = text(
            locale,
            if in_3d {
                Key::OrbitTool
            } else {
                Key::SelectTool
            },
        );
        let pan_label = text(locale, if in_3d { Key::PanTool3d } else { Key::PanTool });
        let select = icon_button(
            "canvas-select",
            if in_3d {
                IconName::Rotate3d
            } else {
                IconName::MousePointer2
            },
            select_label,
            cx,
        )
        .when(select_current, |button| {
            button.custom(crate::theme::canvas_toolbar_current(cx))
        })
        .selected(select_current)
        .toggled(select_current)
        .on_click(cx.listener(|this, _, window, cx| this.set_pointer_tool(false, window, cx)));
        let pan = icon_button("canvas-pan", IconName::Hand, pan_label, cx)
            .when(pan_current, |button| {
                button.custom(crate::theme::canvas_toolbar_current(cx))
            })
            .selected(pan_current)
            .toggled(pan_current)
            .on_click(cx.listener(|this, _, window, cx| this.set_pointer_tool(true, window, cx)));
        let view = cx.entity().downgrade();
        let pick = pick_menu(
            locale,
            self.selection_mode,
            self.pick_filter,
            {
                let view = view.clone();
                Box::new(
                    move |mode: &SelectionMode, window: &mut Window, cx: &mut App| {
                        let _ =
                            view.update(cx, |this, cx| this.set_selection_mode(*mode, window, cx));
                    },
                )
            },
            Box::new(
                move |(category, enabled): &(PickCategory, bool), _: &mut Window, cx: &mut App| {
                    let _ = view.update(cx, |this, cx| {
                        this.set_pick_category(*category, *enabled, cx)
                    });
                },
            ),
            cx,
        );
        vec![
            select.into_any_element(),
            pan.into_any_element(),
            pick.into_any_element(),
        ]
    }
}

type ModeHandler = Box<dyn Fn(&SelectionMode, &mut Window, &mut App)>;
type CategoryHandler = Box<dyn Fn(&(PickCategory, bool), &mut Window, &mut App)>;

/// Selection mode and pick filters share one menu because both decide what a
/// canvas click selects. A restricted filter keeps the trigger filled, and a
/// filter that blocks the mode is flagged, so the state shows without opening.
fn pick_menu(
    locale: Locale,
    mode: SelectionMode,
    filter: PickFilter,
    on_mode: ModeHandler,
    on_category: CategoryHandler,
    cx: &App,
) -> impl IntoElement {
    let on_mode = Rc::new(on_mode);
    let on_category = Rc::new(on_category);
    let mode_label = SELECTION_MODES
        .iter()
        .find(|(candidate, _)| *candidate == mode)
        .map_or(Key::ModeObject, |(_, key)| *key);
    let hint = picking::filter_hint(filter, mode);
    let restricted = filter != PickFilter::all();
    let label = super::canvas_toolbar::labeled_value(
        locale,
        text(locale, Key::PickMode),
        text(locale, mode_label),
    );
    icon_button(
        "canvas-pick-mode",
        if hint.is_some() {
            IconName::TriangleAlert
        } else if restricted {
            IconName::ListFilter
        } else {
            IconName::MousePointerClick
        },
        label,
        cx,
    )
    .selected(restricted)
    .dropdown_menu_with_anchor(Anchor::BottomLeft, move |mut menu, window, _| {
        menu = menu
            .min_w(window.rem_size() * 14.0)
            .label(text(locale, Key::PickMode));
        for (candidate, key) in SELECTION_MODES {
            let on_mode = Rc::clone(&on_mode);
            menu = menu.item(
                PopupMenuItem::new(text(locale, key))
                    .checked(candidate == mode)
                    .on_click(move |_, window, cx| on_mode(&candidate, window, cx)),
            );
        }
        menu = menu.separator().label(text(locale, Key::PickCategories));
        for (category, key) in picking::CATEGORIES {
            let on_category = Rc::clone(&on_category);
            let enabled = filter.contains(category);
            menu = menu.item(
                PopupMenuItem::new(text(locale, key))
                    .checked(enabled)
                    .on_click(move |_, window, cx| on_category(&(category, !enabled), window, cx)),
            );
        }
        if let Some(hint) = hint {
            menu = menu.separator().label(text(locale, hint));
        }
        menu
    })
}

fn panel_toggle(side: Side, open: bool, locale: Locale) -> Button {
    crate::panels::sidebar::toggle_button(side, !open, locale, move |_, window, cx| {
        let action: Box<dyn Action> = match side {
            Side::Left => Box::new(crate::actions::ToggleLeftPanel),
            Side::Right => Box::new(crate::actions::ToggleRightPanel),
        };
        window.dispatch_action(action, cx)
    })
    .selected(open)
}
