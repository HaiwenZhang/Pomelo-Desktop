//! Display settings grouped by intent; mutations and renderer invalidation belong to the viewport.
use super::focus_scroll::FocusScroll;
use gpui_kit::*;
use gpui_kit::{
    assets::IconName,
    base::Disableable,
    component::{ActiveTheme, Sizable, button::Button, switch::Switch, tooltip::Tooltip},
};
use pomelo_core::{
    display::{BoardDisplay, LabelKind},
    i18n::{Locale, MessageKey as Key, text},
};
type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
type VisibilityHandler = Box<dyn Fn(&bool, &mut Window, &mut App)>;
type LabelHandler = Box<dyn Fn(&(LabelKind, bool), &mut Window, &mut App)>;
pub struct DisplayCommands {
    pub net_colors: VisibilityHandler,
    pub grid: VisibilityHandler,
    pub shadow: VisibilityHandler,
    pub dim_active: VisibilityHandler,
    pub filled: VisibilityHandler,
    pub drills: VisibilityHandler,
    pub backdrills: VisibilityHandler,
    pub copper: VisibilityHandler,
    pub static_shapes_fill_solid: VisibilityHandler,
    pub texts: VisibilityHandler,
    pub horizontal_pin_names: VisibilityHandler,
    pub labels: LabelHandler,
    pub active_layer_labels_only: VisibilityHandler,
    pub drawings: VisibilityHandler,
    pub reset_order: ClickHandler,
}

