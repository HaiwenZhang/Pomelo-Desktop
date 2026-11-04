//! Search anchors follow Web BoardIndex submission order and visibility.
use super::*;
use crate::{
    display::{DisplayCategory, LayerPrimitive},
    interaction::membership::Membership,
    model::{LayerId, Pad},
    selection::SelectionTarget,
};
use std::collections::{BTreeMap, BTreeSet};

/// Runtime hit identity, separate from the canonical identity of a selected group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionAnchor {
    pub object: SelectedObject,
    pub layer: LayerId,
    pub category: DisplayCategory,
}

struct Candidate {
    anchor: SelectionAnchor,
    rank: [i64; 5],
    sequence: [usize; 5],
}

#[derive(Default)]
struct Anchors {
    last: Option<Candidate>,
    visible: Option<Candidate>,
    preferred: Option<SelectionAnchor>,
    preferred_found: bool,
}
impl Anchors {
    fn include(
        &mut self,
        anchor: SelectionAnchor,
        sequence: [usize; 5],
        visible: bool,
        display: &BoardDisplay,
    ) {
        self.preferred_found |= self.preferred.is_some_and(|preferred| {
            preferred == anchor
                || (preferred.object == anchor.object
                    && preferred.layer == anchor.layer
                    && preferred.category == DisplayCategory::ZoneOutline
                    && anchor.category == DisplayCategory::Zone)
        });
        let rank = display.display_rank(anchor.layer, anchor.category);
        let later = |current: &Option<Candidate>| {
            current
                .as_ref()
                .is_none_or(|old| (rank, sequence) > (old.rank, old.sequence))
        };
        if visible && later(&self.visible) {
            self.visible = Some(Candidate {
                anchor,
                rank,
                sequence,
            });
        }
        if later(&self.last) {
            self.last = Some(Candidate {
                anchor,
                rank,
                sequence,
            });
        }
    }
    fn finish(self) -> Option<SelectionAnchor> {
        if self.preferred_found {
            return self.preferred;
        }
        self.visible.or(self.last).map(|candidate| candidate.anchor)
    }
}

fn pad_category(pin: &crate::model::Pin) -> DisplayCategory {
    if pin.die.is_some() {
        DisplayCategory::Trace
    } else {
        DisplayCategory::Pin
    }
}

fn pad_pass(pad: &Pad) -> usize {
    if pad.backdrill_base {
        6
    } else if pad.custom.is_some() {
        8
    } else {
        7
    }
}

impl BoardPickingIndex {
    /// Highest visible indexed entry, falling back to the highest hidden entry.
    /// All group members participate; visibility never reduces navigation bounds.
    /// Run on a worker with the same display snapshot as the locate request.
    pub fn selection_anchor(
        &self,
        target: SelectionTarget,
        display: &BoardDisplay,
        cancel: &CancellationToken,
    ) -> Result<Option<SelectionAnchor>, PathError> {
        self.selection_anchor_with_preferred(target, display, None, cancel)
    }

