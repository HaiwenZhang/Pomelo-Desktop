//! Selection details are prepared together off the UI thread and published or
//! cleared together, so the inspector never mixes two selections' data.
use super::*;
use crate::panels::inspector::{PreparedInspection, prepare_inspection};
use pomelo_core::{
    geometry::PathError,
    picking_index::SelectionAnchor,
    selection::{SelectionMembers, SelectionSummary, SelectionTarget},
    task::CancellationToken,
};

pub(super) struct PreparedSelection {
    target: SelectionTarget,
    anchor: Option<SelectionAnchor>,
    summary: SelectionSummary,
    members: SelectionMembers,
    pins: Arc<BTreeSet<pomelo_core::model::ObjectId>>,
    inspection: PreparedInspection,
}

impl PreparedSelection {
    /// Run on a worker; cancellation discards every partial result.
    pub(super) fn prepare(
        target: SelectionTarget,
        anchor: Option<SelectionAnchor>,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Self, PathError> {
        Ok(Self {
            summary: target.summarize(scene, cancel)?,
            members: target.members(scene, 256, cancel)?,
            pins: Arc::new(target.pin_ids(scene, cancel)?),
            inspection: prepare_inspection(target, anchor, scene, cancel)?,
            target,
            anchor,
        })
    }

    pub(super) fn target(&self) -> SelectionTarget {
        self.target
    }
}

impl BoardViewport {
    /// Member paging restarts at the first row of the new selection.
    pub(super) fn publish_selection(&mut self, selection: PreparedSelection) {
        self.members_offset = 0;
        self.members_page_revision = self.members_page_revision.wrapping_add(1);
        self.members_scroll.set_offset(point(px(0.), px(0.)));
        self.selected_members = Some(selection.members);
        self.selected_target = Some(selection.target);
        self.selected_anchor = selection.anchor;
        self.selected_related_net = selection.inspection.net;
        self.selected_related_component = selection.inspection.component;
        self.selected_source_labels = selection.inspection.labels;
        self.selected_related_objects = selection.inspection.related_objects;
        self.selected_summary = Some(selection.summary);
        self.selected_pins = selection.pins;
    }

    pub(super) fn clear_selection_details(&mut self) {
        self.selected_target = None;
        self.selected_anchor = None;
        self.selected_source_labels.clear();
        self.selected_related_net = None;
        self.selected_related_component = None;
        self.selected_summary = None;
        self.selected_members = None;
        self.selected_pins = Arc::new(BTreeSet::new());
        self.selected_related_objects = Arc::new(BTreeSet::new());
    }
}
