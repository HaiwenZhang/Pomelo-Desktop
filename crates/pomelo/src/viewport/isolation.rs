//! Isolate the selection: show only its nets on the conductor layers they use,
//! then return to the previous visibility with the same command.
use super::*;
use pomelo_core::{
    model::{LayerId, NetId},
    selection::{SelectedObject, SelectionTarget},
};

/// Visibility to restore when isolation ends.
pub(super) struct Isolation {
    hidden_nets: BTreeSet<NetId>,
    hidden_layers: BTreeSet<LayerId>,
}

impl BoardViewport {
    pub(super) fn isolation_active(&self) -> bool {
        self.isolation.is_some()
    }

    /// Nets of the current selection, including every member of a group selection.
    fn selected_nets(&self) -> BTreeSet<NetId> {
        let scene = &self.scene;
        let mut nets = BTreeSet::new();
        let mut objects: Vec<SelectedObject> =
            self.selected_related_objects.iter().copied().collect();
        match self.selected_target {
            Some(SelectionTarget::Net(net)) => {
                nets.insert(net);
            }
            Some(SelectionTarget::Object(object)) => objects.push(object),
            _ => {}
        }
        objects.extend(self.review.set.objects().iter().copied());
        for object in objects {
            let net = match object {
                SelectedObject::Segment(id) => {
                    scene.segments.iter().find(|s| s.id == id).map(|s| s.net)
                }
                SelectedObject::Pin(id) => scene.pins.iter().find(|p| p.id == id).map(|p| p.net),
                SelectedObject::Via(id) => scene.vias.iter().find(|v| v.id == id).map(|v| v.net),
                SelectedObject::Zone(id) => scene.zones.iter().find(|z| z.id == id).map(|z| z.net),
                SelectedObject::Drawing(_) | SelectedObject::Placement(_) => None,
            };
            nets.extend(net.filter(|net| net.0 != 0));
        }
        nets
    }

    /// Conductor layers that carry any object of the given nets.
    fn layers_of(&self, nets: &BTreeSet<NetId>) -> BTreeSet<LayerId> {
        let scene = &self.scene;
        let mut layers = BTreeSet::new();
        layers.extend(
            scene
                .segments
                .iter()
                .filter(|s| nets.contains(&s.net))
                .map(|s| s.layer),
        );
        layers.extend(
            scene
                .zones
                .iter()
                .filter(|z| nets.contains(&z.net))
                .map(|z| z.layer),
        );
        for pin in scene.pins.iter().filter(|p| nets.contains(&p.net)) {
            layers.extend(pin.pads.iter().map(|pad| pad.layer));
        }
        for via in scene.vias.iter().filter(|v| nets.contains(&v.net)) {
            layers.extend(via.pads.iter().map(|pad| pad.layer));
        }
        layers
    }

    pub(super) fn toggle_isolation(&mut self, cx: &mut Context<Self>) {
        if let Some(previous) = self.isolation.take() {
            let display = Arc::make_mut(&mut self.display);
            display.hidden_nets = previous.hidden_nets;
            display.hidden_layers = previous.hidden_layers;
            self.invalidate_hover();
            cx.notify();
            return;
        }
        let nets = self.selected_nets();
        if nets.is_empty() {
            self.review.notice = Some(Message::new(Key::IsolateNothing));
            cx.notify();
            return;
        }
        let used = self.layers_of(&nets);
        let display = Arc::make_mut(&mut self.display);
        self.isolation = Some(Isolation {
            hidden_nets: display.hidden_nets.clone(),
            hidden_layers: display.hidden_layers.clone(),
        });
        display.hidden_nets = self
            .scene
            .nets
            .keys()
            .copied()
            .filter(|net| net.0 != 0 && !nets.contains(net))
            .collect();
        for layer in self
            .scene
            .layers
            .iter()
            .filter(|layer| layer.id.is_copper())
        {
            if used.contains(&layer.id) {
                display.hidden_layers.remove(&layer.id);
            } else {
                display.hidden_layers.insert(layer.id);
            }
        }
        self.invalidate_hover();
        cx.notify();
    }
}

/// Visibility to restore when the adjacent-plane view ends.
pub(super) struct PlaneView {
    hidden_layers: BTreeSet<LayerId>,
    layer_primitives: BTreeMap<LayerId, pomelo_core::display::LayerPrimitives>,
}

impl BoardViewport {
    pub(super) fn plane_view_active(&self) -> bool {
        self.plane_view.is_some()
    }

    /// Show the selected net's routing layers together with the nearest plane
    /// layers above and below, planes as outlines so their splits read over the
    /// traces. Display only; no crossing is judged.
    pub(super) fn toggle_plane_view(&mut self, cx: &mut Context<Self>) {
        if let Some(previous) = self.plane_view.take() {
            let display = Arc::make_mut(&mut self.display);
            display.hidden_layers = previous.hidden_layers;
            display.layer_primitives = previous.layer_primitives;
            self.invalidate_hover();
            cx.notify();
            return;
        }
        let Some(report) = self.nets.report.as_ref().and_then(|(_, r)| r.clone()) else {
            self.review.notice = Some(Message::new(Key::PlaneViewNothing));
            cx.notify();
            return;
        };
        let mut routing = BTreeSet::new();
        let mut planes = BTreeSet::new();
        for (layer, above, below) in &report.adjacent_planes {
            routing.insert(*layer);
            planes.extend(above.iter().chain(below.iter()).copied());
        }
        if routing.is_empty() {
            self.review.notice = Some(Message::new(Key::PlaneViewNothing));
            cx.notify();
            return;
        }
        let display = Arc::make_mut(&mut self.display);
        self.plane_view = Some(PlaneView {
            hidden_layers: display.hidden_layers.clone(),
            layer_primitives: display.layer_primitives.clone(),
        });
        for layer in self
            .scene
            .layers
            .iter()
            .filter(|layer| layer.id.is_copper())
        {
            if routing.contains(&layer.id) || planes.contains(&layer.id) {
                display.hidden_layers.remove(&layer.id);
            } else {
                display.hidden_layers.insert(layer.id);
            }
        }
        for plane in &planes {
            display.set_cell_mode(
                *plane,
                pomelo_core::display::LayerPrimitive::Copper,
                pomelo_core::display::CellMode::Outline,
            );
        }
        self.invalidate_hover();
        cx.notify();
    }
}
