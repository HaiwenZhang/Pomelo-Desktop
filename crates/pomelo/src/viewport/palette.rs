//! The command palette under the title search (⌘K). One list answers three
//! questions: which net, which component, and which command. Board results
//! still fill the left panel lists; the palette is the fast path to the top
//! matches and to commands that otherwise live behind toolbar menus.
use super::*;
use crate::services::preferences::CanvasOverlays;
use pomelo_core::{i18n::Locale, review::measure::MeasureKind};
use review::ReviewTool;

/// Net and component rows shown per group; the left panel lists the rest.
const NET_ROWS: usize = 4;
const COMPONENT_ROWS: usize = 3;
/// Commands shown for an empty query, and at most for a typed one.
const COMMAND_ROWS: usize = 8;

/// Commands reachable from the palette, each also reachable elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaletteCommand {
    MeasureDistance,
    MeasurePolyline,
    Clearance,
    PinPath,
    BoxSelect,
    ZoomBox,
    Fit,
    Flip,
    SingleLayer,
    SwitchView,
    Isolate,
    LengthTable,
    CopyImage,
    Display,
    AllDisplaySettings,
    ShowOverlays,
    ToolbarOnly,
    NavigateBack,
}

const COMMANDS: [PaletteCommand; 18] = [
    PaletteCommand::MeasureDistance,
    PaletteCommand::MeasurePolyline,
    PaletteCommand::Clearance,
    PaletteCommand::PinPath,
    PaletteCommand::BoxSelect,
    PaletteCommand::ZoomBox,
    PaletteCommand::Fit,
    PaletteCommand::Flip,
    PaletteCommand::SingleLayer,
    PaletteCommand::SwitchView,
    PaletteCommand::Isolate,
    PaletteCommand::LengthTable,
    PaletteCommand::CopyImage,
    PaletteCommand::Display,
    PaletteCommand::AllDisplaySettings,
    PaletteCommand::ShowOverlays,
    PaletteCommand::ToolbarOnly,
    PaletteCommand::NavigateBack,
];

impl PaletteCommand {
    /// Whether the command applies to the current view.
    fn available(self, in_3d: bool) -> bool {
        match self {
            Self::Clearance
            | Self::PinPath
            | Self::BoxSelect
            | Self::ZoomBox
            | Self::SingleLayer => !in_3d,
            _ => true,
        }
    }

    fn label(self, locale: Locale, in_3d: bool) -> String {
        match self {
            Self::MeasureDistance => text(locale, Key::MeasureDistanceTool),
            Self::MeasurePolyline => text(locale, Key::MeasurePolylineTool),
            Self::Clearance => text(locale, Key::ClearanceTool),
            Self::PinPath => text(locale, Key::PinPathTool),
            Self::BoxSelect => text(locale, Key::BoxSelectTool),
            Self::ZoomBox => text(locale, Key::ZoomBoxTool),
            Self::Fit => text(locale, Key::FitBoard),
            Self::Flip => text(locale, Key::FlipBoard),
            Self::SingleLayer => text(locale, Key::WelcomeShortcutSingleLayer),
            Self::SwitchView => Message::new(Key::PaletteSwitchView)
                .arg(
                    "view",
                    text(locale, if in_3d { Key::View2d } else { Key::View3d }),
                )
                .display(locale),
            Self::Isolate => text(locale, Key::IsolateSelection),
            Self::LengthTable => text(locale, Key::LengthTableTool),
            Self::CopyImage => text(locale, Key::CopyImageTool),
            Self::Display => text(locale, Key::DisplayQuick),
            Self::AllDisplaySettings => text(locale, Key::AllDisplaySettings),
            Self::ShowOverlays => text(locale, Key::PaletteShowOverlays),
            Self::ToolbarOnly => text(locale, Key::OverlayToolbarOnly),
            Self::NavigateBack => text(locale, Key::NavigateBack),
        }
    }

    /// The shortcut shown in the trailing lane, when the command has one
    /// that its label does not already carry.
    fn shortcut(self) -> Option<String> {
        let key = match self {
            Self::Fit => crate::shortcuts::keys::FIT_ACTIVE_BOARD,
            Self::Flip => "b",
            Self::SingleLayer => crate::shortcuts::keys::SINGLE_LAYER,
            Self::NavigateBack => crate::shortcuts::keys::NAVIGATE_BACK,
            _ => return None,
        };
        Some(crate::shortcuts::shortcut_label(key))
    }
}

/// What a palette row does when chosen.
#[derive(Clone, Debug)]
pub(super) enum PaletteAction {
    GoTo(pomelo_core::model::Point),
    Locate(pomelo_core::search::SearchTarget),
    Command(PaletteCommand),
}

#[derive(Clone, Debug)]
pub(super) struct PaletteItem {
    /// Group heading shown above the first row of each group.
    heading: Option<Key>,
    label: String,
    meta: Option<String>,
    /// Board names and coordinates use the monospace face.
    mono: bool,
    action: PaletteAction,
}

