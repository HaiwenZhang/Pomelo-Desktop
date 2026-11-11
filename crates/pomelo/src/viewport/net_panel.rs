//! Net facts for the inspector (per-layer length, via use and stubs, nearest
//! planes, pins), inferred net groups for the net list, and net visibility.
use super::*;
use pomelo_core::{
    i18n::Locale,
    model::{LayerId, NetId},
    review::{
        net_groups::{self, NetGroups},
        net_report::{self, NetReport},
    },
};

/// A net list narrowed to one inferred group family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NetGroupFilter {
    Pairs,
    Buses,
    Power,
}

#[derive(Default)]
pub(super) struct NetState {
    pub groups: Option<Arc<NetGroups>>,
    pub filter: Option<NetGroupFilter>,
    /// Members of the active group filter, rebuilt with it.
    pub members: Option<Arc<BTreeSet<NetId>>>,
    pub report: Option<(NetId, Option<Arc<NetReport>>)>,
    pub report_cancel: pomelo_core::task::CancellationToken,
    pub report_task: Option<Task<()>>,
    /// Treat the list filter as a regular expression.
    pub regex: bool,
}

impl BoardViewport {
    fn net_groups(&mut self) -> Arc<NetGroups> {
        if let Some(groups) = &self.nets.groups {
            return Arc::clone(groups);
        }
        let planes: BTreeSet<LayerId> = self
            .scene
            .layers
            .iter()
            .filter(|layer| layer.function == pomelo_core::model::LayerFunction::Plane)
            .map(|layer| layer.id)
            .collect();
        let plane_nets = self
            .scene
            .zones
            .iter()
            .filter(|zone| planes.contains(&zone.layer))
            .map(|zone| zone.net)
            .collect();
        let groups = Arc::new(net_groups::infer(&self.scene.nets, &plane_nets));
        self.nets.groups = Some(Arc::clone(&groups));
        groups
    }

    pub(super) fn set_net_group_filter(
        &mut self,
        filter: Option<NetGroupFilter>,
        cx: &mut Context<Self>,
    ) {
        let groups = self.net_groups();
        self.nets.filter = filter;
        self.nets.members = filter.map(|filter| {
            Arc::new(match filter {
                NetGroupFilter::Pairs => groups
                    .pairs
                    .iter()
                    .flat_map(|pair| [pair.positive, pair.negative])
                    .collect(),
                NetGroupFilter::Buses => groups
                    .buses
                    .iter()
                    .flat_map(|bus| bus.members.iter().copied())
                    .collect(),
                NetGroupFilter::Power => groups.power.iter().copied().collect(),
            })
        });
        self.entity_rows = None;
        cx.notify();
    }

    pub(super) fn net_in_filter(&self, net: NetId) -> bool {
        self.nets
            .members
            .as_ref()
            .is_none_or(|members| members.contains(&net))
    }

    pub(super) fn toggle_net_visibility(&mut self, net: NetId, cx: &mut Context<Self>) {
        let display = Arc::make_mut(&mut self.display);
        if !display.hidden_nets.remove(&net) {
            display.hidden_nets.insert(net);
        }
        self.invalidate_hover();
        cx.notify();
    }

    /// Show, hide or isolate the nets currently listed.
    pub(super) fn set_listed_nets(
        &mut self,
        listed: Vec<NetId>,
        mode: ListedNets,
        cx: &mut Context<Self>,
    ) {
        let all: Vec<NetId> = self
            .scene
            .nets
            .keys()
            .copied()
            .filter(|n| n.0 != 0)
            .collect();
        let display = Arc::make_mut(&mut self.display);
        match mode {
            ListedNets::Show => {
                for net in listed {
                    display.hidden_nets.remove(&net);
                }
            }
            ListedNets::Hide => display.hidden_nets.extend(listed),
            ListedNets::Only => {
                let keep: BTreeSet<_> = listed.into_iter().collect();
                display.hidden_nets = all.into_iter().filter(|n| !keep.contains(n)).collect();
            }
        }
        self.invalidate_hover();
        cx.notify();
    }

    /// Net whose facts the inspector shows: the selected net, or the net of the selected object.
    fn report_net(&self) -> Option<NetId> {
        match self.selected_target {
            Some(pomelo_core::selection::SelectionTarget::Net(net)) => Some(net),
            _ => self.selected_related_net,
        }
        .filter(|net| net.0 != 0)
    }

