//! Left panel bindings: grouped layers and the net and component lists.
use super::*;
use crate::panels::layers::{GroupCommands, GroupHeader, LayerGroup};
use gpui_kit::component::{
    input::{MoveDown, MoveUp},
    menu::{DropdownMenu, PopupMenuItem},
};
use pomelo_core::{
    display::{CellMode, LayerPrimitive},
    model::LayerId,
    search::{SearchCategory, SearchEntry},
};

/// Identity of one row in the layer list. Groups and layers share the list,
/// so retained row state must be keyed by both kinds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum LayerListKey {
    Group(LayerGroup),
    Matrix,
    Layer(LayerId),
}

/// Order of the net and component lists.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum EntitySort {
    #[default]
    Name,
    Count,
}

/// Sorted rows for one category, rebuilt only when their inputs change.
pub(super) struct EntityRows {
    category: SearchCategory,
    sort: EntitySort,
    query: String,
    filter: String,
    results: usize,
    rows: Arc<Vec<usize>>,
    total: usize,
}

#[derive(Clone)]
enum LayerItem {
    Group {
        group: LayerGroup,
        count: usize,
        visible: usize,
        members: Arc<Vec<LayerId>>,
    },
    /// Column headings of the conductor layer matrix.
    Matrix(Arc<Vec<LayerId>>),
    Layer {
        id: LayerId,
        label: String,
    },
}

/// Plain click shows or hides a cell; Alt switches a shown cell between fill and outline.
fn next_mode(current: CellMode, alt: bool) -> CellMode {
    match (current, alt) {
        (CellMode::Hidden, false) => CellMode::Fill,
        (_, false) => CellMode::Hidden,
        (CellMode::Fill, true) => CellMode::Outline,
        (_, true) => CellMode::Fill,
    }
}

/// A column reads as filled or outlined only when every layer agrees; otherwise hidden
/// wins if any layer hides it, so one click shows the whole column.
fn column_mode(
    display: &pomelo_core::display::BoardDisplay,
    members: &[LayerId],
    kind: LayerPrimitive,
) -> CellMode {
    let mode = |id: &LayerId| display.primitives(*id).mode(kind);
    let mut modes = members.iter().map(mode);
    let Some(first) = modes.next() else {
        return CellMode::Hidden;
    };
    if modes.all(|mode| mode == first) {
        first
    } else if members.iter().any(|id| mode(id) == CellMode::Hidden) {
        CellMode::Hidden
    } else {
        CellMode::Fill
    }
}

