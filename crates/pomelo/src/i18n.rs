//! Application-owned language switching, menu updates and serialized persistence.

use gpui_kit::{App, AppContext, BorrowAppContext, Global, Menu, MenuItem, Task};
use pomelo_core::{
    i18n::{LanguagePreference, Locale, MessageKey, text},
    model::Diagnostic,
};

use crate::{
    actions::{
        CloseDocument, FitActiveBoard, NextDocument, OpenFile, OpenSettings, PreviousDocument,
        Quit, ReloadDocument, ToggleLeftPanel, ToggleRightPanel,
    },
    prefs::LanguageStore,
};

gpui_kit::actions!(
    pomelo,
    [
        SystemLanguage,
        EnglishLanguage,
        SimplifiedLanguage,
        TraditionalLanguage,
        JapaneseLanguage,
        KoreanLanguage
    ]
);

pub struct LanguageState {
    pub preference: LanguagePreference,
    pub locale: Locale,
    pub warning: Option<Diagnostic>,
    store: Option<LanguageStore>,
    save_task: Option<Task<()>>,
    pub revision: u64,
}
impl Global for LanguageState {}

#[derive(Default, PartialEq, Eq)]
pub struct DocumentMenuState {
    pub reload_available: bool,
    pub fit_available: bool,
    pub has_document: bool,
    pub can_cycle: bool,
}
impl Global for DocumentMenuState {}

pub fn set_document_commands(state: DocumentMenuState, cx: &mut App) -> bool {
    if *cx.global::<DocumentMenuState>() == state {
        return false;
    }
    *cx.global_mut::<DocumentMenuState>() = state;
    rebuild_menus(cx);
    true
}

pub fn initialize(cx: &mut App, override_locale: Option<Locale>) {
    cx.set_global(DocumentMenuState::default());
    let store = LanguageStore::platform_default();
    let (preference, warning) = match &store {
        Some(store) => match store.load() {
            Ok(preference) => (preference, None),
            Err(error) => (LanguagePreference::System, Some(error)),
        },
        None => (
            LanguagePreference::System,
            Some(Diagnostic::error(
                "CONFIG_DIRECTORY_MISSING",
                MessageKey::ConfigDirectoryMissing,
            )),
        ),
    };
    // A command-line override is transient and never overwrites the saved preference.
    let locale =
        override_locale.unwrap_or_else(|| preference.resolve(sys_locale::get_locale().as_deref()));
    gpui_kit::component::set_locale(locale.tag());
    cx.set_global(LanguageState {
        preference,
        locale,
        warning,
        store,
        save_task: None,
        revision: 0,
    });
    rebuild_menus(cx);
    cx.on_app_quit(|cx| {
        let pending = cx.update_global::<LanguageState, _>(|state, _| state.save_task.take());
        async move {
            if let Some(pending) = pending {
                pending.await;
            }
        }
    })
    .detach();
}

pub fn current(cx: &App) -> Locale {
    cx.global::<LanguageState>().locale
}

/// Each save awaits its predecessor, so rapid choices cannot reorder disk writes.
pub fn select(preference: LanguagePreference, cx: &mut App) {
    let locale = preference.resolve(sys_locale::get_locale().as_deref());
    let (previous, store, revision) = cx
        .update_global::<LanguageState, _>(|state, _| {
            let revision = state.revision.checked_add(1)?;
            state.revision = revision;
            state.preference = preference;
            state.locale = locale;
            state.warning = None;
            Some((state.save_task.take(), state.store.clone(), revision))
        })
        .unwrap_or_else(|| (None, None, 0));
    if revision == 0 {
        return;
    }
    gpui_kit::component::set_locale(locale.tag());
    crate::theme::apply_ui_font(cx);
    rebuild_menus(cx);
    cx.refresh_windows();
    let task = cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            previous.await;
        }
        let result = cx
            .background_spawn(async move {
                match store {
                    Some(store) => store.save(preference),
                    None => Err(Diagnostic::error(
                        "CONFIG_DIRECTORY_MISSING",
                        MessageKey::ConfigDirectoryMissing,
                    )),
                }
            })
            .await;
        cx.update_global::<LanguageState, _>(|state, cx| {
            if state.revision == revision {
                state.warning = result.err();
                cx.refresh_windows();
            }
        });
    });
    cx.update_global::<LanguageState, _>(|state, _| state.save_task = Some(task));
}