    /// Compute the inspector's net facts off the UI thread when the selection changes.
    pub(super) fn ensure_net_report(&mut self, cx: &mut Context<Self>) {
        let Some(net) = self.report_net() else {
            self.nets.report = None;
            return;
        };
        if self.nets.report.as_ref().is_some_and(|(id, _)| *id == net) {
            return;
        }
        self.nets.report_cancel.cancel();
        self.nets.report_cancel = Default::default();
        let cancel = self.nets.report_cancel.clone();
        let scene = Arc::clone(&self.scene);
        self.nets.report = Some((net, None));
        let worker =
            cx.background_spawn(async move { net_report::report(&scene, net, &cancel).ok() });
        self.nets.report_task = Some(cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if let (Some((id, slot)), Some(report)) = (&mut this.nets.report, result)
                    && *id == net
                {
                    *slot = Some(Arc::new(report));
                    cx.notify();
                }
            });
        }));
    }

    fn layer_name(&self, id: LayerId, locale: Locale) -> String {
        self.scene
            .layers
            .iter()
            .find(|layer| layer.id == id)
            .map_or_else(|| id.0.to_string(), |layer| layer.display_name(locale))
    }

    pub(super) fn render_net_report(&self, locale: Locale, cx: &App) -> Option<AnyElement> {
        let (_, report) = self.nets.report.as_ref()?;
        let theme = cx.theme();
        let heading = |key: Key| {
            div()
                .pt_2()
                .pb_1()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(text(locale, key))
        };
        let Some(report) = report else {
            return Some(
                div()
                    .px_4()
                    .py_2()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(text(locale, Key::NetReportPending))
                    .into_any_element(),
            );
        };
        let unit = self.display.length_unit;
        let length = |mm: f64| super::review::format_length(mm, unit, locale);
        let longest = report
            .layer_lengths
            .iter()
            .map(|(_, l)| *l)
            .fold(0.0_f64, f64::max)
            .max(f64::EPSILON);
        let color = |layer: LayerId| {
            self.colors
                .get(&layer)
                .map(|[r, g, b, a]| {
                    Hsla::from(Rgba {
                        r: *r,
                        g: *g,
                        b: *b,
                        a: *a,
                    })
                })
                .unwrap_or(theme.muted_foreground)
        };
        let lengths = report.layer_lengths.iter().map(|(layer, mm)| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_h_6()
                .text_sm()
                .child(div().w_1().h_4().rounded_sm().bg(color(*layer)))
                .child(
                    div()
                        .w_24()
                        .truncate()
                        .child(self.layer_name(*layer, locale)),
                )
                .child(
                    div()
                        .flex_1()
                        .h_1p5()
                        .rounded_full()
                        .bg(theme.secondary)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(color(*layer))
                                .w(relative((*mm / longest) as f32)),
                        ),
                )
                .child(div().w_20().text_right().child(length(*mm)))
        });
        let vias = report.vias.iter().take(32).map(|via| {
            let used = via
                .used
                .iter()
                .map(|layer| self.layer_name(*layer, locale))
                .collect::<Vec<_>>()
                .join(" → ");
            let stub = via.stubs.last().map(|(from, to)| {
                Message::new(Key::NetReportStub)
                    .arg("from", self.layer_name(*from, locale))
                    .arg("to", self.layer_name(*to, locale))
                    .display(locale)
            });
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_h_7()
                .px_2()
                .rounded_md()
                .bg(theme.secondary)
                .text_sm()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(if used.is_empty() {
                            text(locale, Key::NetReportViaUnconnected)
                        } else {
                            used
                        }),
                )
                .child(match stub {
                    Some(stub) => div()
                        .flex_shrink_0()
                        .text_xs()
                        .px_1p5()
                        .rounded_full()
                        .border_1()
                        .border_color(theme.danger)
                        .text_color(theme.danger)
                        .child(stub),
                    None => div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(text(locale, Key::NetReportNoStub)),
                })
        });
        let planes = report.adjacent_planes.iter().map(|(layer, above, below)| {
            let name = |layer: &Option<LayerId>| {
                layer.map_or_else(
                    || text(locale, Key::NetReportNoPlane),
                    |id| self.layer_name(id, locale),
                )
            };
            div()
                .flex()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .w_24()
                        .truncate()
                        .child(self.layer_name(*layer, locale)),
                )
                .child(
                    div().flex_1().text_color(theme.muted_foreground).child(
                        Message::new(Key::NetReportPlanes)
                            .arg("above", name(above))
                            .arg("below", name(below))
                            .display(locale),
                    ),
                )
        });
        let pins = report
            .pins
            .iter()
            .take(24)
            .map(|(reference, pin)| {
                Message::new(Key::PinLabel)
                    .arg("reference", reference.clone())
                    .arg("pin", pin.clone())
                    .display(locale)
            })
            .collect::<Vec<_>>()
            .join(" · ");
        Some(
            div()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .child(heading(Key::NetReportLengths))
                .children(lengths)
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .pt_1()
                        .text_sm()
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(text(locale, Key::NetReportTotal)),
                        )
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(length(report.total_length)),
                        ),
                )
                .child(heading(Key::NetReportVias))
                .child(
                    div()
                        .pb_1()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(
                            Message::new(Key::NetReportViaSummary)
                                .arg("count", report.vias.len())
                                .arg("stubbed", report.stubbed_vias())
                                .display(locale),
                        ),
                )
                .child(div().flex().flex_col().gap_1().children(vias))
                .when(!report.adjacent_planes.is_empty(), |card| {
                    card.child(heading(Key::NetReportAdjacent)).children(planes)
                })
                .when(!report.pins.is_empty(), |card| {
                    card.child(heading(Key::NetReportPins))
                        .child(div().text_sm().child(pins))
                })
                .child(
                    div()
                        .pt_2()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(text(locale, Key::NetReportScope)),
                )
                .into_any_element(),
        )
    }

    /// Group chips, bulk visibility and regex mode above the net list.
    pub(super) fn render_net_tools(
        &mut self,
        locale: Locale,
        listed: Vec<NetId>,
        cx: &mut Context<Self>,
    ) -> Div {
        let groups = self.net_groups();
        let theme = cx.theme();
        let chip = |id: &'static str,
                    label: String,
                    filter: Option<NetGroupFilter>,
                    current: Option<NetGroupFilter>| {
            let selected = current == filter;
            Button::new(id)
                .xsmall()
                .rounded_full()
                .when(selected, |b| b.primary())
                .when(!selected, |b| b.outline())
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| this.set_net_group_filter(filter, cx)))
        };
        let current = self.nets.filter;
        let count = |key: Key, n: usize| Message::new(key).arg("count", n).display(locale);
        let show = listed.clone();
        let hide = listed.clone();
        let only = listed;
        let regex = self.nets.regex;
        div()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .pt_2()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .child(chip(
                        "net-group-all",
                        text(locale, Key::PresetAll),
                        None,
                        current,
                    ))
                    .child(chip(
                        "net-group-pairs",
                        count(Key::NetGroupPairs, groups.pairs.len()),
                        Some(NetGroupFilter::Pairs),
                        current,
                    ))
                    .child(chip(
                        "net-group-buses",
                        count(Key::NetGroupBuses, groups.buses.len()),
                        Some(NetGroupFilter::Buses),
                        current,
                    ))
                    .child(chip(
                        "net-group-power",
                        count(Key::NetGroupPower, groups.power.len()),
                        Some(NetGroupFilter::Power),
                        current,
                    )),
            )
            .when(current.is_some(), |tools| {
                tools.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(text(locale, Key::NetGroupInferred)),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_1()
                    .child(
                        Button::new("net-regex")
                            .ghost()
                            .xsmall()
                            .label(text(locale, Key::NetRegexShort))
                            .selected(regex)
                            .accessibility_label(text(locale, Key::NetRegex))
                            .native_tooltip(text(locale, Key::NetRegex))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.nets.regex = !this.nets.regex;
                                this.entity_rows = None;
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("nets-show")
                            .ghost()
                            .xsmall()
                            .label(text(locale, Key::NetsShowListed))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_listed_nets(show.clone(), ListedNets::Show, cx)
                            })),
                    )
                    .child(
                        Button::new("nets-hide")
                            .ghost()
                            .xsmall()
                            .label(text(locale, Key::NetsHideListed))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_listed_nets(hide.clone(), ListedNets::Hide, cx)
                            })),
                    )
                    .child(
                        Button::new("nets-only")
                            .ghost()
                            .xsmall()
                            .label(text(locale, Key::NetsOnlyListed))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_listed_nets(only.clone(), ListedNets::Only, cx)
                            })),
                    ),
            )
    }
}

#[derive(Clone, Copy)]
pub(super) enum ListedNets {
    Show,
    Hide,
    Only,
}
