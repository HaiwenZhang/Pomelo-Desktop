//! Picking filter vocabulary; cancellation and selection state belong to the viewport.
use pomelo_core::{
    i18n::MessageKey as Key,
    interaction::SelectionMode,
    picking::{PickCategory, PickFilter},
};

/// Pickable categories in menu order with their localized names.
pub const CATEGORIES: [(PickCategory, Key); 5] = [
    (PickCategory::Segment, Key::LayerTraces),
    (PickCategory::Via, Key::LayerVias),
    (PickCategory::Pin, Key::PickPadsAndPlacements),
    (PickCategory::Zone, Key::PickCopper),
    (PickCategory::Drawing, Key::PickDrawings),
];

/// Explains why a canvas click cannot select anything with this combination.
pub fn filter_hint(filter: PickFilter, mode: SelectionMode) -> Option<Key> {
    if filter == PickFilter::none() {
        Some(Key::PickNoneEnabled)
    } else if mode == SelectionMode::Track && !filter.contains(PickCategory::Segment) {
        Some(Key::PickTrackDisabled)
    } else if mode == SelectionMode::Component
        && !filter.contains(PickCategory::Pin)
        && !filter.contains(PickCategory::Segment)
        && !filter.contains(PickCategory::Via)
    {
        Some(Key::PickComponentDisabled)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_explains_filters_that_leave_the_current_mode_without_targets() {
        assert_eq!(
            filter_hint(PickFilter::none(), SelectionMode::Object),
            Some(Key::PickNoneEnabled)
        );
        let mut filter = PickFilter::all();
        filter.set(PickCategory::Segment, false);
        assert_eq!(
            filter_hint(filter, SelectionMode::Track),
            Some(Key::PickTrackDisabled)
        );
        assert_eq!(filter_hint(filter, SelectionMode::Net), None);
        for category in [PickCategory::Pin, PickCategory::Via] {
            filter.set(category, false);
        }
        assert_eq!(
            filter_hint(filter, SelectionMode::Component),
            Some(Key::PickComponentDisabled)
        );
        assert_eq!(
            filter_hint(PickFilter::all(), SelectionMode::Component),
            None
        );
    }
}