/// Settings sections, ending with the caller's opacity and color controls.
pub fn controls(
    locale: Locale,
    state: &BoardDisplay,
    commands: DisplayCommands,
    appearance: Vec<AnyElement>,
    object_filter: Vec<AnyElement>,
    scroll: &ScrollHandle,
    cx: &App,
) -> Vec<AnyElement> {
    let labels = std::rc::Rc::new(commands.labels);
    // Grouped switches use short labels: the group heading already says what
    // they show. Assistive tools still receive each setting's full name.
    let switch =
        |id: &'static str, short: Key, full: Key, checked: bool, handler: VisibilityHandler| {
            setting_switch(
                id,
                text(locale, short),
                text(locale, full),
                checked,
                handler,
                scroll,
            )
        };
    let label_switch = |kind: LabelKind| {
        let (id, short, full) = match kind {
            LabelKind::TrackNames => ("track-net-names", Key::LayerTraces, Key::TrackNetNames),
            LabelKind::PinNames => ("pin-net-names", Key::LayerPads, Key::PinNetNames),
            LabelKind::ViaNames => ("via-net-names", Key::LayerVias, Key::ViaNetNames),
            LabelKind::ZoneNames => ("zone-net-names", Key::PickCopper, Key::ZoneNetNames),
            LabelKind::ThroughSpans => (
                "through-via-labels",
                Key::ShortThroughVias,
                Key::ThroughViaLabels,
            ),
            LabelKind::BlindBuriedSpans => (
                "blind-buried-via-labels",
                Key::ShortBlindBuriedVias,
                Key::BlindBuriedViaLabels,
            ),
        };
        let labels = labels.clone();
        setting_switch(
            id,
            text(locale, short),
            text(locale, full),
            state.label_options.enabled(kind),
            Box::new(move |enabled, window, cx| labels(&(kind, *enabled), window, cx)),
            scroll,
        )
    };
    vec![
        section(
            text(locale, Key::DisplayShow),
            vec![switch_grid(vec![
                switch(
                    "show-grid",
                    Key::ShowGrid,
                    Key::ShowGrid,
                    state.grid.visible,
                    commands.grid,
                ),
                switch(
                    "show-copper",
                    Key::PickCopper,
                    Key::ShowCopper,
                    state.show_copper,
                    commands.copper,
                ),
                switch(
                    "show-drills",
                    Key::ShortDrills,
                    Key::ShowDrills,
                    state.show_drills,
                    commands.drills,
                ),
                switch(
                    "show-backdrills",
                    Key::ShortBackdrills,
                    Key::ShowBackdrills,
                    state.show_backdrills,
                    commands.backdrills,
                ),
                switch(
                    "show-texts",
                    Key::ShortBoardText,
                    Key::ShowTexts,
                    state.show_texts,
                    commands.texts,
                ),
                switch(
                    "show-drawings",
                    Key::ShortAuxGraphics,
                    Key::ShowDrawings,
                    state.show_drawings,
                    commands.drawings,
                ),
                switch(
                    "fill-pads",
                    Key::FillPads,
                    Key::FillPads,
                    state.filled,
                    commands.filled,
                ),
            ])],
            cx,
        ),
        section(
            text(locale, Key::DisplayNetLabels),
            vec![
                switch(
                    "active-layer-labels-only",
                    Key::ActiveLayerLabelsOnly,
                    Key::ActiveLayerLabelsOnly,
                    state.active_layer_labels_only,
                    commands.active_layer_labels_only,
                ),
                switch_grid(vec![
                    label_switch(LabelKind::TrackNames),
                    label_switch(LabelKind::PinNames),
                    label_switch(LabelKind::ViaNames),
                    label_switch(LabelKind::ZoneNames),
                ]),
                switch(
                    "horizontal-pin-names",
                    Key::HorizontalPinNames,
                    Key::HorizontalPinNames,
                    state.horizontal_pin_names,
                    commands.horizontal_pin_names,
                ),
            ],
            cx,
        ),
        section(
            text(locale, Key::DisplayViaSpans),
            vec![switch_grid(vec![
                label_switch(LabelKind::ThroughSpans),
                label_switch(LabelKind::BlindBuriedSpans),
            ])],
            cx,
        ),
        section(
            text(locale, Key::DisplayRendering),
            vec![
                switch(
                    "show-custom-net-colors",
                    Key::ShowCustomNetColors,
                    Key::ShowCustomNetColors,
                    !state.appearance.hide_net_colors,
                    commands.net_colors,
                ),
                switch(
                    "shadow-mode",
                    Key::ShadowMode,
                    Key::ShadowMode,
                    state.shadow.enabled,
                    commands.shadow,
                ),
                switch(
                    "dim-active-layer",
                    Key::DimActiveLayer,
                    Key::DimActiveLayer,
                    state.shadow.dim_active,
                    commands.dim_active,
                ),
                switch(
                    "static-shapes-fill-solid",
                    Key::StaticShapesFillSolid,
                    Key::StaticShapesFillSolid,
                    state.static_shapes_fill_solid,
                    commands.static_shapes_fill_solid,
                ),
            ],
            cx,
        ),
        section(text(locale, Key::ObjectFilter), object_filter, cx),
        section(text(locale, Key::DisplayOpacityColor), appearance, cx),
        div()
            .px_4()
            .py_3()
            .child(FocusScroll::new(
                "reset-layer-order",
                scroll,
                Button::new("reset-layer-order")
                    .outline()
                    .small()
                    .w_full()
                    .icon(IconName::ListRestart)
                    .label(text(locale, Key::ResetLayerOrder))
                    .disabled(state.layer_order.is_empty())
                    .on_click(commands.reset_order),
            ))
            .into_any_element(),
    ]
}

/// Two columns of short switches; each column keeps its own trailing lane.
fn switch_grid(switches: Vec<AnyElement>) -> AnyElement {
    div()
        .grid()
        .grid_cols(2)
        .gap_x_5()
        .children(switches)
        .into_any_element()
}

/// A titled group of settings separated from its neighbors by a hairline.
pub fn section(title: String, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    div()
        .px_4()
        .pt_3()
        .pb_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().muted_foreground)
                .child(title),
        )
        .children(rows)
        .into_any_element()
}

/// One immediate setting: its label leads and the switch holds the trailing lane.
fn setting_switch(
    id: &'static str,
    label: String,
    accessible_name: String,
    checked: bool,
    handler: VisibilityHandler,
    scroll: &ScrollHandle,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_3()
        .min_h_7()
        .child(
            div()
                .id(SharedString::from(format!("{id}-label")))
                .flex_1()
                .min_w_0()
                .text_sm()
                .truncate()
                .child(label)
                // The full name recovers a short or truncated label.
                .tooltip({
                    let name = accessible_name.clone();
                    move |window, cx| Tooltip::new(name.clone()).build(window, cx)
                }),
        )
        .child(
            FocusScroll::new(
                id,
                scroll,
                Switch::new(id)
                    .small()
                    .checked(checked)
                    .accessibility_label(accessible_name)
                    .on_change(handler),
            )
            .flex_shrink_0(),
        )
        .into_any_element()
}
