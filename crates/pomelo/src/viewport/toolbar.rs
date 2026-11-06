//! Board commands grouped by file access, canvas tool, picking, search, coloring and panels.
use std::rc::Rc;

use gpui_kit::{
    assets::IconName,
    base::Selectable,
    component::{
        ActiveTheme, Icon, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
        menu::{DropdownMenu, PopupMenuItem},
        separator::Separator,
        tab::{Tab, TabBar},
    },
    prelude::FluentBuilder,
    *,
};
use pomelo_core::{
    display::ColorMode,
    i18n::{Locale, MessageKey as Key, text},
    interaction::SelectionMode,
    picking::{PickCategory, PickFilter},
};

use crate::{panels::picking, tooltips::ButtonTooltipExt, workbench::panel_layout::Side};

pub(super) struct State<'a> {
    pub locale: Locale,
    pub pan_tool: bool,
    pub view3d: bool,
    pub color_mode: ColorMode,
    pub selection_mode: SelectionMode,
    pub pick_filter: PickFilter,
    pub left_panel_open: bool,
    pub right_panel_open: bool,
    pub search: &'a Entity<InputState>,
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
/// Receives the requested state: 3D/2D or pan/select.
type ToolHandler = Box<dyn Fn(&bool, &mut Window, &mut App)>;
type ColorHandler = Box<dyn Fn(&ColorMode, &mut Window, &mut App)>;
type ModeHandler = Box<dyn Fn(&SelectionMode, &mut Window, &mut App)>;
type CategoryHandler = Box<dyn Fn(&(PickCategory, bool), &mut Window, &mut App)>;

pub(super) struct Commands {
    pub view3d: ToolHandler,
    pub tool: ToolHandler,
    pub colors: ColorHandler,
    pub fit: ClickHandler,
    pub pick_mode: ModeHandler,
    pub pick_category: CategoryHandler,
}

/// Selection modes in menu order with their localized names.
pub(super) const SELECTION_MODES: [(SelectionMode, Key); 4] = [
    (SelectionMode::Object, Key::ModeObject),
    (SelectionMode::Track, Key::ModeTrack),
    (SelectionMode::Net, Key::ModeNet),
    (SelectionMode::Component, Key::ModeComponent),
];

pub(super) fn render(state: State<'_>, commands: Commands, cx: &App) -> AnyElement {
    let locale = state.locale;
    let colors = Rc::new(commands.colors);
    let separator = || Separator::vertical().h_5().flex_shrink_0();
    let shortcut = |keystroke: &str| {
        div()
            .flex_shrink_0()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(crate::shortcuts::shortcut_label(keystroke))
    };
    let pan_tool = state.pan_tool;
    let view3d = state.view3d;
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .h_11()
        .flex_shrink_0()
        .border_b_1()
        .border_color(cx.theme().border)
        .child(
            Button::new("board-open")
                .ghost()
                .small()
                .flex_shrink_0()
                .icon(IconName::FolderOpen)
                .label(text(locale, Key::OpenFile))
                .native_tooltip(format!(
                    "{} ({})",
                    text(locale, Key::OpenFile),
                    crate::shortcuts::shortcut_label(crate::shortcuts::keys::OPEN_FILE)
                ))
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(crate::actions::OpenFile), cx)
                }),
        )
        .child(separator())
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
                .on_click(move |index, window, cx| {
                    if (*index == 1) != view3d {
                        (commands.view3d)(&(*index == 1), window, cx);
                    }
                }),
        )
        .child(separator())
        .child(
            TabBar::new("canvas-tool")
                .segmented()
                .small()
                .flex_shrink_0()
                .selected_index(usize::from(pan_tool))
                .child(
                    Tab::new()
                        .icon(IconName::MousePointer2)
                        .aria_label(text(locale, Key::SelectTool))
                        .native_tooltip(text(locale, Key::SelectTool)),
                )
                .child(
                    Tab::new()
                        .icon(IconName::Hand)
                        .aria_label(text(locale, Key::PanTool))
                        .native_tooltip(text(locale, Key::PanTool)),
                )
                .on_click(move |index, window, cx| (commands.tool)(&(*index == 1), window, cx)),
        )
        .child(pick_menu(
            locale,
            state.selection_mode,
            state.pick_filter,
            commands.pick_mode,
            commands.pick_category,
            cx,
        ))
        .child(
            div().flex_1().min_w_0().flex().justify_center().child(
                div().w_full().max_w(rems(36.0)).child(
                    Input::new(state.search)
                        .small()
                        .prefix(Icon::new(IconName::Search).small())
                        .suffix(shortcut(crate::shortcuts::keys::FOCUS_SEARCH))
                        .aria_label(text(locale, Key::SearchBoard)),
                ),
            ),
        )
        .child(
            TabBar::new("board-color-mode")
                .segmented()
                .small()
                .flex_shrink_0()
                .selected_index(match state.color_mode {
                    ColorMode::Layer => 0,
                    ColorMode::Net => 1,
                })
                .child(Tab::new().label(text(locale, Key::LayerColor)))
                .child(Tab::new().label(text(locale, Key::NetColor)))
                .on_click(move |index, window, cx| {
                    let mode = if *index == 0 {
                        ColorMode::Layer
                    } else {
                        ColorMode::Net
                    };
                    colors(&mode, window, cx)
                }),
        )
        .child(separator())
        .child(
            Button::new("fit-board")
                .ghost()
                .small()
                .flex_shrink_0()
                .icon(IconName::Scan)
                .label(text(locale, Key::FitBoard))
                .child(shortcut(crate::shortcuts::keys::FIT_ACTIVE_BOARD))
                .on_click(commands.fit),
        )
        .child(separator())
        .child(panel_toggle(Side::Left, state.left_panel_open, locale))
        .child(panel_toggle(Side::Right, state.right_panel_open, locale))
        .into_any_element()
}

/// Selection mode and pick filters share one menu because both decide what a
/// canvas click selects. The trigger names the current mode and marks a
/// restricted filter so the state is visible without opening the menu.
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
    Button::new("pick-mode")
        .outline()
        .small()
        .flex_shrink_0()
        .dropdown_caret(true)
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(text(locale, Key::PickMode)),
                )
                .child(text(locale, mode_label))
                .when(restricted, |label| {
                    label.child(
                        Icon::new(if hint.is_some() {
                            IconName::TriangleAlert
                        } else {
                            IconName::ListFilter
                        })
                        .xsmall()
                        .text_color(if hint.is_some() {
                            cx.theme().warning
                        } else {
                            cx.theme().muted_foreground
                        }),
                    )
                }),
        )
        .accessibility_label(text(locale, Key::PickModeTooltip))
        .dropdown_menu(move |mut menu, window, _| {
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
                        .on_click(move |_, window, cx| {
                            on_category(&(category, !enabled), window, cx)
                        }),
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
