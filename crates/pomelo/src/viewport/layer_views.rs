//! Visibility presets and saved visibility views above the layer list, and the
//! active-layer commands behind 2D single-layer mode.
use super::*;
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use pomelo_core::{
    i18n::Locale,
    model::{LayerFunction, LayerId},
    review::visibility::{MAX_VISIBILITY_VIEWS, VisibilityPreset, VisibilityView},
};

const PRESETS: [(VisibilityPreset, Key); 4] = [
    (VisibilityPreset::All, Key::PresetAll),
    (VisibilityPreset::Surface, Key::PresetSurface),
    (VisibilityPreset::Signal, Key::PresetSignal),
    (VisibilityPreset::PowerGround, Key::PresetPowerGround),
];

impl BoardViewport {
    /// Conductor layers in stack order.
    fn conductor_layers(&self) -> Vec<LayerId> {
        self.scene
            .layers
            .iter()
            .filter(|layer| layer.id.is_copper() && layer.function != LayerFunction::Dielectric)
            .map(|layer| layer.id)
            .collect()
    }

    pub(super) fn apply_visibility_preset(
        &mut self,
        preset: VisibilityPreset,
        cx: &mut Context<Self>,
    ) {
        let previous = pomelo_core::display::LayerVisibility::capture(&self.display);
        if preset.apply(&self.scene.layers, Arc::make_mut(&mut self.display)) {
            self.last_visibility = Some(previous);
            self.invalidate_hover();
            cx.notify();
        }
    }

    fn save_visibility_view(&mut self, locale: Locale, cx: &mut Context<Self>) {
        let state = &mut self.review.state;
        if state.visibility_views.len() >= MAX_VISIBILITY_VIEWS {
            self.review.notice =
                Some(Message::new(Key::VisibilityViewLimit).arg("limit", MAX_VISIBILITY_VIEWS));
            cx.notify();
            return;
        }
        let id = state.allocate_id();
        let name = Message::new(Key::VisibilityViewDefaultName)
            .arg("index", state.visibility_views.len() + 1)
            .display(locale);
        state
            .visibility_views
            .push(VisibilityView::capture(id, &name, &self.display));
        cx.notify();
    }

    fn apply_visibility_view(&mut self, id: u32, cx: &mut Context<Self>) {
        let Some(view) = self
            .review
            .state
            .visibility_views
            .iter()
            .find(|view| view.id == id)
            .cloned()
        else {
            return;
        };
        let previous = pomelo_core::display::LayerVisibility::capture(&self.display);
        if view.apply(Arc::make_mut(&mut self.display)) {
            self.last_visibility = Some(previous);
            self.invalidate_hover();
            cx.notify();
        }
    }

    fn delete_visibility_view(&mut self, id: u32, cx: &mut Context<Self>) {
        self.review
            .state
            .visibility_views
            .retain(|view| view.id != id);
        cx.notify();
    }

    /// Move the active layer through the conductor stack and keep it visible.
    pub(super) fn step_active_layer(&mut self, down: bool, cx: &mut Context<Self>) {
        let layers = self.conductor_layers();
        if layers.is_empty() {
            return;
        }
        let index = self
            .display
            .active_layer
            .and_then(|id| layers.iter().position(|layer| *layer == id));
        let next = match index {
            Some(i) if down => (i + 1).min(layers.len() - 1),
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        let display = Arc::make_mut(&mut self.display);
        display.active_layer = Some(layers[next]);
        display.hidden_layers.remove(&layers[next]);
        self.invalidate_hover();
        cx.notify();
    }

    /// Dim every layer except the active one, choosing the top layer if none is active.
    pub(super) fn toggle_single_layer(&mut self, cx: &mut Context<Self>) {
        let first = self.conductor_layers().first().copied();
        let display = Arc::make_mut(&mut self.display);
        display.shadow.enabled = !display.shadow.enabled;
        display.shadow.dim_active = false;
        if display.shadow.enabled && display.active_layer.is_none() {
            display.active_layer = first;
        }
        if let Some(active) = display.active_layer.filter(|_| display.shadow.enabled) {
            display.hidden_layers.remove(&active);
        }
        self.invalidate_hover();
        cx.notify();
    }

    pub(super) fn render_visibility_views(&self, locale: Locale, cx: &Context<Self>) -> Div {
        let theme = cx.theme();
        let chips = PRESETS.into_iter().map(|(preset, key)| {
            let selected = preset.matches(&self.scene.layers, &self.display);
            Button::new(("visibility-preset", preset as u32))
                .xsmall()
                .when(selected, |button| button.primary())
                .when(!selected, |button| button.outline())
                .rounded_full()
                .label(text(locale, key))
                .on_click(
                    cx.listener(move |this, _, _, cx| this.apply_visibility_preset(preset, cx)),
                )
        });
        let views = self.review.state.visibility_views.clone();
        let view = cx.entity().downgrade();
        let saved = Button::new("visibility-views")
            .ghost()
            .xsmall()
            .label(text(locale, Key::VisibilityViews))
            .dropdown_caret(true)
            .disabled(views.is_empty())
            .dropdown_menu(move |mut menu, window, _| {
                menu = menu
                    .min_w(window.rem_size() * 12.0)
                    .label(text(locale, Key::VisibilityViews));
                for saved in &views {
                    let id = saved.id;
                    let apply = view.clone();
                    menu = menu.item(PopupMenuItem::new(saved.name.clone()).on_click(
                        move |_, _, cx| {
                            let _ = apply.update(cx, |this, cx| this.apply_visibility_view(id, cx));
                        },
                    ));
                }
                menu = menu
                    .separator()
                    .label(text(locale, Key::DeleteVisibilityView));
                for saved in &views {
                    let id = saved.id;
                    let delete = view.clone();
                    menu = menu.item(PopupMenuItem::new(saved.name.clone()).on_click(
                        move |_, _, cx| {
                            let _ =
                                delete.update(cx, |this, cx| this.delete_visibility_view(id, cx));
                        },
                    ));
                }
                menu
            });
        let save_label = text(locale, Key::SaveVisibilityView);
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .pt_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().child(text(locale, Key::VisibilityViews)))
                    .child(saved)
                    .child(
                        Button::new("save-visibility-view")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .accessibility_label(save_label.clone())
                            .native_tooltip(save_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.save_visibility_view(locale, cx)
                            })),
                    ),
            )
            .child(div().flex().flex_wrap().gap_1().children(chips))
    }
}
