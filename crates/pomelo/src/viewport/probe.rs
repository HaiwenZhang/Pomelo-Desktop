//! Stack probe: what each conductor layer holds under the pointer, so the 2D
//! view answers "what is above and below this point" without switching to 3D.
use super::*;
use pomelo_core::{
    display::{BoardDisplay, DisplayCategory},
    i18n::Locale,
    model::{LayerId, NetId},
    selection::SelectedObject,
};

/// What one conductor layer holds at the probed point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProbeHit {
    Trace(NetId),
    Copper(NetId),
    Pad(NetId),
    Via(NetId),
    Empty,
}

#[derive(Default)]
pub(super) struct ProbeState {
    pub at: Option<pomelo_core::model::Point>,
    pub layers: Vec<(LayerId, ProbeHit)>,
    pub cancel: pomelo_core::task::CancellationToken,
    pub task: Option<Task<()>>,
    /// Object to net, built once on first use.
    pub nets: Option<Arc<BTreeMap<SelectedObject, NetId>>>,
}

fn object_nets(scene: &pomelo_core::model::BoardScene) -> BTreeMap<SelectedObject, NetId> {
    let mut nets = BTreeMap::new();
    nets.extend(
        scene
            .segments
            .iter()
            .map(|s| (SelectedObject::Segment(s.id), s.net)),
    );
    nets.extend(
        scene
            .zones
            .iter()
            .map(|z| (SelectedObject::Zone(z.id), z.net)),
    );
    nets.extend(
        scene
            .pins
            .iter()
            .map(|p| (SelectedObject::Pin(p.id), p.net)),
    );
    nets.extend(
        scene
            .vias
            .iter()
            .map(|v| (SelectedObject::Via(v.id), v.net)),
    );
    nets
}

