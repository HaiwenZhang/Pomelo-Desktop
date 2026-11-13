//! Workbench rendering: title bar, document tabs, import states and the welcome page.
use super::*;

impl Workbench {
    pub(super) fn content(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let locale = i18n::current(cx);
        let Some(document) = self.active.and_then(|index| self.documents.get(index)) else {
            return crate::welcome::render(
                locale,
                self.recent_list(
                    if self.recent_only {
                        RecentPresentation::All
                    } else {
                        RecentPresentation::Wall
                    },
                    cx,
                ),
                self.recent_only,
                &self.welcome_scroll,
                cx,
            );
        };
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        if let DocumentStatus::Imported(_) = &document.status
            && let Some(viewport) = &document.viewport
        {
            return div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(viewport.clone())
                .into_any_element();
        }
        if matches!(
            document.status,
            DocumentStatus::Queued | DocumentStatus::Reading
        ) {
            return import_progress::render(document, locale, cx);
        }
        let mut content = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .p_6()
            .gap_4()
            .child(
                div().text_lg().child(
                    document
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                ),
            );
        match &document.status {
            DocumentStatus::Queued | DocumentStatus::Reading => unreachable!(),
            DocumentStatus::Cancelled => {
                content = content
                    .child(text(locale, Key::Cancelled))
                    .child(self.retry_encoding(document, cx))
                    .child(
                        Button::new("retry-cancelled-import")
                            .label(text(locale, Key::RetryImport))
                            .on_click(cx.listener(Self::retry_import)),
                    );
            }
            DocumentStatus::Failed(error) => {
                content = content.child(
                    div()
                        .text_color(theme.danger)
                        .child(error.message.display(locale)),
                );
                content = content.child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(document.path.to_string_lossy().into_owned()),
                );
                content = content.child(self.retry_encoding(document, cx)).child(
                    Button::new("retry-failed-import")
                        .label(text(locale, Key::RetryImport))
                        .on_click(cx.listener(Self::retry_import)),
                );
                if let Some(details) = &error.technical_details {
                    content = content
                        .child(div().text_sm().child(text(locale, Key::TechnicalDetails)))
                        .child(
                            div()
                                .text_sm()
                                .child(SharedString::from(details.to_string())),
                        );
                }
            }
            DocumentStatus::Imported(document) => {
                let board = &document.board;
                for diagnostic in &document.render_diagnostics {
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(theme.warning)
                            .child(diagnostic.message.display(locale)),
                    );
                }
                content = content
                    .child(text(locale, Key::SceneImported))
                    .child(if board.source.format == "allegro" {
                        Message::new(Key::HeaderFormat)
                            .arg("format", format_display_name(&board.source.format))
                            .arg("version", board.source.layout_version)
                            .arg("writer", board.source.writer_version.as_str())
                            .display(locale)
                    } else {
                        format_display_name(&board.source.format).to_owned()
                    })
                    .child(
                        Message::new(Key::SceneSummary)
                            .arg("layers", board.scene.layers.len())
                            .arg("segments", board.scene.segments.len())
                            .arg("pins", board.scene.pins.len())
                            .arg("vias", board.scene.vias.len())
                            .arg("zones", board.scene.zones.len())
                            .display(locale),
                    );
                if let Some(session) = self.active.and_then(|index| self.documents.get(index)) {
                    content = content.child(self.import_diagnostics(
                        session,
                        &board.scene.diagnostics,
                        window,
                        cx,
                    ));
                }
                #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                if let Some(viewport) = self
                    .active
                    .and_then(|index| self.documents.get(index))
                    .and_then(|session| session.viewport.as_ref())
                {
                    content =
                        content.child(div().flex_1().min_h_0().min_w_0().child(viewport.clone()));
                }
                #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
                {
                    content = content.child(text(locale, Key::ViewportPending));
                }
            }
        }
        content.into_any_element()
    }

    pub(super) fn import_diagnostics(
        &self,
        session: &DocumentSession,
        diagnostics: &[Diagnostic],
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let locale = i18n::current(cx);
        let request = session.request;
        let count = diagnostics.len();
        let mut panel = div().flex().flex_col().min_h_0().gap_2().child(
            Button::new("import-diagnostics")
                .ghost()
                .selected(session.diagnostics_expanded)
                .disabled(count == 0)
                .label(
                    Message::new(Key::ImportDiagnostics)
                        .arg("count", count)
                        .display(locale),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(document) = this
                        .documents
                        .iter_mut()
                        .find(|document| document.request == request)
                    {
                        document.diagnostics_expanded = !document.diagnostics_expanded;
                        cx.notify();
                    }
                })),
        );
        if !session.diagnostics_expanded || count == 0 {
            return panel.into_any_element();
        }
        let last_page = (count - 1) / crate::document::DIAGNOSTICS_PAGE_SIZE;
        let page = session.diagnostics_page.min(last_page);
        let range = crate::document::diagnostics_range(count, page);
        let rows = crate::panels::diagnostics::page_rows(locale, page, &diagnostics[range], cx);
        panel = panel.child(rows).child(
            div()
                .flex()
                .items_center()
                .flex_wrap()
                .gap_2()
                .child(
                    crate::panels::diagnostics::page_button(
                        "diagnostics-previous",
                        text(locale, Key::MembersPrevious),
                        &self.diagnostics_navigation.previous,
                        page == 0,
                        window,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(document) = this
                            .documents
                            .iter_mut()
                            .find(|document| document.request == request)
                        {
                            document.diagnostics_page = page.saturating_sub(1);
                            this.diagnostics_navigation.focus_boundary(
                                document.diagnostics_page,
                                last_page,
                                window,
                                cx,
                            );
                            cx.notify();
                        }
                    })),
                )
                .child(
                    Message::new(Key::DiagnosticPage)
                        .arg("current", page + 1)
                        .arg("total", last_page + 1)
                        .display(locale),
                )
                .child(
                    crate::panels::diagnostics::page_button(
                        "diagnostics-next",
                        text(locale, Key::MembersNext),
                        &self.diagnostics_navigation.next,
                        page == last_page,
                        window,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(document) = this
                            .documents
                            .iter_mut()
                            .find(|document| document.request == request)
                        {
                            document.diagnostics_page = (page + 1).min(last_page);
                            this.diagnostics_navigation.focus_boundary(
                                document.diagnostics_page,
                                last_page,
                                window,
                                cx,
                            );
                            cx.notify();
                        }
                    })),
                ),
        );
        panel.into_any_element()
    }

    pub(super) fn retry_encoding(
        &self,
        document: &DocumentSession,
        cx: &Context<Self>,
    ) -> AnyElement {
        use pomelo_import::TextEncoding;
        let locale = i18n::current(cx);
        let request = document.request;
        let current = document.import_options.text_encoding;
        let mut choices = div().flex().flex_wrap().gap_2();
        for encoding in [
            TextEncoding::Auto,
            TextEncoding::Utf8,
            TextEncoding::Gbk,
            TextEncoding::Big5,
            TextEncoding::ShiftJis,
            TextEncoding::Windows1252,
        ] {
            choices = choices.child(
                Button::new(encoding.tag())
                    .ghost()
                    .label(if matches!(encoding, TextEncoding::Auto) {
                        text(locale, Key::EncodingAuto)
                    } else {
                        encoding.tag().to_owned()
                    })
                    .selected(current.tag() == encoding.tag())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(document) = this
                            .documents
                            .iter_mut()
                            .find(|document| document.request == request)
                            && document.set_retry_encoding(encoding)
                        {
                            cx.notify();
                        }
                    })),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_sm().child(text(locale, Key::RetryEncoding)))
            .child(choices)
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(text(locale, Key::RetryEncodingHint)),
            )
            .into_any_element()
    }

    pub(super) fn recent_list(
        &self,
        presentation: RecentPresentation,
        cx: &Context<Self>,
    ) -> AnyElement {
        let locale = i18n::current(cx);
        let entries = &cx.global::<crate::services::recent::RecentState>().entries;
        crate::welcome::recent::render(
            locale,
            entries,
            presentation,
            &cx.global::<crate::services::recent::RecentState>().previews,
            &self.welcome_scroll,
            cx,
            |entry| {
                let open_path = entry.path.clone();
                let relocate_path = entry.path.clone();
                let encoding = entry.encoding;
                let remove_path = entry.path.clone();
                crate::welcome::recent::RecentCommands {
                    open: Box::new(cx.listener(move |this, _, window, cx| {
                        let mut options = this.import_options.clone();
                        options.text_encoding = encoding;
                        this.open_path_with_options(open_path.clone(), options, None, window, cx);
                    })),
                    relocate: Box::new(cx.listener(move |this, _, window, cx| {
                        this.relocate_recent(relocate_path.clone(), encoding, window, cx)
                    })),
                    remove: Box::new(cx.listener(move |_, _, _, cx| {
                        crate::services::recent::remove(&remove_path, cx)
                    })),
                }
            },
        )
    }

    /// Language, appearance and settings, in the title row while no board is open.
    fn app_preferences(
        &self,
        locale: Locale,
        preference: LanguagePreference,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_1()
            .flex_shrink_0()
            .child(
                Button::new("language")
                    .ghost()
                    .small()
                    .label(locale.native_name())
                    .dropdown_menu(move |mut menu, _, _| {
                        menu = menu
                            .item(
                                PopupMenuItem::new(text(locale, Key::FollowSystem))
                                    .checked(preference == LanguagePreference::System)
                                    .action(i18n::action(LanguagePreference::System)),
                            )
                            .separator();
                        for language in Locale::ALL {
                            let candidate = LanguagePreference::Explicit(language);
                            menu = menu.item(
                                PopupMenuItem::new(language.native_name())
                                    .checked(preference == candidate)
                                    .action(i18n::action(candidate)),
                            );
                        }
                        menu
                    }),
            )
            .child(
                Button::new("theme")
                    .ghost()
                    .small()
                    .icon(if cx.theme().is_dark() {
                        IconName::Moon
                    } else {
                        IconName::Sun
                    })
                    .label(text(locale, Key::SettingsTheme))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_theme(&ToggleTheme, window, cx)
                    })),
            )
            .child(
                Button::new("settings")
                    .ghost()
                    .small()
                    .icon(IconName::Settings)
                    .accessibility_label(text(locale, Key::SettingsMenu))
                    .native_tooltip(text(locale, Key::SettingsMenu))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.open_settings(&OpenSettings, window, cx)
                    })),
            )
            .into_any_element()
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let locale = i18n::current(cx);
        let preference = cx.global::<LanguageState>().preference;
        let revision = cx.global::<LanguageState>().revision;
        let reload_available = self
            .active
            .and_then(|index| self.documents.get(index))
            .is_some_and(|document| {
                !matches!(
                    document.status,
                    DocumentStatus::Queued | DocumentStatus::Reading
                )
            });
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        let fit_available = self
            .active
            .and_then(|index| self.documents.get(index))
            .is_some_and(|document| document.viewport.is_some());
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        let fit_available = false;
        let menu_changed = i18n::set_document_commands(
            i18n::DocumentMenuState {
                reload_available,
                fit_available,
                has_document: !self.documents.is_empty(),
                can_cycle: self.documents.len() > 1,
            },
            cx,
        );
        if revision != self.menu_revision || menu_changed {
            self.menu_bar.update(cx, |bar, cx| bar.reload(cx));
            self.menu_revision = revision;
        }
        // Reserve the native macOS traffic lights on the left. Bound the
        // content to the remaining space so long titles cannot displace controls.
        let title_left_padding = if cfg!(target_os = "macos") {
            px(80.0)
        } else {
            px(0.0)
        };
        let controls = window.window_controls();
        let control_count = 1 + u8::from(controls.minimize) + u8::from(controls.maximize);
        let controls_width = if cfg!(target_os = "macos") {
            px(0.0)
        } else {
            TITLE_BAR_HEIGHT * f32::from(control_count)
        };
        // TitleBar adds another 12 pixels inside its content in fullscreen.
        let fullscreen_padding = if window.is_fullscreen() {
            px(12.0)
        } else {
            px(0.0)
        };
        let title_content_width = (window.viewport_size().width
            - title_left_padding
            - controls_width
            - fullscreen_padding)
            .max(px(0.0));
        let tabs = self
            .documents
            .iter()
            .map(|document| {
                let file_name = document
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let format = match &document.status {
                    DocumentStatus::Imported(prepared) => {
                        let format = prepared.board.source.format.as_str();
                        Some(SharedString::from(format_display_name(format).to_owned()))
                    }
                    _ => None,
                };
                document_tabs::DocumentTab {
                    label: file_name.into(),
                    format,
                }
            })
            .collect::<Vec<_>>();
        #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
        let (title_search, title_trailing) = match self
            .active
            .and_then(|index| self.documents.get(index))
            .and_then(|document| document.viewport.clone())
        {
            Some(viewport) => {
                let (search, trailing) =
                    viewport.update(cx, |viewport, cx| viewport.title_views(cx));
                (
                    Some(search.into_any_element()),
                    Some(trailing.into_any_element()),
                )
            }
            None => (None, None),
        };
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        let (title_search, title_trailing): (Option<AnyElement>, Option<AnyElement>) = (None, None);
        let theme = cx.theme();
        div()
            .key_context("Pomelo")
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                for path in paths.paths() {
                    this.open_path(path.clone(), window, cx);
                }
            }))
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(cx.listener(Self::open_file))
            .on_action(cx.listener(Self::close_document))
            .on_action(cx.listener(Self::reload_document))
            .on_action(cx.listener(Self::cancel_import))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(Self::open_settings))
            .on_action(
                cx.listener(|_, _: &crate::actions::AboutPomelo, window, cx| {
                    crate::settings::open_about(window, cx)
                }),
            )
            .on_action(cx.listener(Self::next_document))
            .on_action(cx.listener(Self::previous_document))
            .on_action(cx.listener(Self::fit_active_board))
            .on_action(cx.listener(|this, _: &ToggleLeftPanel, window, cx| {
                this.toggle_panel(panel_layout::Side::Left, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleRightPanel, window, cx| {
                this.toggle_panel(panel_layout::Side::Right, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &crate::actions::FocusSearch, window, cx| {
                    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                    if let Some(viewport) = this
                        .active
                        .and_then(|index| this.documents.get(index))
                        .and_then(|document| document.viewport.as_ref())
                    {
                        viewport.update(cx, |viewport, cx| {
                            viewport.focus_search(&crate::viewport::FocusSearch, window, cx)
                        });
                    }
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ShowRecentFiles, _, cx| {
                    this.recent_only = true;
                    cx.notify();
                }),
            )
            .on_action(
                cx.listener(|this, _: &crate::actions::ShowStartPage, _, cx| {
                    this.recent_only = false;
                    cx.notify();
                }),
            )
            .on_action(cx.listener(|_, _: &i18n::SystemLanguage, _, cx| {
                i18n::select(LanguagePreference::System, cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::EnglishLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::English), cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::SimplifiedLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::SimplifiedChinese), cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::TraditionalLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::TraditionalChinese), cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::JapaneseLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::Japanese), cx);
            }))
            .on_action(cx.listener(|_, _: &i18n::KoreanLanguage, _, cx| {
                i18n::select(LanguagePreference::Explicit(Locale::Korean), cx);
            }))
            .child(
                TitleBar::new().h_11().pl(title_left_padding).child(
                    div()
                        .w(title_content_width)
                        .min_w_0()
                        .h_full()
                        .flex()
                        .items_center()
                        .gap_3()
                        .pl_3()
                        .pr_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .h_full()
                                .flex_shrink_0()
                                .child(self.logo.as_ref().map_or_else(
                                    || {
                                        Icon::new(IconName::CircuitBoard)
                                            .size_5()
                                            .text_color(theme.primary)
                                            .into_any_element()
                                    },
                                    |logo| {
                                        img(logo.clone())
                                            .size_5()
                                            .object_fit(ObjectFit::Contain)
                                            .into_any_element()
                                    },
                                ))
                                // macOS shows these menus in the system menu bar;
                                // other platforms have no native bar to rely on.
                                .when(!cfg!(target_os = "macos"), |row| {
                                    row.child(self.menu_bar.clone())
                                }),
                        )
                        .map(|bar| {
                            if tabs.is_empty() {
                                return bar.child(div().flex_1().min_w_0());
                            }
                            // Documents are the window's objects, so they share the
                            // title row; its empty space remains the drag region.
                            bar.child(
                                div()
                                    .flex_initial()
                                    .min_w_0()
                                    .max_w(relative(0.45))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(document_tabs::render(
                                        document_tabs::DocumentTabs {
                                            items: tabs,
                                            selected: self.active.unwrap_or(0),
                                            locale,
                                            scroll: self.document_scroll.clone(),
                                        },
                                        cx.listener(|this, index: &usize, window, cx| {
                                            this.activate_document(*index, window, cx);
                                        }),
                                        cx.listener(|this, index: &usize, window, cx| {
                                            this.activate_document(*index, window, cx);
                                            this.close_document(&CloseDocument, window, cx);
                                        }),
                                        window,
                                        cx,
                                    ))
                                    .child(
                                        Button::new("title-open-file")
                                            .ghost()
                                            .xsmall()
                                            .flex_shrink_0()
                                            .icon(IconName::Plus)
                                            .accessibility_label(text(locale, Key::OpenFile))
                                            .native_tooltip(format!(
                                                "{} ({})",
                                                text(locale, Key::OpenFile),
                                                crate::shortcuts::shortcut_label(
                                                    crate::shortcuts::keys::OPEN_FILE
                                                )
                                            ))
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_file(&OpenFile, window, cx)
                                            })),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .justify_center()
                                    .children(title_search),
                            )
                        })
                        .map(|bar| match title_trailing {
                            Some(trailing) => bar.child(trailing),
                            None => bar.child(self.app_preferences(locale, preference, cx)),
                        }),
                ),
            )
            .when(self.active.is_some() && !fit_available, |shell| {
                shell.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .p_3()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(
                            Button::new("open")
                                .ghost()
                                .icon(IconName::FolderOpen)
                                .label(text(locale, Key::OpenFile))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_file(&OpenFile, window, cx)
                                })),
                        )
                        .child(
                            Button::new("cancel-import")
                                .ghost()
                                .label(text(locale, Key::CancelImport))
                                .disabled(reload_available)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_import(&CancelImport, window, cx)
                                })),
                        )
                        .child(
                            Button::new("reload")
                                .ghost()
                                .icon(IconName::RotateCcw)
                                .label(text(locale, Key::ReloadDocument))
                                .disabled(!reload_available)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.reload_document(&ReloadDocument, window, cx)
                                })),
                        ),
                )
            })
            .child(self.content(window, cx))
            .when(fit_available, |shell| {
                let warning = self
                    .error
                    .as_ref()
                    .or(cx
                        .global::<crate::services::recent::RecentState>()
                        .warning
                        .as_ref())
                    .or(cx.global::<LanguageState>().warning.as_ref())
                    .or(cx.global::<crate::theme::ThemeState>().warning.as_ref());
                shell.when_some(warning, |shell, warning| {
                    shell.child(
                        div()
                            .px_4()
                            .py_2()
                            .text_sm()
                            .flex_shrink_0()
                            .text_color(theme.danger)
                            .child(warning.message.display(locale)),
                    )
                })
            })
            .when(!fit_available, |shell| {
                shell.child(
                    div()
                        .flex()
                        .items_center()
                        .flex_shrink_0()
                        .justify_between()
                        .px_4()
                        .h_9()
                        .border_t_1()
                        .border_color(theme.border)
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(
                            div().child(
                                self.error
                                    .as_ref()
                                    .or(cx
                                        .global::<crate::services::recent::RecentState>()
                                        .warning
                                        .as_ref())
                                    .or(cx.global::<LanguageState>().warning.as_ref())
                                    .or(cx.global::<crate::theme::ThemeState>().warning.as_ref())
                                    .map(|error| error.message.display(locale))
                                    .unwrap_or_else(|| {
                                        text(
                                            locale,
                                            if self.active.is_none() {
                                                Key::WaitingForFile
                                            } else {
                                                Key::LocalReadOnly
                                            },
                                        )
                                    }),
                            ),
                        )
                        .child(div().child(text(locale, Key::LocalFile))),
                )
            })
    }
}