    /// Retain a saved/picked entry only if it still belongs to the selected target
    /// and exists in indexed geometry. Otherwise recompute the search anchor.
    pub fn selection_anchor_with_preferred(
        &self,
        target: SelectionTarget,
        display: &BoardDisplay,
        preferred: Option<SelectionAnchor>,
        cancel: &CancellationToken,
    ) -> Result<Option<SelectionAnchor>, PathError> {
        cancel.check()?;
        let scene = &self.scene;
        let target = target.canonical_reference(scene, cancel)?;
        // Every via is still scanned below: drill scope groups count unselected vias too.
        let membership = Membership::new(target, scene, cancel)?;
        let mut result = Anchors {
            preferred,
            ..Anchors::default()
        };
        for (source, segment) in scene.segments.iter().enumerate() {
            cancel.check()?;
            if membership.segment(segment) {
                let category = if segment.bond_wire.is_some() {
                    DisplayCategory::BondWire
                } else {
                    DisplayCategory::Trace
                };
                result.include(
                    SelectionAnchor {
                        object: SelectedObject::Segment(segment.id),
                        layer: segment.layer,
                        category,
                    },
                    [0, usize::from(segment.arc.is_some()), 0, source, 0],
                    display.primitive_visible(segment.layer, LayerPrimitive::Traces),
                    display,
                );
            }
        }
        for (source, zone) in scene.zones.iter().enumerate() {
            cancel.check()?;
            if membership.zone(zone)
                && (zone.paths.first().is_some_and(|path| !path.is_empty())
                    || !zone.mesh.ring_bounds.is_empty())
            {
                result.include(
                    SelectionAnchor {
                        object: SelectedObject::Zone(zone.id),
                        layer: zone.layer,
                        category: DisplayCategory::Zone,
                    },
                    [1, 0, 0, source, 0],
                    display.show_copper
                        && display.primitive_visible(zone.layer, LayerPrimitive::Traces),
                    display,
                );
            }
        }
        let mut drawings = BTreeSet::new();
        for drawing in &scene.drawings {
            cancel.check()?;
            if membership.drawing(drawing) {
                drawings.insert(drawing.id);
            }
        }
        for entry in &self.drawings.entries {
            cancel.check()?;
            if !drawings.contains(&entry.owner) {
                continue;
            }
            let text = matches!(
                entry.geometry,
                super::drawings::DrawingGeometry::Glyphs { .. }
            );
            let category = if text {
                DisplayCategory::Text
            } else {
                DisplayCategory::Drawing
            };
            result.include(
                SelectionAnchor {
                    object: SelectedObject::Drawing(entry.owner),
                    layer: entry.layer,
                    category,
                },
                [if text { 3 } else { 2 }, 0, 0, entry.sequence, 0],
                display.layer_visible(entry.layer)
                    && if text {
                        display.show_texts
                            && (!entry.layer.is_copper() || display.primitives(entry.layer).traces)
                    } else {
                        display.show_drawings
                    },
                display,
            );
        }
        for (source, pin) in scene.pins.iter().enumerate() {
            cancel.check()?;
            if !membership.pin(pin) {
                continue;
            }
            let object = SelectedObject::Pin(pin.id);
            let category = pad_category(pin);
            for (pad_index, pad) in pin.pads.iter().enumerate() {
                cancel.check()?;
                if !pad.supported() || pad.backdrill {
                    continue;
                }
                result.include(
                    SelectionAnchor {
                        object,
                        layer: pad.layer,
                        category,
                    },
                    [pad_pass(pad), 0, 0, source, pad_index],
                    !(pad.backdrill_base && display.show_backdrills)
                        && display.primitive_visible(
                            pad.layer,
                            if category == DisplayCategory::Trace {
                                LayerPrimitive::Traces
                            } else {
                                LayerPrimitive::Pads
                            },
                        ),
                    display,
                );
            }
            if pin.drill_shape.pad().is_some() {
                result.include(
                    SelectionAnchor {
                        object,
                        layer: LayerId::UNASSIGNED,
                        category: DisplayCategory::Drill,
                    },
                    [4, 0, 1, source, 0],
                    display.show_drills,
                    display,
                );
            }
        }
        // Scope groups keep their first source appearance, including groups that
        // contain no selected member. Copper pads are emitted after every drill.
        let mut holes = BTreeMap::<Vec<LayerId>, usize>::new();
        let mut backdrills = BTreeMap::<Vec<LayerId>, usize>::new();
        for (source, via) in scene.vias.iter().enumerate() {
            cancel.check()?;
            let selected = membership.via(via);
            let object = SelectedObject::Via(via.id);
            let mut scope = Vec::new();
            let mut back_scope = Vec::new();
            for (pad_index, pad) in via.pads.iter().enumerate() {
                cancel.check()?;
                scope.push(pad.layer);
                if pad.backdrill {
                    back_scope.push(pad.layer);
                }
                if !selected || !pad.supported() || pad.backdrill {
                    continue;
                }
                result.include(
                    SelectionAnchor {
                        object,
                        layer: pad.layer,
                        category: DisplayCategory::Via,
                    },
                    [pad_pass(pad), 0, 1, source, pad_index],
                    !(pad.backdrill_base && display.show_backdrills)
                        && display.primitive_visible(pad.layer, LayerPrimitive::Vias),
                    display,
                );
            }
            if via.drill_shape.pad().is_some() {
                scope.sort_unstable();
                scope.dedup();
                let visible = display.drill_visible(Some(&scope));
                let ordinal = holes.len() + 1;
                let group = *holes.entry(scope).or_insert(ordinal);
                if selected {
                    result.include(
                        SelectionAnchor {
                            object,
                            layer: LayerId::UNASSIGNED,
                            category: DisplayCategory::Drill,
                        },
                        [4, group, 0, source, 0],
                        visible,
                        display,
                    );
                }
            }
            if via.backdrill.is_some() && via.pads.iter().any(|pad| pad.backdrill) {
                let visible = display.backdrill_visible(&back_scope);
                let ordinal = backdrills.len();
                let group = *backdrills.entry(back_scope).or_insert(ordinal);
                if selected {
                    result.include(
                        SelectionAnchor {
                            object,
                            layer: LayerId::UNASSIGNED,
                            category: DisplayCategory::Drill,
                        },
                        [5, group, 0, source, 0],
                        visible,
                        display,
                    );
                }
            }
        }
        let placements = target.placement_ids(scene, cancel)?;
        for (source, component) in scene.components.iter().enumerate() {
            cancel.check()?;
            if placements.contains(&component.id)
                && let Some(body) = &component.body
                && super::placements::bounds(body).is_some()
            {
                result.include(
                    SelectionAnchor {
                        object: SelectedObject::Placement(component.id),
                        layer: body.layer,
                        category: DisplayCategory::Placement,
                    },
                    [6, 0, 0, source, 0],
                    display.category_visible(body.layer, DisplayCategory::Placement),
                    display,
                );
            }
        }
        cancel.check()?;
        Ok(result.finish())
    }
}
