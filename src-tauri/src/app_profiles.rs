//! Shared frontmost-application lookup and per-app dictation profile rules.
//!
//! Resolves the active application's display name, bundle identifier, and
//! executable stem in-process without reading any screen text or window title,
//! and maps application kinds and profile tones to capitalization and
//! punctuation surface styles.

use crate::settings::CorrectionPair;
use serde::{Deserialize, Serialize};
use specta::Type;

const CHAT_APPS: &str = "com.tinyspeck.slackmacgap|slack|com.apple.mobilesms|messages|com.hnc.discord|discord|ru.keepcoder.telegram|telegram|net.whatsapp.whatsapp|whatsapp|org.whispersystems.signal-desktop|signal";
const EMAIL_APPS: &str = "com.apple.mail|mail|com.microsoft.outlook|microsoft outlook|com.superhuman.electron|superhuman|org.mozilla.thunderbird|thunderbird";
const CODE_APPS: &str = "com.todesktop.230313mzl4w4u92|cursor|com.microsoft.vscode|code|visual studio code|com.apple.dt.xcode|xcode|dev.zed.zed|zed|com.googlecode.iterm2|iterm2|com.apple.terminal|terminal";
const DOC_APPS: &str =
    "notion.id|notion|com.apple.notes|notes|com.apple.iwork.pages|pages|md.obsidian|obsidian";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppTone {
    #[default]
    Default,
    Auto,
    Casual,
    Direct,
    Formal,
    Code,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapitalizationStyle {
    #[default]
    Default,
    Sentence,
    Lowercase,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum PunctuationStyle {
    #[default]
    Default,
    Full,
    DropTrailingPeriod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SurfaceStyle {
    pub capitalization: CapitalizationStyle,
    pub punctuation: PunctuationStyle,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    Chat,
    Email,
    Code,
    Document,
    Other,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Type, Default)]
pub struct ActiveAppInfo {
    pub app_name: String,
    pub bundle_id: Option<String>,
    pub process_stem: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type, Default)]
pub struct AppProfile {
    pub id: String,
    pub name: String,
    pub app_identifier: String,
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub tone: AppTone,
    #[serde(default)]
    pub capitalization: CapitalizationStyle,
    #[serde(default)]
    pub punctuation: PunctuationStyle,
    #[serde(default)]
    pub post_process_enabled: Option<bool>,
    #[serde(default)]
    pub prompt_id: Option<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub corrections: Vec<CorrectionPair>,
}

pub fn is_self_identifier(value: &str) -> bool {
    let t = value.trim();
    t.eq_ignore_ascii_case("Coco Voice")
        || t.eq_ignore_ascii_case("coco-voice")
        || t.eq_ignore_ascii_case("com.cocoresearch.cocovoice")
}

pub fn is_self_app(info: &ActiveAppInfo) -> bool {
    is_self_identifier(&info.app_name)
        || info.bundle_id.as_deref().is_some_and(is_self_identifier)
        || info.process_stem.as_deref().is_some_and(is_self_identifier)
}

pub fn filter_active_app(mut info: ActiveAppInfo) -> Option<ActiveAppInfo> {
    info.app_name = info.app_name.trim().to_string();
    info.bundle_id = info
        .bundle_id
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    info.process_stem = info
        .process_stem
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if info.app_name.is_empty() {
        info.app_name = info.process_stem.clone().unwrap_or_default();
    }
    if (info.app_name.is_empty() && info.bundle_id.is_none() && info.process_stem.is_none())
        || is_self_app(&info)
    {
        return None;
    }
    Some(info)
}

// ponytail: ceiling = macOS NSWorkspace frontmostApplication (0 subprocesses, 0 screen/window-title reads); upgrade path = foreground process query on Windows/Linux.
pub fn read_active_app() -> Option<ActiveAppInfo> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        use std::path::Path;

        let front = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let app_name = front
            .localizedName()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let bundle_id = front.bundleIdentifier().map(|s| s.to_string());
        let process_stem = front.executableURL().and_then(|u| u.path()).and_then(|p| {
            let s = p.to_string();
            Path::new(&s)
                .file_stem()
                .and_then(|st| st.to_str())
                .map(str::to_string)
        });
        filter_active_app(ActiveAppInfo {
            app_name,
            bundle_id,
            process_stem,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn app_profile_matches(saved: &str, info: &ActiveAppInfo) -> bool {
    let p = saved.trim();
    if p.is_empty() || is_self_app(info) || is_self_identifier(p) {
        return false;
    }
    let eq = |c: &str| c.trim().eq_ignore_ascii_case(p);
    info.bundle_id.as_deref().is_some_and(eq)
        || (!info.app_name.is_empty() && eq(&info.app_name))
        || info.process_stem.as_deref().is_some_and(eq)
}

pub fn classify_app_kind(info: &ActiveAppInfo) -> AppKind {
    let fields = [
        info.bundle_id.as_deref().unwrap_or(""),
        info.app_name.as_str(),
        info.process_stem.as_deref().unwrap_or(""),
    ];
    let matches_list = |list: &str| {
        list.split('|')
            .any(|item| fields.iter().any(|f| f.trim().eq_ignore_ascii_case(item)))
    };
    if matches_list(CHAT_APPS) {
        AppKind::Chat
    } else if matches_list(EMAIL_APPS) {
        AppKind::Email
    } else if matches_list(CODE_APPS) {
        AppKind::Code
    } else if matches_list(DOC_APPS) {
        AppKind::Document
    } else {
        AppKind::Other
    }
}

pub fn tone_defaults(tone: AppTone, app: Option<&ActiveAppInfo>) -> SurfaceStyle {
    let effective = match tone {
        AppTone::Auto => match app.map(classify_app_kind).unwrap_or(AppKind::Other) {
            AppKind::Chat => AppTone::Casual,
            AppKind::Email => AppTone::Formal,
            AppKind::Code => AppTone::Code,
            AppKind::Document => AppTone::Direct,
            AppKind::Other => AppTone::Default,
        },
        other => other,
    };
    match effective {
        AppTone::Default | AppTone::Auto => SurfaceStyle::default(),
        AppTone::Casual => SurfaceStyle {
            capitalization: CapitalizationStyle::Lowercase,
            punctuation: PunctuationStyle::DropTrailingPeriod,
        },
        AppTone::Direct | AppTone::Formal => SurfaceStyle {
            capitalization: CapitalizationStyle::Sentence,
            punctuation: PunctuationStyle::Full,
        },
        AppTone::Code => SurfaceStyle {
            capitalization: CapitalizationStyle::Default,
            punctuation: PunctuationStyle::DropTrailingPeriod,
        },
    }
}

pub fn apply_surface_style(text: &str, style: SurfaceStyle) -> String {
    if style == SurfaceStyle::default() || text.trim().is_empty() {
        return text.to_string();
    }
    let trimmed = text.trim();
    let lead = &text[..text.len() - text.trim_start().len()];
    let trail = &text[text.trim_end().len()..];
    let mut body = match style.capitalization {
        CapitalizationStyle::Default => trimmed.to_string(),
        CapitalizationStyle::Sentence => apply_sentence_caps(trimmed),
        CapitalizationStyle::Lowercase => apply_lowercase_starts(trimmed),
    };
    match style.punctuation {
        PunctuationStyle::Default => {}
        PunctuationStyle::DropTrailingPeriod if body.ends_with('.') && !body.ends_with("..") => {
            body.pop();
        }
        PunctuationStyle::DropTrailingPeriod => {}
        PunctuationStyle::Full => {
            if body
                .chars()
                .last()
                .is_some_and(|c| !matches!(c, '.' | '!' | '?' | ':' | ';'))
            {
                body.push('.');
            }
        }
    }
    format!("{lead}{body}{trail}")
}

fn apply_sentence_caps(text: &str) -> String {
    let (mut out, mut cap) = (String::with_capacity(text.len()), true);
    for ch in text.chars() {
        if cap && ch.is_alphabetic() {
            out.extend(ch.to_uppercase());
            cap = false;
        } else {
            out.push(ch);
            cap = if matches!(ch, '.' | '!' | '?') {
                true
            } else if ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | '[') {
                cap
            } else {
                false
            };
        }
    }
    out
}

fn apply_lowercase_starts(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut lower) = (String::with_capacity(text.len()), true);
    for i in 0..chars.len() {
        let ch = chars[i];
        if lower && ch.is_alphabetic() {
            let end = chars[i + 1..]
                .iter()
                .position(|c| !(c.is_alphanumeric() || matches!(c, '\'' | '\u{2019}')))
                .map_or(chars.len(), |p| i + 1 + p);
            let word: String = chars[i..end].iter().collect();
            let keep = chars[i + 1..end].iter().any(|c| c.is_uppercase())
                || word == "I"
                || word.starts_with("I'")
                || word.starts_with("I\u{2019}");
            if ch.is_uppercase() && !keep {
                out.extend(ch.to_lowercase());
            } else {
                out.push(ch);
            }
            lower = false;
        } else {
            out.push(ch);
            lower = if matches!(ch, '.' | '!' | '?') {
                true
            } else if ch.is_whitespace() || matches!(ch, '"' | '\'' | '(' | '[') {
                lower
            } else {
                false
            };
        }
    }
    out
}

#[cfg(test)]
mod per_app_tests {
    use super::*;

    fn app(name: &str, bundle: Option<&str>, stem: Option<&str>) -> ActiveAppInfo {
        ActiveAppInfo {
            app_name: name.into(),
            bundle_id: bundle.map(Into::into),
            process_stem: stem.map(Into::into),
        }
    }

    #[test]
    fn frontmost_app_filter_excludes_self_identifiers_and_keeps_external_apps() {
        assert_eq!(
            filter_active_app(app(
                "Coco Voice",
                Some("com.cocoresearch.cocovoice"),
                Some("coco-voice"),
            )),
            None
        );
        assert_eq!(
            filter_active_app(app(
                "Custom Build",
                Some("com.cocoresearch.cocovoice"),
                None
            )),
            None
        );
        assert_eq!(
            filter_active_app(app("Custom Build", None, Some("coco-voice"))),
            None
        );
        let mail = filter_active_app(app(" Mail ", Some("com.apple.mail"), Some("Mail"))).unwrap();
        assert_eq!(mail.app_name, "Mail");
    }

    #[test]
    fn per_app_matching_kind_and_surface_style_preserve_words() {
        let slack = app("Slack", Some("com.tinyspeck.slackmacgap"), Some("Slack"));
        assert!(app_profile_matches("com.tinyspeck.slackmacgap", &slack));
        assert!(app_profile_matches("SLACK", &slack));
        assert!(!app_profile_matches(" ", &slack));

        let chat_style = tone_defaults(AppTone::Auto, Some(&slack));
        assert_eq!(
            apply_surface_style("Sounds good to me.", chat_style),
            "sounds good to me"
        );
        let mail = app("Mail", Some("com.apple.mail"), None);
        assert_eq!(
            apply_surface_style(
                "thanks for the note. see you soon",
                tone_defaults(AppTone::Auto, Some(&mail))
            ),
            "Thanks for the note. See you soon."
        );
        let vscode = app("Code", Some("com.microsoft.VSCode"), Some("Electron"));
        assert_eq!(classify_app_kind(&vscode), AppKind::Code);
        assert_eq!(
            apply_surface_style(
                "cargo test useState.",
                tone_defaults(AppTone::Code, Some(&vscode))
            ),
            "cargo test useState"
        );
    }
}
