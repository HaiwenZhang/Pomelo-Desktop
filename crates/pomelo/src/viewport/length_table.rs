//! Collapsible table under the canvas comparing routed lengths of related nets:
//! the selected net with its pair partner or bus, plus nets the user pinned.
use super::*;
use pomelo_core::{
    i18n::Locale,
    model::{LayerId, NetId},
    review::net_report::{self, NetReport},
};

/// Largest comparison the table computes at once.
const MAX_ROWS: usize = 128;

#[derive(Default)]
pub(super) struct LengthTable {
    pub open: bool,
    pub pinned: BTreeSet<NetId>,
    pub nets: Vec<NetId>,
    pub rows: Vec<Arc<NetReport>>,
    pub pending: bool,
    pub cancel: pomelo_core::task::CancellationToken,
    pub task: Option<Task<()>>,
}

impl BoardViewport {
    /// Nets compared with the selection: its differential partner or bus, and pinned nets.
    fn comparison_nets(&mut self) -> Vec<NetId> {
        let mut nets: Vec<NetId> = Vec::new();
        let selected = match self.selected_target {
            Some(pomelo_core::selection::SelectionTarget::Net(net)) => Some(net),
            _ => self.selected_related_net,
        }
        .filter(|net| net.0 != 0);
        if let Some(net) = selected {
            let groups = self.net_groups();
            if let Some(pair) = groups
                .pairs
                .iter()
                .find(|pair| pair.positive == net || pair.negative == net)
            {
                nets.extend([pair.positive, pair.negative]);
            } else if let Some(bus) = groups.buses.iter().find(|bus| bus.members.contains(&net)) {
                nets.extend(bus.members.iter().copied());
            } else {
                nets.push(net);
            }
        }
        for net in &self.length_table.pinned {
            if !nets.contains(net) {
                nets.push(*net);
            }
        }
        nets.truncate(MAX_ROWS);
        nets
    }

    pub(super) fn toggle_length_table(&mut self, cx: &mut Context<Self>) {
        self.length_table.open = !self.length_table.open;
        cx.notify();
    }

    pub(super) fn pin_to_length_table(&mut self, net: NetId, cx: &mut Context<Self>) {
        if !self.length_table.pinned.remove(&net) {
            self.length_table.pinned.insert(net);
        }
        self.length_table.open = true;
        cx.notify();
    }

