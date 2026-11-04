//! Named net material assignments, independent of the automatic net palette.
use super::RgbColor;
use crate::display::DisplayCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NetColorKind {
    All,
    Traces,
    Pins,
    Vias,
    Shapes,
}
impl NetColorKind {
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Traces,
        Self::Pins,
        Self::Vias,
        Self::Shapes,
    ];
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetColors {
    pub all: Option<RgbColor>,
    pub traces: Option<RgbColor>,
    pub pins: Option<RgbColor>,
    pub vias: Option<RgbColor>,
    pub shapes: Option<RgbColor>,
}
impl NetColors {
    pub fn color(self, kind: NetColorKind) -> Option<RgbColor> {
        match kind {
            NetColorKind::All => self.all,
            NetColorKind::Traces => self.traces,
            NetColorKind::Pins => self.pins,
            NetColorKind::Vias => self.vias,
            NetColorKind::Shapes => self.shapes,
        }
    }
    pub fn set(&mut self, kind: NetColorKind, color: Option<RgbColor>) {
        match kind {
            // An entire-net command replaces its category assignments together.
            NetColorKind::All => {
                *self = Self {
                    all: color,
                    ..Self::default()
                }
            }
            NetColorKind::Traces => self.traces = color,
            NetColorKind::Pins => self.pins = color,
            NetColorKind::Vias => self.vias = color,
            NetColorKind::Shapes => self.shapes = color,
        }
    }
    pub fn material(self, category: DisplayCategory) -> Option<RgbColor> {
        let category = match category {
            DisplayCategory::Trace | DisplayCategory::BondWire => self.traces,
            DisplayCategory::Pin => self.pins,
            DisplayCategory::Via => self.vias,
            DisplayCategory::Zone | DisplayCategory::ZoneOutline => self.shapes,
            _ => return None,
        };
        category.or(self.all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        appearance::{BoardAppearance, ColorTarget},
        model::{LayerId, NetId},
    };
    #[test]
    fn net_assignments_round_trip_override_categories_and_hide_without_clearing() {
        let mut appearance = BoardAppearance::default();
        let net = NetId(7);
        let all = RgbColor([71, 131, 211]);
        let pin = RgbColor([13, 241, 17]);
        appearance.set_color(ColorTarget::Etch(LayerId(0)), Some(RgbColor([255, 0, 0])));
        appearance.set_color(ColorTarget::Net(net, NetColorKind::All), Some(all));
        appearance.set_color(ColorTarget::Net(net, NetColorKind::Pins), Some(pin));
        for category in [
            DisplayCategory::Trace,
            DisplayCategory::Via,
            DisplayCategory::Zone,
            DisplayCategory::ZoneOutline,
        ] {
            assert_eq!(appearance.net_material(net, category), Some(all));
        }
        assert_eq!(
            appearance.net_material(net, DisplayCategory::Pin),
            Some(pin)
        );
        for category in [
            DisplayCategory::Text,
            DisplayCategory::Drill,
            DisplayCategory::Drawing,
            DisplayCategory::Outline,
        ] {
            assert_eq!(appearance.net_material(net, category), None);
        }
        assert_eq!(
            appearance.net_material(NetId(8), DisplayCategory::Pin),
            None
        );
        let restored: BoardAppearance =
            serde_json::from_str(&serde_json::to_string(&appearance).unwrap()).unwrap();
        assert_eq!(restored, appearance);
        appearance.hide_net_colors = true;
        assert_eq!(appearance.net_material(net, DisplayCategory::Pin), None);
        assert_eq!(
            appearance.color(ColorTarget::Net(net, NetColorKind::Pins)),
            Some(pin)
        );
        appearance.hide_net_colors = false;
        appearance.set_color(ColorTarget::Net(net, NetColorKind::Pins), None);
        assert_eq!(
            appearance.net_material(net, DisplayCategory::Pin),
            Some(all)
        );
        appearance.set_color(ColorTarget::Net(net, NetColorKind::Pins), Some(pin));
        appearance.set_color(ColorTarget::Net(net, NetColorKind::All), Some(all));
        assert_eq!(
            appearance.color(ColorTarget::Net(net, NetColorKind::Pins)),
            None
        );
        appearance.set_color(ColorTarget::Net(net, NetColorKind::All), None);
        assert!(appearance.nets.is_empty());
        assert!(!appearance.layers.is_empty());
    }
}