impl BoardViewport {
    fn is_conductor(&self, id: LayerId) -> bool {
        self.scene.layers.iter().any(|layer| {
            layer.id == id && layer.function != pomelo_core::model::LayerFunction::Dielectric
        })
    }
    fn layer_shown(&self, id: LayerId) -> bool {
        self.display.layer_visible(id)
            || self
                .display
                .quickview
                .is_some_and(|v| v.layer == id && v.pins)
    }
    fn quickview_control(
        &self,
        locale: pomelo_core::i18n::Locale,
        cx: &Context<Self>,
    ) -> AnyElement {
        let selected = self.display.quickview.map(|v| v.layer);
        let options: Vec<_> = self
            .scene
            .layers
            .iter()
            .map(|layer| (layer.id, layer.display_name(locale)))
            .collect();
        let name = selected
            .and_then(|id| options.iter().find(|(candidate, _)| *candidate == id))
            .map_or_else(
                || text(locale, Key::QuickviewOff),
                |(_, layer)| {
                    Message::new(Key::QuickviewPlacement)
                        .arg("layer", layer.clone())
                        .display(locale)
                },
            );
        let view = cx.entity().downgrade();
        div()
            .px_3()
            .pt_2()
            .child(
                Button::new("placement-quickview")
                    .ghost()
                    .small()
                    .w_full()
                    .justify_start()
                    .icon(IconName::Layers)
                    .label(name)
                    .accessibility_label(text(locale, Key::Quickviews))
                    .native_tooltip(text(locale, Key::QuickviewHint))
                    .dropdown_menu(move |mut menu, _, _| {
                        for (candidate, name) in
                            std::iter::once((None, text(locale, Key::QuickviewOff))).chain(
                                options.iter().map(|(id, layer)| {
                                    (
                                        Some(*id),
                                        Message::new(Key::QuickviewPlacement)
                                            .arg("layer", layer.clone())
                                            .display(locale),
                                    )
                                }),
                            )
                        {
                            let view = view.clone();
                            menu = menu.item(
                                PopupMenuItem::new(name)
                                    .checked(candidate == selected)
                                    .on_click(move |_, _, cx| {
                                        let _ = view.update(cx, |this, cx| {
                                            if this.display.quickview.map(|v| v.layer) == candidate
                                            {
                                                return;
                                            }
                                            this.last_visibility = Some(
                                                pomelo_core::display::LayerVisibility::capture(
                                                    &this.display,
                                                ),
                                            );
                                            Arc::make_mut(&mut this.display).quickview = candidate
                                                .map(|layer| {
                                                    pomelo_core::display::PlacementQuickview {
                                                        layer,
                                                        pins: true,
                                                    }
                                                });
                                            this.invalidate_hover();
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        menu
                    }),
            )
            .into_any_element()
    }
    pub(super) fn render_layer_panel(
        &mut self,
        left_panel_width: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, Arc<Vec<LayerId>>) {
        let locale = i18n::current(cx);
        let layer_locale_changed = self.input_locale != Some(locale);
        if layer_locale_changed {
            let search_placeholder: SharedString = text(locale, Key::SearchBoard).into();
            self.search_input.update(cx, |input, cx| {
                input.set_placeholder(search_placeholder, window, cx)
            });
            let layer_placeholder: SharedString = text(locale, Key::LayerFilter).into();
            self.layer_input.update(cx, |input, cx| {
                input.set_placeholder(layer_placeholder, window, cx)
            });
            let entity_placeholder: SharedString = text(locale, Key::ListFilter).into();
            self.entity_filter_input.update(cx, |input, cx| {
                input.set_placeholder(entity_placeholder, window, cx)
            });
        }
        self.input_locale = Some(locale);
        // Below this width the matrix would leave no room for layer names.
        self.layer_matrix = left_panel_width >= px(240.0);
        let layers = self.grouped_layers(locale);
        let layer_order = Arc::new(
            self.display
                .ordered_layers(layers.iter().map(|(id, _, _)| *id)),
        );
        let switcher = crate::panels::tabs::render(
            crate::panels::tabs::SidebarTabs {
                id: "left-panel-tabs",
                tabs: vec![
                    crate::panels::tabs::SidebarTab {
                        label: text(locale, Key::Layers).into(),
                        count: None,
                    },
                    crate::panels::tabs::SidebarTab {
                        label: text(locale, Key::SearchNets).into(),
                        count: None,
                    },
                    crate::panels::tabs::SidebarTab {
                        label: text(locale, Key::SearchComponents).into(),
                        count: None,
                    },
                ],
                selected: Some(self.left_panel.index()),
            },
            cx.listener(|this, index: &usize, _, cx| {
                this.left_panel = super::LeftPanelTab::from_index(*index);
                cx.notify();
            }),
            cx,
        );
        let panel = div()
            .relative()
            .w_full()
            .h_full()
            .flex_shrink_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(cx.theme().sidebar)
            .child(switcher);
        let panel = match self.left_panel.search_category() {
            None => self.render_layer_list(panel, layers, &layer_order, layer_locale_changed, cx),
            Some(category) => self.render_entity_list(panel, category, locale, cx),
        };
        (panel.into_any_element(), layer_order)
    }

    /// Every displayable layer with its family, in group order and then the
    /// current front-to-back drawing order.
    fn grouped_layers(
        &self,
        locale: pomelo_core::i18n::Locale,
    ) -> Vec<(LayerId, String, LayerGroup)> {
        let mut seen = BTreeSet::new();
        let mut layers: Vec<_> = self
            .scene
            .layers
            .iter()
            .map(|layer| (layer.id, layer.display_name(locale), LayerGroup::Copper))
            .chain(
                self.scene
                    .special_layers
                    .iter()
                    .map(|layer| (layer.id, layer.display_name(locale), LayerGroup::Package)),
            )
            .chain(
                self.scene
                    .drawing_layers
                    .iter()
                    .map(|layer| (layer.id, layer.display_name(locale), LayerGroup::Drawings)),
            )
            .filter(|(id, _, _)| seen.insert(*id))
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
                    LayerGroup::Copper,
                ));
            }
        }
        let ranks: BTreeMap<_, _> = self
            .display
            .layers_front_to_back(layers.iter().map(|(id, _, _)| *id))
            .into_iter()
            .enumerate()
            .map(|(rank, id)| (id, rank))
            .collect();
        layers.sort_by_key(|(id, _, group)| (*group, ranks[id]));
        layers
    }

    fn render_layer_list(
        &mut self,
        panel: Div,
        layers: Vec<(LayerId, String, LayerGroup)>,
        layer_order: &Arc<Vec<LayerId>>,
        locale_changed: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let locale = i18n::current(cx);
        let all_layers: Arc<Vec<_>> = Arc::new(layers.iter().map(|(id, _, _)| *id).collect());
        let visible_count = all_layers
            .iter()
            .filter(|id| self.layer_shown(**id))
            .count();
        let filtering = !self.layer_query.is_empty();
        let mut items = Vec::new();
        for group in LayerGroup::ALL {
            let members: Vec<_> = layers
                .iter()
                .filter(|(_, _, candidate)| *candidate == group)
                .collect();
            if members.is_empty() {
                continue;
            }
            let matching: Vec<_> = members
                .iter()
                .filter(|(_, name, _)| name.to_lowercase().contains(&self.layer_query))
                .collect();
            if filtering && matching.is_empty() {
                continue;
            }
            items.push(LayerItem::Group {
                group,
                count: members.len(),
                visible: members
                    .iter()
                    .filter(|(id, _, _)| self.layer_shown(*id))
                    .count(),
                members: Arc::new(members.iter().map(|(id, _, _)| *id).collect()),
            });
            if group == LayerGroup::Copper
                && self.layer_matrix
                && (filtering || !self.collapsed_layer_groups.contains(&group))
            {
                items.push(LayerItem::Matrix(Arc::new(
                    members.iter().map(|(id, _, _)| *id).collect(),
                )));
            }
            if filtering || !self.collapsed_layer_groups.contains(&group) {
                items.extend(matching.into_iter().map(|(id, label, _)| LayerItem::Layer {
                    id: *id,
                    label: label.clone(),
                }));
            }
        }
        let keys: Vec<_> = items
            .iter()
            .map(|item| match item {
                LayerItem::Group { group, .. } => LayerListKey::Group(*group),
                LayerItem::Matrix(_) => LayerListKey::Matrix,
                LayerItem::Layer { id, .. } => LayerListKey::Layer(*id),
            })
            .collect();
        if keys != self.layer_list_ids {
            self.layer_scroll.reset(keys.len());
            self.layer_list_ids = keys;
        } else if locale_changed
            && let Some(index) = self.layer_list_ids.iter().position(
                |key| matches!(key, LayerListKey::Layer(id) if Some(*id) == self.expanded_layer),
            )
        {
            self.layer_scroll.splice(index..index + 1, 1);
        }
        // Resolve against the displayed (grouped, filtered) rows.
        if let Some(index) = self
            .layer_scroll_target
            .take()
            .and_then(|target| match target {
                super::LayerScrollTarget::Start => (!items.is_empty()).then_some(0),
                super::LayerScrollTarget::Layer(id) => self
                    .layer_list_ids
                    .iter()
                    .position(|key| *key == LayerListKey::Layer(id)),
            })
        {
            self.layer_scroll.scroll_to_reveal_item(index);
        }
        let list_order = Arc::clone(layer_order);
        let solo_layers = Arc::clone(&all_layers);
        let layer_rows = list(
            self.layer_scroll.clone(),
            cx.processor(
                move |this, index: usize, window, cx| match items[index].clone() {
                    LayerItem::Group {
                        group,
                        count,
                        visible,
                        members,
                    } => crate::panels::layers::group_header(
                        locale,
                        GroupHeader {
                            group,
                            count,
                            visible,
                            // A filter reveals matches inside collapsed groups.
                            collapsed: this.layer_query.is_empty()
                                && this.collapsed_layer_groups.contains(&group),
                        },
                        GroupCommands {
                            toggle: Box::new(cx.listener(move |this, _, _, cx| {
                                if !this.collapsed_layer_groups.remove(&group) {
                                    this.collapsed_layer_groups.insert(group);
                                }
                                cx.notify();
                            })),
                            visibility: Box::new(cx.listener(move |this, show: &bool, _, cx| {
                                let display = Arc::make_mut(&mut this.display);
                                if let Some(view) = display
                                    .quickview
                                    .as_mut()
                                    .filter(|v| members.contains(&v.layer))
                                {
                                    view.pins = *show;
                                }
                                for id in members.iter() {
                                    if *show {
                                        display.hidden_layers.remove(id);
                                    } else {
                                        display.hidden_layers.insert(*id);
                                    }
                                }
                                this.invalidate_hover();
                                cx.notify();
                            })),
                        },
                        cx,
                    ),
                    LayerItem::Matrix(members) => {
                        let columns = LayerPrimitive::MATRIX
                            .map(|kind| column_mode(&this.display, &members, kind));
                        crate::panels::layers::matrix_header(
                            locale,
                            columns,
                            Box::new(cx.listener(
                                move |this, (kind, alt): &(LayerPrimitive, bool), _, cx| {
                                    let display = Arc::make_mut(&mut this.display);
                                    let current = column_mode(display, &members, *kind);
                                    let mode = next_mode(current, *alt);
                                    for id in members.iter() {
                                        display.set_cell_mode(*id, *kind, mode);
                                    }
                                    this.invalidate_hover();
                                    cx.notify();
                                },
                            )),
                            cx,
                        )
                    }
                    LayerItem::Layer { id, label } => this.render_layer_row(
                        id,
                        label,
                        &list_order,
                        &solo_layers,
                        locale,
                        window,
                        cx,
                    ),
                },
            ),
        )
        .flex_1()
        .min_h_0();
        let show = Arc::clone(&all_layers);
        let hide = Arc::clone(&all_layers);
        panel
            .child(self.quickview_control(locale, cx))
            .child(
                div().px_3().pt_2().child(
                    Input::new(&self.layer_input)
                        .small()
                        .prefix(Icon::new(IconName::Search).small())
                        .aria_label(text(locale, Key::LayerFilter)),
                ),
            )
            .child(self.render_visibility_views(locale, cx))
            .child(
                // The count and the commands wrap as whole items so a longer
                // translation moves to its own line instead of being cut.
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_x_2()
                    .pl_4()
                    .pr_2()
                    .pt_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div().whitespace_nowrap().child(
                            Message::new(Key::LayersVisible)
                                .arg("visible", visible_count)
                                .arg("total", all_layers.len())
                                .display(locale),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .ml_auto()
                            .child(
                                Button::new("restore-layer-visibility")
                                    .ghost()
                                    .xsmall()
                                    .label(text(locale, Key::RestoreLayerVisibility))
                                    .disabled(self.last_visibility.is_none())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(last) = &mut this.last_visibility {
                                            last.swap(Arc::make_mut(&mut this.display));
                                            this.invalidate_hover();
                                            cx.notify();
                                        }
                                    })),
                            )
                            .child(
                                Button::new("show-all-layers")
                                    .ghost()
                                    .xsmall()
                                    .label(text(locale, Key::ShowAllLayers))
                                    .disabled(
                                        visible_count == all_layers.len()
                                            && self.display.layer_primitives.is_empty()
                                            && self.display.quickview.is_none_or(|v| v.pins),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.last_visibility =
                                            Some(pomelo_core::display::LayerVisibility::capture(
                                                &this.display,
                                            ));
                                        let display = Arc::make_mut(&mut this.display);
                                        display.layer_primitives.clear();
                                        if let Some(view) = &mut display.quickview {
                                            view.pins = true;
                                        }
                                        for id in show.iter() {
                                            display.hidden_layers.remove(id);
                                        }
                                        this.invalidate_hover();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("hide-all-layers")
                                    .ghost()
                                    .xsmall()
                                    .label(text(locale, Key::HideAllLayers))
                                    .disabled(
                                        visible_count == 0 && self.display.quickview.is_none(),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.last_visibility =
                                            Some(pomelo_core::display::LayerVisibility::capture(
                                                &this.display,
                                            ));
                                        let display = Arc::make_mut(&mut this.display);
                                        display.quickview = None;
                                        display.hidden_layers.extend(hide.iter().copied());
                                        this.invalidate_hover();
                                        cx.notify();
                                    })),
                            ),
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
                    .pb_2()
                    .child(layer_rows)
                    .vertical_scrollbar(&self.layer_scroll),
            )
            .when(self.view3d.active, |panel| {
                panel.child(
                    div()
                        .px_3()
                        .py_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(self.render_stackup_notice(locale, cx)),
                )
            })
            .child(panel_footer(
                Message::new(Key::SoloLayerHint)
                    .arg(
                        "modifier",
                        if cfg!(target_os = "macos") {
                            "⌥"
                        } else {
                            "Alt"
                        },
                    )
                    .display(locale),
                cx,
            ))
    }

    #[allow(clippy::too_many_arguments)]
    fn render_layer_row(
        &mut self,
        id: LayerId,
        label: String,
        list_order: &Arc<Vec<LayerId>>,
        all_layers: &Arc<Vec<LayerId>>,
        locale: pomelo_core::i18n::Locale,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = self.expanded_layer == Some(id);
        let colors = if expanded {
            [
                pomelo_core::appearance::ColorTarget::Etch(id),
                pomelo_core::appearance::ColorTarget::Pin(id),
                pomelo_core::appearance::ColorTarget::Via(id),
            ]
            .map(|target| self.color_swatch(target, locale, window, cx))
            .into_iter()
            .collect()
        } else {
            Vec::new()
        };
        let bottom_order = Arc::clone(list_order);
        let top_order = Arc::clone(list_order);
        let all_layers = Arc::clone(all_layers);
        crate::panels::layers::row(
            locale,
            crate::panels::layers::LayerRow {
                id,
                label,
                visible: self.layer_shown(id),
                color: self
                    .display
                    .appearance
                    .material(id, pomelo_core::display::DisplayCategory::Trace)
                    .or_else(|| self.colors.get(&id).copied()),
                colors,
                at_bottom: list_order.first() == Some(&id),
                at_top: list_order.last() == Some(&id),
                expanded,
                active: self.display.active_layer == Some(id),
                cells: (self.layer_matrix && self.is_conductor(id)).then(|| {
                    LayerPrimitive::MATRIX.map(|kind| self.display.cell_mode(id, kind))
                }),
                opacity: self
                    .is_conductor(id)
                    .then(|| self.display.layer_alpha(id)),
            },
            crate::panels::layers::LayerCommands {
                activate: Box::new(cx.listener(move |this, _, _, cx| {
                    Arc::make_mut(&mut this.display).active_layer = Some(id);
                    this.invalidate_hover();
                    cx.notify();
                })),
                expand: Box::new(cx.listener(move |this, _, _, cx| {
                    let old = this.expanded_layer;
                    this.expanded_layer = if old == Some(id) { None } else { Some(id) };
                    for (index, key) in this.layer_list_ids.iter().enumerate() {
                        if matches!(key, LayerListKey::Layer(layer) if Some(*layer) == old || *layer == id)
                        {
                            this.layer_scroll.splice(index..index + 1, 1);
                        }
                    }
                    if let Some(index) = this
                        .layer_list_ids
                        .iter()
                        .position(|key| *key == LayerListKey::Layer(id))
                    {
                        this.layer_scroll.scroll_to_reveal_item(index);
                    }
                    cx.notify();
                })),
                cell: Box::new(cx.listener(
                    move |this, (kind, alt): &(LayerPrimitive, bool), _, cx| {
                        let display = Arc::make_mut(&mut this.display);
                        // A cell of a hidden layer brings the layer back with that cell shown.
                        let current = if display.hidden_layers.remove(&id) {
                            CellMode::Hidden
                        } else {
                            display.primitives(id).mode(*kind)
                        };
                        display.set_cell_mode(id, *kind, next_mode(current, *alt));
                        this.invalidate_hover();
                        cx.notify();
                    },
                )),
                opacity: Box::new(cx.listener(move |this, alpha: &f32, _, cx| {
                    Arc::make_mut(&mut this.display).set_layer_alpha(id, *alpha);
                    cx.notify();
                })),
                solo: {
                    let solo_layers = Arc::clone(&all_layers);
                    Box::new(cx.listener(move |this, _, _, cx| {
                        this.last_visibility =
                            Some(pomelo_core::display::LayerVisibility::capture(&this.display));
                        Arc::make_mut(&mut this.display).hidden_layers = solo_layers
                            .iter()
                            .copied()
                            .filter(|layer| *layer != id)
                            .collect();
                        this.invalidate_hover();
                        cx.notify();
                    }))
                },
                visibility: {
                    let view = cx.entity().downgrade();
                    Box::new(move |event, visible, _, cx| {
                        // Alt-click isolates this layer; a plain click toggles it.
                        let solo = event.modifiers().alt;
                        let _ = view.update(cx, |this, cx| {
                            let display = Arc::make_mut(&mut this.display);
                            let quick_changed=if let Some(view)=display.quickview.as_mut().filter(|v|v.layer==id) {let changed=view.pins!=visible;view.pins=visible;changed}else{false};
                            let changed = if solo {
                                let previous = display.hidden_layers.clone();
                                display.hidden_layers =
                                    all_layers.iter().copied().filter(|layer| *layer != id).collect();
                                display.hidden_layers != previous
                            } else if visible {
                                display.hidden_layers.remove(&id)
                            } else {
                                display.hidden_layers.insert(id)
                            };
                            if changed || quick_changed {
                                this.invalidate_hover();
                                cx.notify();
                            }
                        });
                    })
                },
                to_bottom: Box::new(cx.listener(move |this, _, _, cx| {
                    if Arc::make_mut(&mut this.display).move_layer_to_edge(
                        id,
                        false,
                        &bottom_order,
                    ) {
                        this.layer_scroll_target = Some(super::LayerScrollTarget::Layer(id));
                        this.invalidate_hover();
                        cx.notify();
                    }
                })),
                to_top: Box::new(cx.listener(move |this, _, _, cx| {
                    if Arc::make_mut(&mut this.display)
                        .move_layer_to_edge(id, true, &top_order)
                    {
                        this.layer_scroll_target = Some(super::LayerScrollTarget::Layer(id));
                        this.invalidate_hover();
                        cx.notify();
                    }
                })),
            },
            cx,
        )
    }

    fn render_entity_list(
        &mut self,
        panel: Div,
        category: SearchCategory,
        locale: pomelo_core::i18n::Locale,
        cx: &mut Context<Self>,
    ) -> Div {
        let query = self.search_query.trim().to_owned();
        let use_index = query.is_empty();
        let rows = self.entity_rows(category);
        let total = self.entity_rows.as_ref().map_or(0, |cache| cache.total);
        let count = rows.len();
        let highlighted = self.entity_highlight.min(count.saturating_sub(1));
        let sort = self.entity_sort;
        let highlight: Arc<str> = if self.entity_filter.is_empty() || self.nets.regex {
            query.to_lowercase().into()
        } else {
            self.entity_filter.clone().into()
        };
        let net_tools = (category == SearchCategory::Net).then(|| {
            let entries: &[SearchEntry] = if use_index {
                self.search.entries()
            } else {
                &self.search_results
            };
            let listed = rows
                .iter()
                .filter_map(|index| match entries[*index].target {
                    pomelo_core::search::SearchTarget::Net(net) => Some(net),
                    _ => None,
                })
                .collect();
            self.render_net_tools(locale, listed, cx)
        });
        panel
            .child(
                div()
                    .px_3()
                    .pt_2()
                    // The filter keeps focus while arrow keys move the list
                    // highlight, so capture them before the text field does.
                    .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
                        this.move_entity_highlight(-1, cx);
                        cx.stop_propagation();
                    }))
                    .capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
                        this.move_entity_highlight(1, cx);
                        cx.stop_propagation();
                    }))
                    .child(
                        Input::new(&self.entity_filter_input)
                            .small()
                            .prefix(Icon::new(IconName::Search).small())
                            .cleanable(true)
                            .aria_label(text(locale, Key::ListFilter)),
                    ),
            )
            .children(net_tools)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .pl_4()
                    .pr_2()
                    .pt_1()
                    .pb_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div().truncate().child(
                            Message::new(Key::ResultsCount)
                                .arg("shown", count)
                                .arg("total", total)
                                .display(locale),
                        ),
                    )
                    .child(
                        Button::new("entity-sort")
                            .ghost()
                            .xsmall()
                            .flex_shrink_0()
                            .icon(match sort {
                                EntitySort::Name => IconName::ArrowDownAZ,
                                EntitySort::Count => IconName::ArrowDownWideNarrow,
                            })
                            .label(text(
                                locale,
                                match sort {
                                    EntitySort::Name => Key::SortByName,
                                    EntitySort::Count => Key::SortByCount,
                                },
                            ))
                            .accessibility_label(text(locale, Key::SortResults))
                            .dropdown_menu({
                                let view = cx.entity().downgrade();
                                move |mut menu, _, _| {
                                    for (candidate, key) in [
                                        (EntitySort::Name, Key::SortByName),
                                        (EntitySort::Count, Key::SortByCount),
                                    ] {
                                        let view = view.clone();
                                        menu = menu.item(
                                            PopupMenuItem::new(text(locale, key))
                                                .checked(candidate == sort)
                                                .on_click(move |_, _, cx| {
                                                    let _ = view.update(cx, |this, cx| {
                                                        this.entity_sort = candidate;
                                                        this.entity_highlight = 0;
                                                        cx.notify();
                                                    });
                                                }),
                                        );
                                    }
                                    menu
                                }
                            }),
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
                                    &this.search.entries()[rows[index]]
                                } else {
                                    &this.search_results[rows[index]]
                                };
                                let target = entry.target;
                                let net_name = entry.name.clone();
                                let row = crate::panels::search::row(
                                    crate::panels::search::EntityRow {
                                        index,
                                        entry,
                                        highlight: &highlight,
                                        selected: this.selected_target == Some(target.into()),
                                        keyboard: index == highlighted,
                                        color: match target {
                                            pomelo_core::search::SearchTarget::Net(net) => (!this
                                                .display
                                                .appearance
                                                .hide_net_colors)
                                                .then(|| this.display.appearance.nets.get(&net))
                                                .flatten()
                                                .and_then(|colors| {
                                                    pomelo_core::appearance::net::NetColorKind::ALL
                                                        .into_iter()
                                                        .find_map(|kind| colors.color(kind))
                                                })
                                                .map(pomelo_core::appearance::RgbColor::rgba)
                                                .or_else(|| {
                                                    pomelo_render::scene::colors::net_color(net)
                                                }),
                                            pomelo_core::search::SearchTarget::Component(_)
                                            | pomelo_core::search::SearchTarget::ComponentGroup(
                                                _,
                                            ) => None,
                                        },
                                    },
                                    locale,
                                    cx.listener(move |this, _, window, cx| {
                                        this.entity_highlight = index;
                                        this.locate_search(target, window, cx)
                                    }),
                                    cx,
                                );
                                if let pomelo_core::search::SearchTarget::Net(net) = target {
                                    let label = Message::new(Key::NetColorsFor)
                                        .arg("name", net_name)
                                        .display(locale);
                                    div()
                                        .flex()
                                        .items_center()
                                        .w_full()
                                        .child(div().flex_1().min_w_0().child(row))
                                        .child({
                                            let visible = this.display.net_visible(net);
                                            let label = text(
                                                locale,
                                                if visible { Key::HideNet } else { Key::ShowNet },
                                            );
                                            Button::new(("net-visible", u64::from(net.0)))
                                                .ghost()
                                                .small()
                                                .icon(if visible {
                                                    IconName::Eye
                                                } else {
                                                    IconName::EyeOff
                                                })
                                                .accessibility_label(label.clone())
                                                .native_tooltip(label)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.toggle_net_visibility(net, cx)
                                                }))
                                        })
                                        .child(
                                            Button::new(("net-colors", u64::from(net.0)))
                                                .ghost()
                                                .small()
                                                .icon(IconName::Palette)
                                                .accessibility_label(label.clone())
                                                .native_tooltip(label)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.open_net_colors(net, cx)
                                                })),
                                        )
                                        .into_any_element()
                                } else {
                                    row
                                }
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.entity_scroll)
                .flex_1()
                .min_h_0(),
            )
            .when(count == 0 && !self.search_pending, |panel| {
                panel.child(
                    div()
                        .absolute()
                        .top_32()
                        .left_0()
                        .right_0()
                        .text_center()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(text(locale, Key::NoSearchResults)),
                )
            })
            .child(panel_footer(
                if self.search_pending {
                    text(locale, Key::Searching)
                } else {
                    text(locale, Key::ListKeysHint)
                },
                cx,
            ))
    }

    /// Rows for one category after the board search, the panel filter and
    /// the chosen order. Rebuilt only when one of those inputs changes.
    fn entity_rows(&mut self, category: SearchCategory) -> Arc<Vec<usize>> {
        let query = self.search_query.trim().to_owned();
        let stale = self.entity_rows.as_ref().is_none_or(|cache| {
            cache.category != category
                || cache.sort != self.entity_sort
                || cache.query != query
                || cache.filter != self.entity_filter
                || cache.results != self.search_results.len()
        });
        if stale {
            let entries: &[SearchEntry] = if query.is_empty() {
                self.search.entries()
            } else {
                &self.search_results
            };
            // Regex mode falls back to plain text while the expression is incomplete.
            let pattern = (self.nets.regex && !self.entity_filter.is_empty())
                .then(|| {
                    regex::RegexBuilder::new(&self.entity_filter)
                        .case_insensitive(true)
                        .size_limit(1 << 20)
                        .build()
                        .ok()
                })
                .flatten();
            let mut rows: Vec<_> = entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.target.category() == category
                        && match &pattern {
                            Some(pattern) => pattern.is_match(&entry.name),
                            None => {
                                self.entity_filter.is_empty()
                                    || entry.name.to_lowercase().contains(&self.entity_filter)
                            }
                        }
                        && match entry.target {
                            pomelo_core::search::SearchTarget::Net(net) => self.net_in_filter(net),
                            _ => true,
                        }
                })
                .map(|(index, _)| index)
                .collect();
            // Search results arrive in relevance order; both sorts are stable.
            match self.entity_sort {
                EntitySort::Name => {
                    rows.sort_by_cached_key(|index| entries[*index].name.to_lowercase())
                }
                EntitySort::Count => {
                    rows.sort_by_key(|index| std::cmp::Reverse(entries[*index].count))
                }
            }
            let total = self
                .search
                .entries()
                .iter()
                .filter(|entry| entry.target.category() == category)
                .count();
            self.entity_rows = Some(EntityRows {
                category,
                sort: self.entity_sort,
                query,
                filter: self.entity_filter.clone(),
                results: self.search_results.len(),
                rows: Arc::new(rows),
                total,
            });
        }
        self.entity_rows
            .as_ref()
            .map_or_else(Default::default, |cache| Arc::clone(&cache.rows))
    }

    fn move_entity_highlight(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(category) = self.left_panel.search_category() else {
            return;
        };
        let count = self.entity_rows(category).len();
        if count == 0 {
            return;
        }
        self.entity_highlight = self
            .entity_highlight
            .min(count - 1)
            .saturating_add_signed(delta)
            .min(count - 1);
        self.entity_scroll
            .scroll_to_item(self.entity_highlight, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Select and frame the keyboard-highlighted net or component.
    pub(super) fn locate_highlighted_entity(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(category) = self.left_panel.search_category() else {
            return;
        };
        let rows = self.entity_rows(category);
        let Some(&row) = rows.get(self.entity_highlight.min(rows.len().saturating_sub(1))) else {
            return;
        };
        let entries = if self.search_query.trim().is_empty() {
            self.search.entries()
        } else {
            &self.search_results
        };
        if let Some(entry) = entries.get(row) {
            let target = entry.target;
            self.locate_search(target, window, cx);
        }
    }
}

/// A quiet one-line note pinned to the bottom of a side panel.
pub(super) fn panel_footer(message: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_2()
        .flex_shrink_0()
        .px_4()
        .py_2()
        .border_t_1()
        .border_color(cx.theme().border)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(Icon::new(IconName::Info).xsmall().flex_shrink_0())
        .child(div().min_w_0().child(message.into()))
}
