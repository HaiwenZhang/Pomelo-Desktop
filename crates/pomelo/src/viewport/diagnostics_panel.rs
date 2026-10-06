//! Import and render diagnostic presentation for an open document.
use super::*;
impl BoardViewport {
    pub(super) fn diagnostics_panel(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let locale = i18n::current(cx);
        let count = self.scene.diagnostics.len() + self.render_diagnostics.len();
        let mut panel = div().flex().flex_col().min_h_0().gap_2().child(
            self.inspector_focus.diagnostics.wrap(
                Button::new("import-diagnostics")
                    .ghost()
                    .selected(self.diagnostics_expanded)
                    .disabled(count == 0)
                    .label(
                        Message::new(Key::ImportDiagnostics)
                            .arg("count", count)
                            .display(locale),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.diagnostics_expanded = !this.diagnostics_expanded;
                        cx.notify();
                    })),
            ),
        );
        if !self.diagnostics_expanded || count == 0 {
            return panel.into_any_element();
        }
        let last_page = (count - 1) / crate::document::DIAGNOSTICS_PAGE_SIZE;
        let page = self.diagnostics_page.min(last_page);
        let range = crate::document::diagnostics_range(count, page);
        let rows = crate::panels::diagnostics::page_rows(
            locale,
            page,
            self.scene
                .diagnostics
                .iter()
                .chain(&self.render_diagnostics)
                .skip(range.start)
                .take(range.len()),
            cx,
        );
        panel = panel.child(rows).child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(
                    self.inspector_focus.previous.wrap(
                        crate::panels::diagnostics::page_button(
                            "diagnostics-previous",
                            text(locale, Key::MembersPrevious),
                            &self.diagnostics_navigation.previous,
                            page == 0,
                            window,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.diagnostics_page = page.saturating_sub(1);
                                this.diagnostics_navigation.focus_boundary(
                                    this.diagnostics_page,
                                    last_page,
                                    window,
                                    cx,
                                );
                                cx.notify();
                            },
                        )),
                    ),
                )
                .child(
                    Message::new(Key::DiagnosticPage)
                        .arg("current", page + 1)
                        .arg("total", last_page + 1)
                        .display(locale),
                )
                .child(
                    self.inspector_focus.next.wrap(
                        crate::panels::diagnostics::page_button(
                            "diagnostics-next",
                            text(locale, Key::MembersNext),
                            &self.diagnostics_navigation.next,
                            page == last_page,
                            window,
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.diagnostics_page = (page + 1).min(last_page);
                                this.diagnostics_navigation.focus_boundary(
                                    this.diagnostics_page,
                                    last_page,
                                    window,
                                    cx,
                                );
                                cx.notify();
                            },
                        )),
                    ),
                ),
        );
        panel.into_any_element()
    }
}