/// A command matches when every word of the query appears in its label, in
/// the interface language or in English, ignoring case.
fn command_matches(query: &str, labels: &[String]) -> bool {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|word| word.to_lowercase())
        .collect();
    if words.is_empty() {
        return true;
    }
    labels.iter().any(|label| {
        let label = label.to_lowercase();
        words.iter().all(|word| label.contains(word.as_str()))
    })
}

/// Palette keyboard state, owned by the viewport.
#[derive(Default)]
pub(super) struct PaletteState {
    pub active: usize,
    /// Escape closed the palette; typing or refocusing reopens it.
    pub dismissed: bool,
}

impl BoardViewport {
    pub(super) fn palette_items(&self, locale: Locale) -> Vec<PaletteItem> {
        let query = self.search_query.trim();
        let in_3d = self.view3d.active;
        let mut items = Vec::new();
        if let Some(point) = pomelo_core::units::parse_coordinates(query, self.display.length_unit)
        {
            let unit = self.display.length_unit;
            items.push(PaletteItem {
                heading: None,
                label: Message::new(Key::PaletteGoTo)
                    .arg("x", review::format_length(point.x, unit, locale))
                    .arg("y", review::format_length(point.y, unit, locale))
                    .display(locale),
                meta: None,
                mono: true,
                action: PaletteAction::GoTo(point),
            });
        }
        if !query.is_empty() {
            use pomelo_core::search::SearchCategory;
            for (category, heading, limit, count_key) in [
                (
                    SearchCategory::Net,
                    Key::SearchNets,
                    NET_ROWS,
                    Key::PaletteObjects,
                ),
                (
                    SearchCategory::Component,
                    Key::SearchComponents,
                    COMPONENT_ROWS,
                    Key::PalettePins,
                ),
            ] {
                let rows = self
                    .search_results
                    .iter()
                    .filter(|entry| entry.target.category() == category)
                    .take(limit);
                for (index, entry) in rows.enumerate() {
                    items.push(PaletteItem {
                        heading: (index == 0).then_some(heading),
                        label: entry.name.clone(),
                        meta: Some(
                            Message::new(count_key)
                                .arg("count", entry.count)
                                .display(locale),
                        ),
                        mono: true,
                        action: PaletteAction::Locate(entry.target),
                    });
                }
            }
        }
        let commands = COMMANDS
            .into_iter()
            .filter(|command| command.available(in_3d))
            .filter(|command| {
                command_matches(
                    query,
                    &[
                        command.label(locale, in_3d),
                        command.label(Locale::English, in_3d),
                    ],
                )
            })
            .take(COMMAND_ROWS);
        for (index, command) in commands.enumerate() {
            items.push(PaletteItem {
                heading: (index == 0).then_some(Key::PaletteCommands),
                label: command.label(locale, in_3d),
                meta: command.shortcut(),
                mono: false,
                action: PaletteAction::Command(command),
            });
        }
        items
    }

    /// The palette shows while the search field has focus, until Escape.
    pub(super) fn palette_open(&self, window: &Window, cx: &App) -> bool {
        !self.palette.dismissed
            && self
                .search_input
                .read(cx)
                .focus_handle(cx)
                .is_focused(window)
    }

    pub(super) fn step_palette(&mut self, delta: isize, cx: &mut Context<Self>) {
        let count = self.palette_items(crate::i18n::current(cx)).len();
        if count == 0 {
            return;
        }
        let next = (self.palette.active as isize + delta).rem_euclid(count as isize);
        self.palette.active = next as usize;
        cx.notify();
    }

    /// Run the highlighted row. Returns false when there is nothing to run.
    pub(super) fn run_active_palette_item(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let items = self.palette_items(crate::i18n::current(cx));
        let Some(item) = items.get(self.palette.active.min(items.len().saturating_sub(1))) else {
            return false;
        };
        self.run_palette_action(item.action.clone(), window, cx);
        true
    }

    fn run_palette_action(
        &mut self,
        action: PaletteAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Leave the field so the palette closes and the canvas takes the keys.
        self.palette.dismissed = true;
        window.focus(&self.focus, cx);
        match action {
            PaletteAction::GoTo(point) => self.go_to_point(point, cx),
            PaletteAction::Locate(target) => self.locate_search(target, window, cx),
            PaletteAction::Command(command) => self.run_palette_command(command, window, cx),
        }
        cx.notify();
    }

