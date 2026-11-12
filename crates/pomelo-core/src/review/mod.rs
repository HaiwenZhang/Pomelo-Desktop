//! Read-only review tools: measurement, clearance, selection sets, highlight
//! groups, named views and lightweight export. None of them write back to the design.

pub mod clearance;
pub mod export;
pub mod highlight;
pub mod measure;
pub mod net_groups;
pub mod net_report;
pub mod selection_set;
pub mod views;
pub mod visibility;

use highlight::{HighlightGroup, MAX_DOCUMENT_MEMBERS, MAX_HIGHLIGHT_GROUPS};
use views::{MAX_NAMED_VIEWS, NamedView};
use visibility::{MAX_VISIBILITY_VIEWS, VisibilityView};

/// Review state saved with a document's view. Older saved views without it
/// restore with no groups or named views.
#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewState {
    pub highlight_groups: Vec<HighlightGroup>,
    pub named_views: Vec<NamedView>,
    /// Saved layer, category and net visibility without a camera.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub visibility_views: Vec<VisibilityView>,
    /// Next identity for a group or view; identities are never reused in a document.
    pub next_id: u32,
}

impl ReviewState {
    pub fn valid(&self) -> bool {
        let ids = self
            .highlight_groups
            .iter()
            .map(|group| group.id)
            .chain(self.named_views.iter().map(|view| view.id))
            .chain(self.visibility_views.iter().map(|view| view.id));
        let mut seen = std::collections::BTreeSet::new();
        self.highlight_groups.len() <= MAX_HIGHLIGHT_GROUPS
            && self.named_views.len() <= MAX_NAMED_VIEWS
            && self.highlight_groups.iter().all(HighlightGroup::valid)
            && self.member_count() <= MAX_DOCUMENT_MEMBERS
            && self.named_views.iter().all(NamedView::valid)
            && self.visibility_views.len() <= MAX_VISIBILITY_VIEWS
            && self.visibility_views.iter().all(VisibilityView::valid)
            && ids
                .into_iter()
                .all(|id| id < self.next_id && seen.insert(id))
    }

    /// Members saved across all highlight groups.
    pub fn member_count(&self) -> usize {
        self.highlight_groups
            .iter()
            .map(|group| group.members.len())
            .sum()
    }

    pub fn allocate_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    pub fn is_empty(&self) -> bool {
        self.highlight_groups.is_empty()
            && self.named_views.is_empty()
            && self.visibility_views.is_empty()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{BoardScene, Bounds, Point};

    pub(crate) fn empty_scene() -> BoardScene {
        BoardScene {
            layers: vec![],
            special_layers: vec![],
            nets: Default::default(),
            segments: vec![],
            pins: vec![],
            components: vec![],
            vias: vec![],
            zones: vec![],
            outline: vec![],
            texts: vec![],
            drawing_layers: vec![],
            drawings: vec![],
            bounds: Bounds {
                min: Point::default(),
                max: Point::default(),
            },
            diagnostics: vec![],
        }
    }

    #[test]
    fn identities_must_be_unique_and_allocated() {
        let mut state = ReviewState::default();
        let id = state.allocate_id();
        state.highlight_groups.push(HighlightGroup::new(
            id,
            "a".into(),
            highlight::PALETTE[0],
            [],
        ));
        assert!(state.valid());
        state.highlight_groups.push(HighlightGroup::new(
            id,
            "b".into(),
            highlight::PALETTE[1],
            [],
        ));
        assert!(!state.valid());
        state.highlight_groups.pop();
        state.next_id = 0;
        assert!(!state.valid());
    }
}