pub fn action(preference: LanguagePreference) -> Box<dyn gpui_kit::Action> {
    match preference {
        LanguagePreference::System => Box::new(SystemLanguage),
        LanguagePreference::Explicit(Locale::English) => Box::new(EnglishLanguage),
        LanguagePreference::Explicit(Locale::SimplifiedChinese) => Box::new(SimplifiedLanguage),
        LanguagePreference::Explicit(Locale::TraditionalChinese) => Box::new(TraditionalLanguage),
        LanguagePreference::Explicit(Locale::Japanese) => Box::new(JapaneseLanguage),
        LanguagePreference::Explicit(Locale::Korean) => Box::new(KoreanLanguage),
    }
}

fn rebuild_menus(cx: &mut App) {
    let state = cx.global::<LanguageState>();
    let locale = state.locale;
    let mut languages = vec![
        MenuItem::action(text(locale, MessageKey::FollowSystem), SystemLanguage)
            .checked(state.preference == LanguagePreference::System),
        MenuItem::separator(),
    ];
    for language in Locale::ALL {
        let preference = LanguagePreference::Explicit(language);
        languages.push(MenuItem::Action {
            name: language.native_name().into(),
            action: action(preference),
            os_action: None,
            checked: state.preference == preference,
            disabled: false,
        });
    }
    let menus = vec![
        Menu::new(text(locale, MessageKey::FileMenu)).items([
            MenuItem::action(text(locale, MessageKey::OpenFile), OpenFile),
            MenuItem::action(text(locale, MessageKey::ReloadDocument), ReloadDocument)
                .disabled(!cx.global::<DocumentMenuState>().reload_available),
            MenuItem::action(text(locale, MessageKey::CloseDocument), CloseDocument)
                .disabled(!cx.global::<DocumentMenuState>().has_document),
            MenuItem::separator(),
            MenuItem::action(text(locale, MessageKey::SettingsMenu), OpenSettings),
            MenuItem::separator(),
            MenuItem::action(text(locale, MessageKey::Quit), Quit),
        ]),
        Menu::new(text(locale, MessageKey::ViewMenu)).items([
            MenuItem::action(text(locale, MessageKey::NextDocument), NextDocument)
                .disabled(!cx.global::<DocumentMenuState>().can_cycle),
            MenuItem::action(text(locale, MessageKey::PreviousDocument), PreviousDocument)
                .disabled(!cx.global::<DocumentMenuState>().can_cycle),
            MenuItem::separator(),
            MenuItem::action(text(locale, MessageKey::FitBoard), FitActiveBoard)
                .disabled(!cx.global::<DocumentMenuState>().fit_available),
            MenuItem::separator(),
            MenuItem::action(text(locale, MessageKey::ToggleLeftPanel), ToggleLeftPanel)
                .disabled(!cx.global::<DocumentMenuState>().fit_available),
            MenuItem::action(text(locale, MessageKey::ToggleRightPanel), ToggleRightPanel)
                .disabled(!cx.global::<DocumentMenuState>().fit_available),
        ]),
        Menu::new(text(locale, MessageKey::ToolsMenu)).items([
            MenuItem::action(text(locale, MessageKey::SettingsMenu), OpenSettings),
            MenuItem::submenu(Menu::new(text(locale, MessageKey::LanguageMenu)).items(languages)),
        ]),
        Menu::new(text(locale, MessageKey::HelpMenu)).items([MenuItem::action(
            text(locale, MessageKey::AboutPomelo),
            crate::actions::AboutPomelo,
        )]),
    ];
    cx.set_menus(menus);
    let owned = cx.get_menus().unwrap_or_default();
    gpui_kit::base::GlobalState::global_mut(cx).set_app_menus(owned);
}