impl BoardViewport {
    /// Probe every conductor layer at a canvas position, ignoring current visibility.
    pub(super) fn probe_at(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.view3d.active {
            return;
        }
        let Some(bounds) = self.bounds else {
            return;
        };
        let local = position - bounds.origin;
        let camera = self.navigation.camera();
        let size = self.navigation.size();
        let point = pomelo_core::model::Point::new(f64::from(local.x), f64::from(local.y));
        let board = camera.view_to_board(point, size.x, size.y);
        let Some(query) = pomelo_core::picking::PickQuery::from_viewport(camera, point, size, 0.0)
        else {
            return;
        };
        self.probe.cancel.cancel();
        self.probe.cancel = Default::default();
        let cancel = self.probe.cancel.clone();
        let index = Arc::clone(&self.picking);
        let scene = Arc::clone(&self.scene);
        let nets = self.probe.nets.clone();
        let layers: Vec<LayerId> = self
            .scene
            .layers
            .iter()
            .filter(|layer| {
                layer.id.is_copper()
                    && layer.function != pomelo_core::model::LayerFunction::Dielectric
            })
            .map(|layer| layer.id)
            .collect();
        let delay = cx
            .background_executor()
            .timer(std::time::Duration::from_millis(60));
        let worker = cx.background_spawn(async move {
            delay.await;
            cancel.check().ok()?;
            let nets = nets.unwrap_or_else(|| Arc::new(object_nets(&scene)));
            // Everything counts, whatever the user currently shows.
            let display = BoardDisplay {
                show_copper: true,
                ..BoardDisplay::default()
            };
            let hits = index
                .query_visible_hits(
                    query,
                    camera.pixels_per_mm,
                    pomelo_core::picking::PickFilter::all(),
                    &display,
                    256,
                    &cancel,
                )
                .ok()?;
            let result = layers
                .iter()
                .map(|layer| {
                    let hit = hits
                        .iter()
                        .filter(|hit| hit.anchor.layer == *layer && hit.distance_mm <= 0.0)
                        .find_map(|hit| {
                            let net = nets.get(&hit.anchor.object).copied()?;
                            Some(match hit.anchor.category {
                                DisplayCategory::Trace | DisplayCategory::BondWire => {
                                    ProbeHit::Trace(net)
                                }
                                DisplayCategory::Zone | DisplayCategory::ZoneOutline => {
                                    ProbeHit::Copper(net)
                                }
                                DisplayCategory::Pin => ProbeHit::Pad(net),
                                DisplayCategory::Via => ProbeHit::Via(net),
                                _ => return None,
                            })
                        })
                        .unwrap_or(ProbeHit::Empty);
                    (*layer, hit)
                })
                .collect::<Vec<_>>();
            Some((result, nets))
        });
        let cancel = self.probe.cancel.clone();
        self.probe.task = Some(cx.spawn_in(window, async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                if cancel.is_cancelled() {
                    return;
                }
                if let Some((layers, nets)) = result {
                    this.probe.nets = Some(nets);
                    this.probe.at = Some(board);
                    this.probe.layers = layers;
                    cx.notify();
                }
            });
        }));
    }

    pub(super) fn render_probe(&self, locale: Locale, cx: &App) -> Option<AnyElement> {
        if self.view3d.active || self.probe.layers.is_empty() {
            return None;
        }
        let at = self.probe.at?;
        let _ = cx;
        // The card floats on the dark canvas in both UI themes.
        let (_, ink, hairline) = crate::theme::canvas_colors();
        let muted = crate::theme::canvas_muted();
        let net_name = |net: NetId| {
            self.scene
                .nets
                .get(&net)
                .filter(|name| !name.is_empty())
                .cloned()
                .unwrap_or_else(|| net.0.to_string())
        };
        let rows = self.probe.layers.iter().map(|(layer, hit)| {
            let color = self
                .colors
                .get(layer)
                .map(|[r, g, b, a]| {
                    Hsla::from(Rgba {
                        r: *r,
                        g: *g,
                        b: *b,
                        a: *a,
                    })
                })
                .unwrap_or(muted);
            let (key, net) = match *hit {
                ProbeHit::Trace(net) => (Key::ProbeTrace, Some(net)),
                ProbeHit::Copper(net) => (Key::ProbeCopper, Some(net)),
                ProbeHit::Pad(net) => (Key::ProbePad, Some(net)),
                ProbeHit::Via(net) => (Key::ProbeVia, Some(net)),
                ProbeHit::Empty => (Key::ProbeEmpty, None),
            };
            let filled = net.is_some();
            let selected = net.is_some_and(|net| {
                self.selected_target == Some(pomelo_core::selection::SelectionTarget::Net(net))
                    || self.selected_related_net == Some(net)
            });
            div()
                .flex()
                .items_center()
                .gap_2()
                .min_h_5()
                .text_xs()
                .text_color(if selected { ink } else { muted })
                .child(
                    div()
                        .w_12()
                        .h_2()
                        .rounded_sm()
                        .border_1()
                        .border_color(if filled { color } else { hairline })
                        .when(filled, |bar| bar.bg(color)),
                )
                .child(
                    div().w_20().truncate().child(
                        self.scene
                            .layers
                            .iter()
                            .find(|l| l.id == *layer)
                            .map_or_else(String::new, |l| l.display_name(locale)),
                    ),
                )
                .child(div().flex_1().min_w_0().truncate().child(match net {
                    Some(net) => Message::new(key).arg("net", net_name(net)).display(locale),
                    None => text(locale, key),
                }))
        });
        let unit = self.display.length_unit;
        Some(
            div()
                .pl_3()
                .pr_2()
                .py_2()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .justify_between()
                        .gap_x_2()
                        .pb_1()
                        // Leave the close button's lane free.
                        .pr_6()
                        .text_xs()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(muted)
                                .child(text(locale, Key::ProbeHeading)),
                        )
                        .child(
                            div().text_color(muted).child(
                                Message::new(Key::ProbePosition)
                                    .arg("x", super::review::format_length(at.x, unit, locale))
                                    .arg("y", super::review::format_length(at.y, unit, locale))
                                    .display(locale),
                            ),
                        ),
                )
                .children(rows)
                .into_any_element(),
        )
    }
}
