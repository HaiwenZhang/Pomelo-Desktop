//! Kit tooltip appearance with GPUI's element-owned hover lifetime.
use gpui_kit::{component::tooltip::Tooltip, *};

/// The Kit-managed overlay can outlive a button removed by a tab or panel change.
/// Register on the button's public interactivity instead, so GPUI releases the
/// tooltip with its owner. Keep Kit appearance and bound long source paths.
pub trait ButtonTooltipExt: InteractiveElement + Sized {
    fn native_tooltip(mut self, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        self.interactivity().tooltip(move |window, cx| {
            let label = label.clone();
            Tooltip::element(move |_, _| div().max_w_96().child(label.clone())).build(window, cx)
        });
        self
    }
}

impl ButtonTooltipExt for gpui_kit::component::button::Button {}
impl ButtonTooltipExt for gpui_kit::base::Tab {}
