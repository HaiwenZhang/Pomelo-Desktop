//! Platform-aware shortcut hints derived from the same keystrokes the app binds.

/// Keystrokes shared by the key bindings and every on-screen hint, so the two cannot
/// drift. `secondary` is Command on macOS and Control elsewhere, exactly as GPUI binds it.
pub mod keys {
    pub const OPEN_FILE: &str = "secondary-o";
    pub const FOCUS_SEARCH: &str = "secondary-k";
    pub const FIT_ACTIVE_BOARD: &str = "f2";
    pub const TOGGLE_LEFT_PANEL: &str = "secondary-b";
    pub const TOGGLE_RIGHT_PANEL: &str = "secondary-shift-b";
    pub const NAVIGATE_BACK: &str = "alt-left";
    pub const NAVIGATE_FORWARD: &str = "alt-right";
}

/// Formats a binding such as `secondary-shift-b` with the current platform's names:
/// `⇧⌘B` on macOS, `Ctrl+Shift+B` elsewhere.
pub fn shortcut_label(keystroke: &str) -> String {
    shortcut_label_for(keystroke, cfg!(target_os = "macos"))
}

fn shortcut_label_for(keystroke: &str, mac: bool) -> String {
    let mut parts: Vec<&str> = keystroke.split('-').collect();
    // A trailing empty part means the key itself is `-`, as in `secondary--`.
    let key = match parts.pop() {
        Some("") => "-".to_owned(),
        Some("left") => "←".to_owned(),
        Some("right") => "→".to_owned(),
        Some("up") => "↑".to_owned(),
        Some("down") => "↓".to_owned(),
        Some("escape") => "Esc".to_owned(),
        Some(key) => key.to_uppercase(),
        None => String::new(),
    };
    let (mut control, mut alt, mut shift, mut command) = (false, false, false, false);
    for modifier in parts.into_iter().filter(|part| !part.is_empty()) {
        match modifier {
            "secondary" if mac => command = true,
            "secondary" | "ctrl" => control = true,
            "cmd" => command = true,
            "alt" => alt = true,
            "shift" => shift = true,
            _ => {}
        }
    }
    if mac {
        // Apple's modifier order: Control, Option, Shift, Command, then the key.
        [(control, "⌃"), (alt, "⌥"), (shift, "⇧"), (command, "⌘")]
            .into_iter()
            .filter_map(|(on, symbol)| on.then_some(symbol))
            .chain([key.as_str()])
            .collect()
    } else {
        [
            (control, "Ctrl"),
            (alt, "Alt"),
            (shift, "Shift"),
            (command, "Win"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .chain([key.as_str()])
        .collect::<Vec<_>>()
        .join("+")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secondary_shortcuts_use_command_on_macos() {
        assert_eq!(shortcut_label_for(keys::OPEN_FILE, true), "⌘O");
        assert_eq!(shortcut_label_for(keys::TOGGLE_RIGHT_PANEL, true), "⇧⌘B");
        assert_eq!(shortcut_label_for(keys::FIT_ACTIVE_BOARD, true), "F2");
    }

    #[test]
    fn secondary_shortcuts_use_control_elsewhere() {
        assert_eq!(shortcut_label_for(keys::FOCUS_SEARCH, false), "Ctrl+K");
        assert_eq!(
            shortcut_label_for(keys::TOGGLE_RIGHT_PANEL, false),
            "Ctrl+Shift+B"
        );
        assert_eq!(shortcut_label_for("secondary--", false), "Ctrl+-");
    }

    #[test]
    fn arrow_keys_use_arrow_symbols() {
        assert_eq!(shortcut_label_for(keys::NAVIGATE_BACK, true), "⌥←");
        assert_eq!(shortcut_label_for(keys::NAVIGATE_FORWARD, false), "Alt+→");
    }
}
