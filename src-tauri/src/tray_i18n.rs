//! Tray menu internationalization
//!
//! Everything is auto-generated at compile time by build.rs from the
//! frontend locale files (src/i18n/locales/*/translation.json).
//!
//! The English translation.json is the single source of truth:
//! - TrayStrings struct fields are derived from the English "tray" keys
//! - All languages are auto-discovered from the locales directory
//!
//! To add a new tray menu item:
//! 1. Add the key to en/translation.json under "tray"
//! 2. Add translations to other locale files
//! 3. Update tray.rs to use the new field (e.g., strings.new_field)

use once_cell::sync::Lazy;
use std::collections::HashMap;

// Include the auto-generated TrayStrings struct and TRANSLATIONS static
include!(concat!(env!("OUT_DIR"), "/tray_translations.rs"));

/// Get localized tray menu strings based on the system locale.
///
/// Lookup order: full locale (e.g. "zh-TW") → language code ("zh") → English.
pub fn get_tray_translations(locale: Option<String>) -> TrayStrings {
    let locale_str = locale.as_deref().unwrap_or("en");
    let lang_code = locale_str.split(['-', '_']).next().unwrap_or("en");

    TRANSLATIONS
        .get(locale_str)
        .or_else(|| TRANSLATIONS.get(lang_code))
        .or_else(|| TRANSLATIONS.get("en"))
        .or_else(|| TRANSLATIONS.values().next())
        .cloned()
        .unwrap_or_else(fallback_tray_strings)
}

fn fallback_tray_strings() -> TrayStrings {
    TrayStrings {
        settings: "Settings".to_string(),
        check_updates: "Check for updates".to_string(),
        copy_last_transcript: "Copy last transcript".to_string(),
        unload_model: "Unload model".to_string(),
        model: "Model".to_string(),
        quit: "Quit".to_string(),
        cancel: "Cancel".to_string(),
    }
}
