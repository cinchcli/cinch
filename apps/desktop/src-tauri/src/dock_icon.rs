//! macOS Dock-icon theming.
//!
//! The bundle ships one dark icon (cream mark on an ink squircle). While a
//! window is on screen the app is a Regular app (see
//! `window_manage::set_dock_visible`) and a Dock icon appears; this module
//! makes that Dock icon follow the system appearance via
//! `[NSApp setApplicationIconImage:]`.
//!
//! Concerns are split: this module owns the *image* (which variant + reacting
//! to `ThemeChanged`); `window_manage` owns *presence* (Regular vs Accessory).
//! `applicationIconImage` persists across activation-policy changes, so
//! staging it at startup means no default-icon flash when the policy first
//! flips to Regular.

use tauri::Theme;

/// Dark default (cream mark on ink squircle) — also the bundle icon.
const ICON_DARK: &[u8] = include_bytes!("../icons/icon.png");
/// Light variant (ink mark on cream squircle).
const ICON_LIGHT: &[u8] = include_bytes!("../icons/icon-light.png");

/// Pick the Dock-icon bytes for a system appearance. Pure; unit-tested.
pub(crate) fn icon_bytes_for(theme: Theme) -> &'static [u8] {
    match theme {
        Theme::Light => ICON_LIGHT,
        // Dark, and any future/unknown (`Theme` is non-exhaustive) -> dark.
        _ => ICON_DARK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_differ() {
        let light = icon_bytes_for(Theme::Light);
        let dark = icon_bytes_for(Theme::Dark);
        assert!(!light.is_empty(), "light icon embedded");
        assert!(!dark.is_empty(), "dark icon embedded");
        assert_ne!(light, dark, "light and dark icons must differ");
    }
}
