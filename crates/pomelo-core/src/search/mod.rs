//! Locale-independent matching of source names; results retain domain identity.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    geometry::PathError,
    model::{BoardScene, Bounds, NetId, ObjectId},
    pad::PadPlacement,
    task::CancellationToken,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchTarget {
    Net(NetId),
    Component(ObjectId),
}

impl SearchTarget {
    /// Source-space bounds independent of current layer visibility and camera.
    /// Run off the UI thread for large boards; cancellation discards partial bounds.
    pub fn bounds(
        self,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Option<Bounds>, PathError> {
        if cancel.is_cancelled() {
            return Err(PathError::Cancelled);
        }
        if self == Self::Net(NetId(0)) {
            return Ok(None);
        }
        let mut result: Option<Bounds> = None;
        let mut include = |bounds: Option<Bounds>, id| -> Result<(), PathError> {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            let bounds = bounds
                .filter(|value| value.is_valid())
                .ok_or(PathError::Invalid(id))?;
            if let Some(result) = &mut result {
                result.include(bounds.min);
                result.include(bounds.max);
            } else {
                result = Some(bounds);
            }
            Ok(())
        };
        let component = match self {
            Self::Component(id) => scene.components.iter().find(|value| value.id == id),
            Self::Net(_) => None,
        };
        if let Some(component) = component {
            include(Bounds::from_points([component.at]), component.id)?;
        }
        let mut remaining_pins = BTreeSet::new();
        if let Some(component) = component {
            for id in &component.pins {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if !remaining_pins.insert(*id) {
                    return Err(PathError::Invalid(*id));
                }
            }
        }
        for segment in &scene.segments {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            if matches!(self, Self::Net(net) if net == segment.net) {
                include(segment.bounds(), segment.id)?;
            }
        }
        for pin in &scene.pins {
            if cancel.is_cancelled() {
                return Err(PathError::Cancelled);
            }
            let selected = match self {
                Self::Net(net) => pin.net == net,
                Self::Component(_) => remaining_pins.remove(&pin.id),
            };
            if !selected {
                continue;
            }
            let owner = PadPlacement {
                at: pin.at,
                angle: pin.angle,
                mirrored: pin.mirrored,
            };
            include(Bounds::from_points([pin.at]), pin.id)?;
            for pad in &pin.pads {
                include(pad.bounds(owner), pin.id)?;
            }
            if let Some(pad) = pin.drill_shape.pad() {
                include(pad.bounds(owner), pin.id)?;
            }
        }
        if let Some(id) = remaining_pins.first() {
            return Err(PathError::Invalid(*id));
        }
        if let Self::Net(net) = self {
            for via in &scene.vias {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if via.net != net {
                    continue;
                }
                let owner = PadPlacement {
                    at: via.at,
                    angle: via.angle,
                    mirrored: via.mirrored,
                };
                include(Bounds::from_points([via.at]), via.id)?;
                for pad in via.pads.iter() {
                    include(pad.bounds(owner), via.id)?;
                }
                if let Some(pad) = via.drill_shape.pad() {
                    include(pad.bounds(owner), via.id)?;
                }
            }
            for zone in &scene.zones {
                if cancel.is_cancelled() {
                    return Err(PathError::Cancelled);
                }
                if zone.net != net {
                    continue;
                }
                for bounds in &zone.mesh.ring_bounds {
                    include(Some(*bounds), zone.id)?;
                }
                for path in &zone.paths {
                    for segment in path {
                        include(segment.centreline_bounds(), zone.id)?;
                    }
                }
            }
        }
        if cancel.is_cancelled() {
            Err(PathError::Cancelled)
        } else {
            Ok(result)
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchEntry {
    pub target: SearchTarget,
    pub name: String,
    pub count: usize,
    folded: String,
}

impl SearchEntry {
    pub fn new(target: SearchTarget, name: String, count: usize) -> Self {
        let folded = name.to_lowercase();
        Self {
            target,
            name,
            count,
            folded,
        }
    }
}

#[derive(Debug, Default)]
pub struct SearchIndex {
    entries: Vec<SearchEntry>,
}

impl SearchIndex {
    pub fn entries(&self) -> &[SearchEntry] {
        &self.entries
    }
    /// Build before publishing a document; cancellation never publishes a partial index.
    pub fn build(scene: &BoardScene, cancel: &CancellationToken) -> Option<Self> {
        let mut counts = BTreeMap::new();
        let mut order = Vec::new();
        for net in scene
            .segments
            .iter()
            .map(|value| value.net)
            .chain(scene.vias.iter().map(|value| value.net))
            .chain(scene.pins.iter().map(|value| value.net))
            .chain(scene.zones.iter().map(|value| value.net))
        {
            if cancel.is_cancelled() {
                return None;
            }
            if net.0 == 0 {
                continue;
            }
            let count = counts.entry(net).or_insert_with(|| {
                order.push(net);
                0
            });
            *count += 1;
        }
        let mut entries = Vec::new();
        for net in order {
            if cancel.is_cancelled() {
                return None;
            }
            entries.push(SearchEntry::new(
                SearchTarget::Net(net),
                scene
                    .nets
                    .get(&net)
                    .filter(|name| !name.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| net.0.to_string()),
                counts[&net],
            ));
        }
        for component in &scene.components {
            if cancel.is_cancelled() {
                return None;
            }
            if !component.reference.is_empty() {
                entries.push(SearchEntry::new(
                    SearchTarget::Component(component.id),
                    component.reference.clone(),
                    component.pins.len(),
                ));
            }
        }
        if cancel.is_cancelled() {
            None
        } else {
            Some(Self::new(entries))
        }
    }

    pub fn new(entries: Vec<SearchEntry>) -> Self {
        Self { entries }
    }

    /// Exact names precede prefixes, then substring matches. Ties keep source order.
    /// Unicode lowercase is deterministic across UI locales; source text is unchanged.
    pub fn find(&self, query: &str, limit: usize) -> Vec<&SearchEntry> {
        self.find_cancellable(query, limit, &CancellationToken::default())
            .unwrap_or_default()
    }

    /// Cancellation returns no partial results. At most `limit` references are retained.
    pub fn find_cancellable(
        &self,
        query: &str,
        limit: usize,
        cancel: &CancellationToken,
    ) -> Option<Vec<&SearchEntry>> {
        if cancel.is_cancelled() {
            return None;
        }
        let query = query.trim().to_lowercase();
        if query.is_empty() || limit == 0 {
            return Some(Vec::new());
        }
        let rank = |name: &str| {
            if name == query {
                0
            } else if name.starts_with(&query) {
                1
            } else {
                2
            }
        };
        let compare = |a: &SearchEntry, b: &SearchEntry| {
            rank(&a.folded)
                .cmp(&rank(&b.folded))
                .then_with(|| a.folded.cmp(&b.folded))
        };
        let mut matches: Vec<&SearchEntry> = Vec::new();
        for entry in &self.entries {
            if cancel.is_cancelled() {
                return None;
            }
            if !entry.folded.contains(&query) {
                continue;
            }
            let position = matches.partition_point(|other| !compare(other, entry).is_gt());
            if position >= limit {
                continue;
            }
            if matches.len() == limit {
                matches.pop();
            }
            matches.insert(position, entry);
        }
        if cancel.is_cancelled() {
            None
        } else {
            Some(matches)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_query_keeps_late_exact_matches_and_stable_duplicate_order() {
        let index = SearchIndex::new(
            (0..10_000)
                .map(|id| {
                    SearchEntry::new(
                        SearchTarget::Net(NetId(id)),
                        if id >= 9_998 {
                            "power".into()
                        } else {
                            format!("power_{id:05}")
                        },
                        1,
                    )
                })
                .collect(),
        );
        let result = index.find("power", 3);
        assert_eq!(
            result.iter().map(|entry| entry.target).collect::<Vec<_>>(),
            vec![
                SearchTarget::Net(NetId(9_998)),
                SearchTarget::Net(NetId(9_999)),
                SearchTarget::Net(NetId(0))
            ]
        );
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert!(index.find_cancellable("power", 3, &cancel).is_none());
        assert!(index.find_cancellable("", 0, &cancel).is_none());
    }

    #[test]
    fn matching_keeps_duplicate_component_identities_and_ranks_exact_first() {
        let index = SearchIndex::new(vec![
            SearchEntry::new(SearchTarget::Net(NetId(1)), "VCC_A".into(), 3),
            SearchEntry::new(SearchTarget::Component(ObjectId(8)), "vcc".into(), 2),
            SearchEntry::new(SearchTarget::Component(ObjectId(9)), "vcc".into(), 4),
            SearchEntry::new(SearchTarget::Net(NetId(2)), "AVCC".into(), 1),
        ]);
        let targets: Vec<_> = index
            .find(" VcC ", 3)
            .iter()
            .map(|entry| entry.target)
            .collect();
        assert_eq!(
            targets,
            vec![
                SearchTarget::Component(ObjectId(8)),
                SearchTarget::Component(ObjectId(9)),
                SearchTarget::Net(NetId(1))
            ]
        );
        assert!(index.find(" ", 20).is_empty());
        assert!(index.find("vcc", 0).is_empty());
    }

    #[test]
    fn cjk_and_mixed_names_are_preserved_without_translation() {
        let index = SearchIndex::new(vec![SearchEntry::new(
            SearchTarget::Net(NetId(4)),
            "電源_전원_A".into(),
            1,
        )]);
        assert_eq!(index.find("전원_a", 20)[0].name, "電源_전원_A");
        assert_eq!(
            index.find("電源", 20)[0].target,
            SearchTarget::Net(NetId(4))
        );
    }
}