    /// Recompute rows when the compared nets change.
    pub(super) fn ensure_length_table(&mut self, cx: &mut Context<Self>) {
        if !self.length_table.open {
            return;
        }
        let nets = self.comparison_nets();
        if nets == self.length_table.nets {
            return;
        }
        self.length_table.cancel.cancel();
        self.length_table.cancel = Default::default();
        self.length_table.nets = nets.clone();
        self.length_table.rows.clear();
        self.length_table.pending = !nets.is_empty();
        let cancel = self.length_table.cancel.clone();
        let scene = Arc::clone(&self.scene);
        let worker = cx.background_spawn(async move {
            nets.iter()
                .map(|net| net_report::report(&scene, *net, &cancel).map(Arc::new))
                .collect::<Result<Vec<_>, _>>()
                .ok()
        });
        let cancel = self.length_table.cancel.clone();
        self.length_table.task = Some(cx.spawn(async move |this, cx| {
            let rows = worker.await;
            let _ = this.update(cx, |this, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                this.length_table.pending = false;
                this.length_table.rows = rows.unwrap_or_default();
                cx.notify();
            });
        }));
    }

    fn table_layers(&self) -> Vec<LayerId> {
        let used: BTreeSet<_> = self
            .length_table
            .rows
            .iter()
            .flat_map(|row| row.layer_lengths.iter().map(|(layer, _)| *layer))
            .collect();
        self.scene
            .layers
            .iter()
            .map(|layer| layer.id)
            .filter(|id| used.contains(id))
            .collect()
    }

    fn net_label(&self, net: NetId) -> String {
        self.scene
            .nets
            .get(&net)
            .filter(|name| !name.is_empty())
            .cloned()
            .unwrap_or_else(|| net.0.to_string())
    }

    /// Rows as CSV in millimetres, with a header naming every layer column.
    fn length_table_csv(&self, locale: Locale) -> String {
        let layers = self.table_layers();
        let escape = |value: String| {
            if value.contains([',', '"', '\n']) {
                format!("\"{}\"", value.replace('"', "\"\""))
            } else {
                value
            }
        };
        let mut header = vec![
            text(locale, Key::LengthTableNet),
            text(locale, Key::LengthTableTotal),
        ];
        header.extend(layers.iter().map(|layer| {
            self.scene
                .layers
                .iter()
                .find(|l| l.id == *layer)
                .map_or_else(String::new, |l| l.display_name(locale))
        }));
        header.push(text(locale, Key::NetReportVias));
        let mut out = header.into_iter().map(escape).collect::<Vec<_>>().join(",");
        for row in &self.length_table.rows {
            out.push('\n');
            let mut cells = vec![
                escape(self.net_label(row.net)),
                format!("{:.4}", row.total_length),
            ];
            for layer in &layers {
                cells.push(
                    row.layer_lengths
                        .iter()
                        .find(|(l, _)| l == layer)
                        .map_or_else(String::new, |(_, mm)| format!("{mm:.4}")),
                );
            }
            cells.push(row.vias.len().to_string());
            out.push_str(&cells.join(","));
        }
        out
    }

    pub(super) fn render_length_table(
        &self,
        locale: Locale,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.length_table.open {
            return None;
        }
        let theme = cx.theme();
        let unit = self.display.length_unit;
        let layers = self.table_layers();
        let reference = match self.selected_target {
            Some(pomelo_core::selection::SelectionTarget::Net(net)) => Some(net),
            _ => self.selected_related_net,
        };
        let reference_length = self
            .length_table
            .rows
            .iter()
            .find(|row| Some(row.net) == reference)
            .or(self.length_table.rows.first())
            .map(|row| row.total_length);
        let number = |mm: f64| {
            let value = unit.from_millimeters(mm);
            match unit {
                pomelo_core::units::LengthUnit::Millimeters => format!("{value:.2}"),
                pomelo_core::units::LengthUnit::Mils => format!("{value:.1}"),
            }
        };
        let cell = |content: String, strong: bool| {
            div()
                .flex_1()
                .min_w_16()
                .truncate()
                .text_right()
                .when(strong, |c| c.font_weight(FontWeight::SEMIBOLD))
                .child(content)
        };
        let header = div()
            .flex()
            .gap_2()
            .px_3()
            .h_6()
            .items_center()
            .text_xs()
            .text_color(theme.muted_foreground)
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .w_40()
                    .truncate()
                    .child(text(locale, Key::LengthTableNet)),
            )
            .child(cell(text(locale, Key::LengthTableTotal), false))
            .children(layers.iter().map(|layer| {
                cell(
                    self.scene
                        .layers
                        .iter()
                        .find(|l| l.id == *layer)
                        .map_or_else(String::new, |l| l.display_name(locale)),
                    false,
                )
            }))
            .child(cell(text(locale, Key::NetReportVias), false))
            .child(cell(text(locale, Key::LengthTableDelta), false));
        let mut rows: Vec<_> = self.length_table.rows.iter().collect();
        rows.sort_by(|a, b| b.total_length.total_cmp(&a.total_length));
        let body = rows.into_iter().map(|row| {
            let selected = Some(row.net) == reference;
            let delta = reference_length.map(|base| row.total_length - base);
            div()
                .flex()
                .gap_2()
                .px_3()
                .h_6()
                .items_center()
                .text_xs()
                .when(selected, |r| r.bg(theme.list_active))
                .child(
                    div()
                        .w_40()
                        .truncate()
                        .when(selected, |c| c.font_weight(FontWeight::SEMIBOLD))
                        .child(self.net_label(row.net)),
                )
                .child(cell(number(row.total_length), true))
                .children(layers.iter().map(|layer| {
                    cell(
                        row.layer_lengths
                            .iter()
                            .find(|(l, _)| l == layer)
                            .map_or_else(
                                || text(locale, Key::LengthTableNone),
                                |(_, mm)| number(*mm),
                            ),
                        false,
                    )
                }))
                .child(cell(row.vias.len().to_string(), false))
                .child(cell(
                    match delta {
                        Some(_) if selected => text(locale, Key::LengthTableReference),
                        Some(d) => {
                            let s = number(d);
                            if d >= 0.0 { format!("+{s}") } else { s }
                        }
                        None => String::new(),
                    },
                    false,
                ))
        });
        let unit_label = text(
            locale,
            match unit {
                pomelo_core::units::LengthUnit::Millimeters => Key::UnitMillimeters,
                pomelo_core::units::LengthUnit::Mils => Key::UnitMils,
            },
        );
        let csv = self.length_table_csv(locale);
        Some(
            div()
                .id("length-table")
                .h_48()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .bg(theme.sidebar)
                .border_t_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .px_3()
                        .h_8()
                        .border_b_1()
                        .border_color(theme.border)
                        .text_sm()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(text(locale, Key::LengthTableTitle)),
                        )
                        .child(
                            div().text_xs().text_color(theme.muted_foreground).child(
                                Message::new(Key::LengthTableSummary)
                                    .arg("count", self.length_table.rows.len())
                                    .arg("unit", unit_label)
                                    .display(locale),
                            ),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("length-table-copy")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Copy)
                                .label(text(locale, Key::LengthTableCopy))
                                .disabled(self.length_table.rows.is_empty())
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(csv.clone()))
                                }),
                        )
                        .child(
                            Button::new("length-table-close")
                                .ghost()
                                .xsmall()
                                .icon(IconName::X)
                                .accessibility_label(text(locale, Key::LengthTableClose))
                                .native_tooltip(text(locale, Key::LengthTableClose))
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.toggle_length_table(cx)),
                                ),
                        ),
                )
                .child(header)
                .child(
                    div()
                        .id("length-table-rows")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .children(body)
                        .when(self.length_table.rows.is_empty(), |list| {
                            list.child(
                                div()
                                    .p_3()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(text(
                                        locale,
                                        if self.length_table.pending {
                                            Key::NetReportPending
                                        } else {
                                            Key::LengthTableEmpty
                                        },
                                    )),
                            )
                        }),
                )
                .into_any_element(),
        )
    }
}
