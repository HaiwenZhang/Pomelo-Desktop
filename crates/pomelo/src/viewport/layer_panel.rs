//! Layer and search panel bindings for the viewport.
use super::*;
use crate::workbench::panel_layout::Side;
impl BoardViewport {
    pub(super) fn render_layer_panel(
        &mut self,
        left_panel_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Arc<Vec<pomelo_core::model::LayerId>>) {
        let locale = i18n::current(cx);
        let left_tab_labels: Vec<SharedString> =
            [Key::Layers, Key::SearchNets, Key::SearchComponents]
                .map(|key| text(locale, key).into())
                .into();
        let layer_locale_changed = self.input_locale != Some(locale);
        let search_placeholder: SharedString = text(locale, Key::SearchBoard).into();
        if self.input_locale != Some(locale) {
            self.search_input.update(cx, |input, cx| {
                input.set_placeholder(search_placeholder, window, cx)
            });
        }
        let layer_placeholder: SharedString = text(locale, Key::LayerFilter).into();
        if self.input_locale != Some(locale) {
            self.layer_input.update(cx, |input, cx| {
                input.set_placeholder(layer_placeholder, window, cx)
            });
        }
        self.input_locale = Some(locale);
        let mut seen = BTreeSet::new();
        let mut layers: Vec<_> = self
            .scene
            .layers
            .iter()
            .map(|layer| (layer.id, layer.display_name(locale)))
            .chain(
                self.scene
                    .special_layers
                    .iter()
                    .map(|layer| (layer.id, layer.display_name(locale))),
            )
            .chain(
                self.scene
                    .drawing_layers
                    .iter()
                    .map(|layer| (layer.id, layer.display_name(locale))),
            )
            .filter(|(id, _)| seen.insert(*id))
            .collect();
        let rendered_layers = self
            .tracks
            .batches
            .iter()
            .filter(|batch| !batch.outline)
            .map(|batch| batch.layer)
            .chain(self.copper.batches.iter().map(|batch| batch.layer))
            .chain(self.drawings.batches.iter().map(|batch| batch.layer))
            .chain(
                self.texts
                    .iter()
                    .flat_map(|source| source.batches.iter().map(|batch| batch.layer)),
            )
            .chain(
                self.source_strokes
                    .iter()
                    .flat_map(|source| source.batches.iter().map(|batch| batch.layer)),
            )
            .chain(self.pads.batches.iter().map(|batch| batch.layer))
            .chain(
                self.pads
                    .custom_mesh
                    .iter()
                    .flat_map(|mesh| mesh.batches.iter().map(|batch| batch.layer)),
            );
        for id in rendered_layers {
            if seen.insert(id) {
                layers.push((
                    id,
                    Message::new(Key::DefaultLayerName)
                        .arg("index", u64::from(id.0) + 1)
                        .display(locale),
                ));
            }
        }
        let layer_order = Arc::new(
            self.display
                .ordered_layers(layers.iter().map(|(id, _)| *id)),
        );
        let ranks: BTreeMap<_, _> = self
            .display
            .layers_front_to_back(layers.iter().map(|(id, _)| *id))
            .into_iter()
            .enumerate()
            .map(|(rank, id)| (id, rank))
            .collect();
        layers.sort_by_key(|(id, _)| ranks[id]);
        let list_order = Arc::clone(&layer_order);
        let all_layers: Arc<Vec<_>> = Arc::new(layers.iter().map(|(id, _)| *id).collect());
        let layer_count = layers.len();
        layers.retain(|(_, name)| name.to_lowercase().contains(&self.layer_query));
        let ids: Vec<_> = layers.iter().map(|(id, _)| *id).collect();
        if ids != self.layer_list_ids {
            self.layer_scroll.reset(ids.len());
            self.layer_list_ids = ids;
        } else if layer_locale_changed
            && let Some(index) = self
                .layer_list_ids
                .iter()
                .position(|id| Some(*id) == self.expanded_layer)
        {
            self.layer_scroll.splice(index..index + 1, 1);
        }
        if let Some(index) = self.layer_scroll_target.take()
            && index < layers.len()
        {
            self.layer_scroll.scroll_to_reveal_item(index);
        }
        let layer_rows = list(
            self.layer_scroll.clone(),
            cx.processor(move |this, index: usize, window, cx| {
                let (id, label) = &layers[index];
                let id = *id;
                let bottom_order = Arc::clone(&list_order);
                let top_order = Arc::clone(&list_order);
                let colors = if this.expanded_layer == Some(id) {
                    [
                        pomelo_core::appearance::ColorTarget::Etch(id),
                        pomelo_core::appearance::ColorTarget::Pin(id),
                        pomelo_core::appearance::ColorTarget::Via(id),
                    ]
                    .map(|target| this.color_control(target, locale, window, cx))
                    .into_iter()
                    .collect()
                } else {
                    Vec::new()
                };
                crate::panels::layers::row(
                    locale,
                    crate::panels::layers::LayerRow {
                        id,
                        label: label.clone(),
                        visible: this.display.layer_visible(id),
                        color: this
                            .display
                            .appearance
                            .material(id, pomelo_core::display::DisplayCategory::Trace)
                            .or_else(|| this.colors.get(&id).copied()),
                        colors,
                        function: this
                            .scene
                            .layers
                            .iter()
                            .find(|layer| layer.id == id)
                            .map(|layer| layer.function),
                        at_bottom: list_order.first() == Some(&id),
                        at_top: list_order.last() == Some(&id),
                        expanded: this.expanded_layer == Some(id),
                        primitives: this.display.primitives(id),
                    },
                    crate::panels::layers::LayerCommands {
                        expand: Box::new(cx.listener(move |this, _, _, cx| {
                            let old = this.expanded_layer;
                            this.expanded_layer = if old == Some(id) { None } else { Some(id) };
                            for (index, layer) in this.layer_list_ids.iter().enumerate() {
                                if Some(*layer) == old || *layer == id {
                                    this.layer_scroll.splice(index..index + 1, 1);
                                }
                            }
                            this.layer_scroll.scroll_to_reveal_item(index);
                            cx.notify();
                        })),
                        primitive: Box::new(cx.listener(
                            move |this,
                                  (kind, visible): &(
                                pomelo_core::display::LayerPrimitive,
                                bool,
                            ),
                                  _,
                                  cx| {
                                Arc::make_mut(&mut this.display).set_primitive(id, *kind, *visible);
                                this.invalidate_hover();
                                cx.notify();
                            },
                        )),
                        visibility: Box::new(cx.listener(move |this, visible: &bool, _, cx| {
                            let display = Arc::make_mut(&mut this.display);
                            let changed = if *visible {
                                display.hidden_layers.remove(&id)
                            } else {
                                display.hidden_layers.insert(id)
                            };
                            if changed {
                                this.invalidate_hover();
                                cx.notify();
                            }
                        })),
                        to_bottom: Box::new(cx.listener(move |this, _, _, cx| {
                            if Arc::make_mut(&mut this.display).move_layer_to_edge(
                                id,
                                false,
                                &bottom_order,
                            ) {
                                this.layer_scroll_target = Some(0);
                                this.invalidate_hover();
                                cx.notify();
                            }
                        })),
                        to_top: Box::new(cx.listener(move |this, _, _, cx| {
                            if Arc::make_mut(&mut this.display)
                                .move_layer_to_edge(id, true, &top_order)
                            {
                                this.layer_scroll_target = Some(top_order.len().saturating_sub(1));
                                this.invalidate_hover();
                                cx.notify();
                            }
                        })),
                    },
                    cx,
                )
            }),
        )
        .flex_1()
        .min_h_0();
        let layer_panel = div()
            .w_full()
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(crate::panels::sidebar::header(crate::panels::tabs::render(
                crate::panels::tabs::SidebarTabs {
                    id: "left-panel-tabs",
                    locale,
                    labels: left_tab_labels,
                    selected: self.left_panel,
                    width: left_panel_width - window.rem_size() * 2.5,
                },
                cx.listener(|this, index: &usize, _, cx| {
                    this.left_panel = *index;
                    cx.notify();
                }),
                window,
                cx,
            ), Side::Left, crate::panels::sidebar::toggle_button(Side::Left, false, locale,
                |_, window, cx| window.dispatch_action(Box::new(crate::actions::ToggleLeftPanel), cx))))
            .when(self.left_panel == 0, |panel| {
                let show = Arc::clone(&all_layers);
                let hide = Arc::clone(&all_layers);
                panel
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_4()
                            .pt_4()
                            .pb_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(text(locale, Key::Layers)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        Message::new(Key::UiNumber)
                                            .arg("value", layer_count)
                                            .display(locale),
                                    ),
                            ),
                    )
                    .child(
                        div().px_4().pb_2().child(
                            Input::new(&self.layer_input)
                                .prefix(Icon::new(IconName::Search).size_4())
                                .aria_label(text(locale, Key::LayerFilter)),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .px_2()
                            .pb_2()
                            .child(
                                Button::new("show-all-layers")
                                    .ghost()
                                    .small()
                                    .label(text(locale, Key::ShowAllLayers))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        for id in show.iter() {
                                            Arc::make_mut(&mut this.display)
                                                .hidden_layers
                                                .remove(id);
                                        }
                                        this.invalidate_hover();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("hide-all-layers")
                                    .ghost()
                                    .small()
                                    .label(text(locale, Key::HideAllLayers))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        Arc::make_mut(&mut this.display)
                                            .hidden_layers
                                            .extend(hide.iter().copied());
                                        this.invalidate_hover();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .id("layer-list")
                            .relative()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .child(layer_rows)
                            .vertical_scrollbar(&self.layer_scroll),
                    )
                    .child(crate::panels::layers::settings(
                        locale,
                        self.display.copper_opacity,
                        &self.copper_slider,
                        cx,
                    ))
            })
            .when(self.left_panel != 0, |panel| {
                let components = self.left_panel == 2;
                let use_index = self.search_query.trim().is_empty();
                let entries: Vec<_> = if use_index {
                    self.search.entries()
                } else {
                    &self.search_results
                }
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    matches!(
                        entry.target,
                        pomelo_core::search::SearchTarget::Component(_) | pomelo_core::search::SearchTarget::ComponentGroup(_)
                    ) == components
                })
                .map(|(index, _)| index)
                .collect();
                let entries = Arc::new(entries);
                let count = entries.len();
                panel
                    .child(
                        div().px_4().py_3().text_sm().child(
                            Message::new(Key::UiNumber)
                                .arg("value", count)
                                .display(locale),
                        ),
                    )
                    .child(
                        uniform_list(
                            "board-entities",
                            count,
                            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                range
                                    .map(|index| {
                                        let entry = if use_index {
                                            &this.search.entries()[entries[index]]
                                        } else {
                                            &this.search_results[entries[index]]
                                        };
                                        let target = entry.target;
                                        crate::panels::search::row(
                                            index,
                                            entry,
                                            this.selected_target == Some(target.into()),
                                            match target {
                                                pomelo_core::search::SearchTarget::Net(net) => {
                                                    pomelo_render::scene::colors::net_color(net)
                                                }
                                                pomelo_core::search::SearchTarget::Component(_) | pomelo_core::search::SearchTarget::ComponentGroup(_) => {
                                                    None
                                                }
                                            },
                                            locale,
                                            cx.listener(move |this, _, window, cx| {
                                                this.locate_search(target, window, cx)
                                            }),
                                        )
                                    })
                                    .collect::<Vec<_>>()
                            }),
                        )
                        .flex_1()
                        .min_h_0(),
                    )
                    .when(self.search_pending, |panel| {
                        panel.child(div().p_3().text_sm().child(text(locale, Key::Searching)))
                    })
            });

        (layer_panel.into_any_element(), layer_order)
    }
}