    fn run_palette_command(
        &mut self,
        command: PaletteCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tool = |kind| Some(ReviewTool::Measure(kind));
        match command {
            PaletteCommand::MeasureDistance => {
                self.set_review_tool(tool(MeasureKind::Distance), window, cx)
            }
            PaletteCommand::MeasurePolyline => {
                self.set_review_tool(tool(MeasureKind::Polyline), window, cx)
            }
            PaletteCommand::Clearance => {
                self.set_review_tool(Some(ReviewTool::Clearance), window, cx)
            }
            PaletteCommand::PinPath => self.set_review_tool(Some(ReviewTool::PinPath), window, cx),
            PaletteCommand::BoxSelect => {
                self.set_review_tool(Some(ReviewTool::BoxSelect), window, cx)
            }
            PaletteCommand::ZoomBox => self.set_review_tool(Some(ReviewTool::ZoomBox), window, cx),
            PaletteCommand::Fit => self.fit_view(window, cx),
            PaletteCommand::Flip => self.flip_board(&FlipBoard, window, cx),
            PaletteCommand::SingleLayer => self.toggle_single_layer(cx),
            PaletteCommand::SwitchView => self.toggle_3d(window, cx),
            PaletteCommand::Isolate => self.toggle_isolation(cx),
            PaletteCommand::LengthTable => self.toggle_length_table(cx),
            PaletteCommand::CopyImage => self.copy_view_image(window, cx),
            PaletteCommand::Display => self.display_quick_open = true,
            PaletteCommand::AllDisplaySettings => self.open_display_settings(cx),
            PaletteCommand::ShowOverlays => self.set_overlays(CanvasOverlays::all(true), cx),
            PaletteCommand::ToolbarOnly => self.set_overlays(CanvasOverlays::all(false), cx),
            PaletteCommand::NavigateBack => self.navigate_back(&NavigateBack, window, cx),
        }
    }

    /// The list below the title search. It floats above the window content
    /// and keeps the search field's width.
    pub(super) fn render_palette(
        &self,
        locale: Locale,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.palette_open(window, cx) {
            return None;
        }
        let items = self.palette_items(locale);
        let active = self.palette.active.min(items.len().saturating_sub(1));
        let theme = cx.theme();
        let mono = theme.mono_font_family.clone();
        let mut list = div().flex().flex_col().p_1p5();
        if items.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py_2()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(text(
                        locale,
                        if self.search_pending {
                            Key::Searching
                        } else {
                            Key::NoSearchResults
                        },
                    )),
            );
        }
        for (index, item) in items.into_iter().enumerate() {
            if let Some(heading) = item.heading {
                list = list.child(
                    div()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(text(locale, heading)),
                );
            }
            let selected = index == active;
            let action = item.action.clone();
            list = list.child(
                div()
                    .id(("palette-row", index))
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_h_8()
                    .px_2()
                    .rounded_md()
                    .when(selected, |row| row.bg(theme.list_active))
                    .when(!selected, |row| row.hover(|row| row.bg(theme.list_hover)))
                    .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_palette_action(action.clone(), window, cx)
                    }))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .when(item.mono, |label| label.font_family(mono.clone()))
                            .child(item.label),
                    )
                    .when_some(item.meta, |row, meta| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(meta),
                        )
                    }),
            );
        }
        // A zero-height slot below the field gives the floating list its
        // origin; the list itself paints above the rest of the window.
        Some(
            div()
                .w_full()
                .h_0()
                .relative()
                .child(
                    deferred(
                        anchored().snap_to_window_with_margin(px(8.0)).child(
                            div()
                                .id("command-palette")
                                .occlude()
                                .mt_1()
                                .w(rems(28.0))
                                .max_h(rems(26.0))
                                .overflow_y_scroll()
                                .rounded_lg()
                                .border_1()
                                .border_color(theme.border)
                                .bg(theme.popover)
                                .text_color(theme.popover_foreground)
                                .shadow_lg()
                                .child(list)
                                .child(
                                    div()
                                        .px_3()
                                        .py_1p5()
                                        .border_t_1()
                                        .border_color(theme.border)
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(text(locale, Key::PaletteHint)),
                                ),
                        ),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{COMMANDS, PaletteCommand, command_matches};
    use pomelo_core::i18n::Locale;

    #[test]
    fn every_word_of_the_query_must_match_one_label() {
        let labels = [
            "测量两点距离 (M)".to_owned(),
            "Measure distance (M)".to_owned(),
        ];
        assert!(command_matches("", &labels));
        assert!(command_matches("测量", &labels));
        assert!(command_matches("measure DIST", &labels));
        assert!(!command_matches("measure area", &labels));
    }

    #[test]
    fn two_dimensional_tools_are_not_offered_in_3d() {
        let in_3d: Vec<_> = COMMANDS.into_iter().filter(|c| c.available(true)).collect();
        assert!(!in_3d.contains(&PaletteCommand::BoxSelect));
        assert!(in_3d.contains(&PaletteCommand::MeasureDistance));
        assert!(COMMANDS.into_iter().all(|c| c.available(false)));
    }

    #[test]
    fn every_command_has_a_label_in_every_language() {
        for locale in Locale::ALL {
            for command in COMMANDS {
                assert!(!command.label(locale, false).is_empty(), "{command:?}");
            }
        }
    }
}
