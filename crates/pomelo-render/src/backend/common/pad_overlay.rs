//! Identity of immutable selection inputs; colors and camera do not change membership.
use super::super::{TraceFrame, board::TraceSelection};
use pomelo_core::{
    model::{NetId, ObjectId},
    selection::{SelectedObject, SelectionTarget},
};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone)]
pub(super) struct OverlayKey {
    object: Option<SelectedObject>,
    net: Option<NetId>,
    trace: Option<(u8, ObjectId)>,
    pins: Option<Arc<BTreeSet<ObjectId>>>,
    related: Option<Arc<BTreeSet<SelectedObject>>>,
    hovered: Option<SelectedObject>,
    hover: Option<(SelectionTarget, Arc<BTreeSet<SelectedObject>>)>,
}

impl OverlayKey {
    pub fn new<S: super::trace::InstanceSource>(frame: &TraceFrame<S>) -> Self {
        let hover = frame.pass != super::super::board::OverlayPass::Selection;
        Self {
            object: frame.highlighted_object.map(|(object, _)| object),
            net: frame.highlighted_net.map(|(net, _)| net),
            trace: frame.highlighted_trace.map(|(trace, _)| match trace {
                TraceSelection::Segment(id) => (0, id),
                TraceSelection::Track(id) => (1, id),
            }),
            pins: frame
                .highlighted_objects
                .as_ref()
                .map(|(ids, _)| Arc::clone(ids)),
            related: frame
                .highlighted_related_objects
                .as_ref()
                .map(|(ids, _)| Arc::clone(ids)),
            hovered: hover
                .then(|| frame.hovered_object.map(|(object, _)| object))
                .flatten(),
            hover: hover.then(|| frame.hover_selection.clone()).flatten(),
        }
    }
}

fn same_arc<T>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl PartialEq for OverlayKey {
    fn eq(&self, other: &Self) -> bool {
        self.object == other.object
            && self.net == other.net
            && self.trace == other.trace
            && same_arc(&self.pins, &other.pins)
            && same_arc(&self.related, &other.related)
            && self.hovered == other.hovered
            && match (&self.hover, &other.hover) {
                (Some((target, members)), Some((next_target, next_members))) => {
                    target == next_target
                        && (!matches!(
                            target,
                            SelectionTarget::Component(_) | SelectionTarget::ComponentGroup(_)
                        ) || Arc::ptr_eq(members, next_members))
                }
                (None, None) => true,
                _ => false,
            }
    }
}
